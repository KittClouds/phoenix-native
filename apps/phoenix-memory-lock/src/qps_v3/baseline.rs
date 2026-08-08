use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::time::Instant;

use anyhow::{bail, Context, Result};
use hashbrown::{HashMap, HashSet};
use phoenix_lexical_qps::{
    DocumentInput, Expansion, FieldConfig, LinearRankerV1, QpsBuilder, QpsConfig, QpsIndex,
    QueryGroup, SearchHit, SearchReceipt, SearchScratch, MAXIMUM_QUERY_GROUPS,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::receipt::{
    BaselineArtifacts, BaselineGates, BaselinePublication, BaselineReceiptV3, CandidateEvidenceV2,
    CohortBaseline, FileIdentity, FrozenFieldConfiguration, FrozenV2Configuration, LatencyReceipt,
    OracleLocation, QualityMetrics, QueryBaseline, QueryExecutionReceipt, BASELINE_CONTRACT,
    BASELINE_PUBLICATION_CONTRACT,
};
use crate::artifact::read_artifact;
use crate::baseline::verify_binding;
use crate::model::{
    FreezeManifest, GoldArtifact, HistorySession, WorkloadArtifact, GOLD_CONTRACT, GOLD_MAGIC,
    WORKLOAD_CONTRACT, WORKLOAD_MAGIC,
};

#[path = "evidence.rs"]
mod evidence;
pub(crate) use evidence::verify as verify_evidence;
#[path = "tiers.rs"]
mod tiers;
pub(crate) use tiers::verify as verify_tiers;
#[path = "ledger.rs"]
mod ledger;
pub(crate) use ledger::verify as verify_ledger;
#[path = "external_ledger.rs"]
mod external_ledger;
pub(crate) use external_ledger::qualify as qualify_external_ledger;
#[path = "corpus.rs"]
mod corpus;
#[path = "provenance.rs"]
mod provenance;
pub(crate) use corpus::audit as audit_corpus;
#[path = "split.rs"]
mod split;
pub(crate) use split::split as split_ledger;
#[path = "train.rs"]
mod train;
pub(crate) use train::train as train_linear;
#[path = "kernel_bench.rs"]
mod kernel_bench;
pub(crate) use kernel_bench::benchmark as benchmark_kernel;
#[path = "e2e_bench.rs"]
mod e2e_bench;
pub(crate) use e2e_bench::benchmark as benchmark_e2e;
#[path = "performance.rs"]
mod performance;
pub(crate) use performance::qualify as qualify_performance;
#[path = "promotion.rs"]
mod promotion;
pub(crate) use promotion::{promote, shadow};
#[path = "activation.rs"]
mod activation;
pub(crate) use activation::audit as audit_activation;
#[path = "tree_eligibility.rs"]
mod tree_eligibility;
pub(crate) use tree_eligibility::audit as audit_tree_eligibility;
#[path = "quality.rs"]
mod quality;
pub(crate) use quality::qualify as qualify_quality;

const MIXED_CONTRACT: &str = "phoenix.memory.qps-mixed-qualification/v1";
const ENGINE: &str = "phoenix-qps-v2.01-frozen-for-v3";
const TOP_K: usize = 10;
const CANDIDATE_CAP: usize = 160;
const MIXED_FIELDS: [FieldConfig; 2] = [
    FieldConfig::new("title", 2.5, 0.35, 0.35),
    FieldConfig::new("body", 1.0, 0.75, 0.10),
];
const LONGMEMEVAL_FIELDS: [FieldConfig; 1] = [FieldConfig::new("session", 1.0, 0.75, 0.0)];

#[allow(clippy::too_many_arguments)]
pub fn freeze(
    manifest: &FreezeManifest,
    manifest_path: &Path,
    suite_path: &Path,
    source_path: &Path,
    workload_path: &Path,
    gold_path: &Path,
    output_path: &Path,
    repetitions: usize,
) -> Result<BaselinePublication> {
    if repetitions == 0 || repetitions > 4_096 {
        bail!("repetitions must be in 1..=4096");
    }
    if output_path.exists() {
        bail!(
            "refusing to overwrite baseline receipt {}",
            output_path.display()
        );
    }

    let artifacts = BaselineArtifacts {
        freeze_manifest: file_identity(manifest_path)?,
        mixed_suite: file_identity(suite_path)?,
        source_corpus: file_identity(source_path)?,
        workload: file_identity(workload_path)?,
        gold: file_identity(gold_path)?,
    };
    let suite = load_suite(suite_path)?;
    let workload: WorkloadArtifact = read_artifact(workload_path, WORKLOAD_MAGIC)?;
    let gold: GoldArtifact = read_artifact(gold_path, GOLD_MAGIC)?;
    validate_release_inputs(manifest, &workload, &gold)?;

    let configuration = frozen_configuration();
    let configuration_bytes = serde_json::to_vec(&configuration)?;
    let configuration_sha256 = sha256_bytes(&configuration_bytes);
    let mixed_suite = capture_mixed(&suite, repetitions)?;
    let longmemeval_release = capture_longmemeval(&workload, &gold, repetitions)?;
    let source_matches =
        artifacts.source_corpus.sha256 == workload.source.sha256 && workload.source == gold.source;
    let gates = gates(
        source_matches,
        &configuration,
        &suite,
        &workload,
        &mixed_suite,
        &longmemeval_release,
    );
    let phase_1_verified = gates.all_pass();
    let receipt = BaselineReceiptV3 {
        contract: BASELINE_CONTRACT,
        architecture: "frozen_v2_retrieval_and_primitive_evidence_then_learned_v3_final_order",
        v2_binary: current_binary_identity()?,
        v2_configuration: configuration,
        v2_configuration_sha256: configuration_sha256,
        artifacts,
        mixed_suite,
        longmemeval_release,
        gates,
        phase_1_verified,
    };
    write_json_atomic(output_path, &receipt)?;
    let output = file_identity(output_path)?;
    Ok(BaselinePublication {
        contract: BASELINE_PUBLICATION_CONTRACT,
        output,
        mixed_metrics: receipt.mixed_suite.metrics,
        longmemeval_metrics: receipt.longmemeval_release.metrics,
        gates,
        phase_1_verified,
    })
}

fn capture_mixed(suite: &MixedSuite, repetitions: usize) -> Result<CohortBaseline> {
    let index = build_index(
        suite.documents.iter().enumerate().map(|(index, document)| {
            (
                index as u64,
                [document.title.as_str(), document.body.as_str()],
            )
        }),
        &MIXED_FIELDS,
    )?;
    let versions = suite
        .documents
        .iter()
        .map(mixed_document_version)
        .collect::<Vec<_>>();
    let mut queries = Vec::with_capacity(suite.queries.len());
    let mut aggregate_samples = Vec::with_capacity(suite.queries.len() * repetitions);
    let mut quality = QualityAccumulator::default();
    let mut growths = 0_u64;
    let mut determinism_failures = 0_u64;

    for query in &suite.queries {
        let prepared = PreparedMixedQuery::new(query);
        let groups = prepared.group_views();
        let mut scratch =
            SearchScratch::with_document_capacity(suite.documents.len(), MAXIMUM_QUERY_GROUPS);
        let mut evidence = Vec::<SearchHit>::with_capacity(CANDIDATE_CAP);
        let evidence_receipt =
            prepared.search_evidence(&groups, &index, &mut scratch, &mut evidence)?;
        let expected_order = evidence
            .iter()
            .take(TOP_K)
            .map(|hit| hit.external_id)
            .collect::<Vec<_>>();
        let mut serving = Vec::<SearchHit>::with_capacity(CANDIDATE_CAP);
        prepared.search(&groups, &index, &mut scratch, &mut serving)?;
        let mut samples = Vec::with_capacity(repetitions);
        let mut query_growths = 0_u64;
        for _ in 0..repetitions {
            let started = Instant::now();
            let receipt = prepared.search(&groups, &index, &mut scratch, &mut serving)?;
            let nanos = elapsed_nanos(started);
            samples.push(nanos);
            aggregate_samples.push(nanos);
            query_growths += u64::from(receipt.allocations_grew);
            determinism_failures += u64::from(!matches_order(&serving, &expected_order));
        }
        growths += query_growths;
        let expected = query.expected.as_deref();
        let oracle_rank = expected
            .and_then(|identity| {
                suite
                    .documents
                    .iter()
                    .position(|document| document.stable_id == identity)
            })
            .and_then(|document| {
                evidence
                    .iter()
                    .position(|hit| hit.external_id as usize == document)
            });
        quality.observe_rank(oracle_rank, expected.is_none(), evidence.is_empty());
        let candidates = evidence
            .iter()
            .enumerate()
            .map(|(rank, hit)| {
                let document = &suite.documents[hit.external_id as usize];
                CandidateEvidenceV2::from_hit(
                    hit,
                    document.stable_id.clone(),
                    versions[hit.external_id as usize].clone(),
                    rank + 1,
                )
            })
            .collect::<Vec<_>>();
        queries.push(QueryBaseline {
            query_identity: query.stable_id.clone(),
            query_shape: query.shape.label().to_owned(),
            query_sha256: sha256_bytes(&serde_json::to_vec(query)?),
            v2_final_order: candidates
                .iter()
                .take(TOP_K)
                .map(|candidate| candidate.document_identity.clone())
                .collect(),
            oracle_locations: oracle_locations(expected.into_iter(), &candidates),
            candidate_pool: candidates,
            latency: LatencyReceipt::from_samples(&mut samples),
            execution: QueryExecutionReceipt::from_search(evidence_receipt, query_growths),
        });
    }
    Ok(CohortBaseline {
        cohort: "frozen_mixed_suite_v1",
        metrics: quality.finish(),
        aggregate_latency: LatencyReceipt::from_samples(&mut aggregate_samples),
        warm_allocation_growths: growths,
        deterministic_ranking_failures: determinism_failures,
        maximum_candidate_pool: maximum_pool(&queries),
        queries,
    })
}

fn capture_longmemeval(
    workload: &WorkloadArtifact,
    gold: &GoldArtifact,
    repetitions: usize,
) -> Result<CohortBaseline> {
    let gold_by_query = gold
        .cases
        .iter()
        .map(|case| (case.question_id.as_str(), case))
        .collect::<HashMap<_, _>>();
    let mut queries = Vec::with_capacity(workload.cases.len());
    let mut aggregate_samples = Vec::with_capacity(workload.cases.len() * repetitions);
    let mut quality = QualityAccumulator::default();
    let mut growths = 0_u64;
    let mut determinism_failures = 0_u64;

    for case in &workload.cases {
        let gold_case = gold_by_query
            .get(case.question_id.as_str())
            .with_context(|| format!("gold missing query {}", case.question_id))?;
        let documents = case
            .sessions
            .iter()
            .map(|session| {
                session
                    .turns
                    .iter()
                    .map(|turn| turn.content.as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .collect::<Vec<_>>();
        let index = build_index(
            documents
                .iter()
                .enumerate()
                .map(|(index, document)| (index as u64, [document.as_str()])),
            &LONGMEMEVAL_FIELDS,
        )?;
        let versions = case
            .sessions
            .iter()
            .map(session_version)
            .collect::<Vec<_>>();
        let mut scratch =
            SearchScratch::with_document_capacity(documents.len(), MAXIMUM_QUERY_GROUPS);
        let mut evidence = Vec::<SearchHit>::with_capacity(CANDIDATE_CAP);
        let evidence_receipt =
            index.search_evidence_into(&case.question, TOP_K, &mut scratch, &mut evidence)?;
        let expected_order = evidence
            .iter()
            .take(TOP_K)
            .map(|hit| hit.external_id)
            .collect::<Vec<_>>();
        let mut serving = Vec::<SearchHit>::with_capacity(CANDIDATE_CAP);
        index.search_into(&case.question, TOP_K, &mut scratch, &mut serving)?;
        let mut samples = Vec::with_capacity(repetitions);
        let mut query_growths = 0_u64;
        for _ in 0..repetitions {
            let started = Instant::now();
            let receipt = index.search_into(&case.question, TOP_K, &mut scratch, &mut serving)?;
            let nanos = elapsed_nanos(started);
            samples.push(nanos);
            aggregate_samples.push(nanos);
            query_growths += u64::from(receipt.allocations_grew);
            determinism_failures += u64::from(!matches_order(&serving, &expected_order));
        }
        growths += query_growths;
        let oracle_rank = evidence.iter().position(|hit| {
            let identity = &case.sessions[hit.external_id as usize].stable_id;
            gold_case.answer_session_ids.contains(identity)
        });
        quality.observe_rank(
            oracle_rank,
            gold_case.answer_session_ids.is_empty(),
            evidence.is_empty(),
        );
        let candidates = evidence
            .iter()
            .enumerate()
            .map(|(rank, hit)| {
                let index = hit.external_id as usize;
                CandidateEvidenceV2::from_hit(
                    hit,
                    case.sessions[index].stable_id.clone(),
                    versions[index].clone(),
                    rank + 1,
                )
            })
            .collect::<Vec<_>>();
        queries.push(QueryBaseline {
            query_identity: case.question_id.clone(),
            query_shape: case.question_type.clone(),
            query_sha256: sha256_bytes(case.question.as_bytes()),
            v2_final_order: candidates
                .iter()
                .take(TOP_K)
                .map(|candidate| candidate.document_identity.clone())
                .collect(),
            oracle_locations: oracle_locations(
                gold_case.answer_session_ids.iter().map(String::as_str),
                &candidates,
            ),
            candidate_pool: candidates,
            latency: LatencyReceipt::from_samples(&mut samples),
            execution: QueryExecutionReceipt::from_search(evidence_receipt, query_growths),
        });
    }
    Ok(CohortBaseline {
        cohort: "frozen_longmemeval_release_500",
        metrics: quality.finish(),
        aggregate_latency: LatencyReceipt::from_samples(&mut aggregate_samples),
        warm_allocation_growths: growths,
        deterministic_ranking_failures: determinism_failures,
        maximum_candidate_pool: maximum_pool(&queries),
        queries,
    })
}

fn build_index<'a, const N: usize>(
    documents: impl Iterator<Item = (u64, [&'a str; N])>,
    fields: &[FieldConfig],
) -> Result<QpsIndex> {
    let mut builder = QpsBuilder::new(fields.to_vec().into_boxed_slice(), v2_config())?;
    for (external_id, values) in documents {
        builder.insert(DocumentInput {
            external_id,
            fields: &values,
        })?;
    }
    builder.build().map_err(Into::into)
}

fn v2_config() -> QpsConfig {
    QpsConfig {
        maximum_candidate_pool: CANDIDATE_CAP,
        maximum_query_groups: MAXIMUM_QUERY_GROUPS,
        learned_ranker: LinearRankerV1::disabled(),
        ..QpsConfig::default()
    }
}

fn frozen_configuration() -> FrozenV2Configuration {
    let config = v2_config();
    FrozenV2Configuration {
        engine: ENGINE,
        top_k: TOP_K,
        k1: config.k1,
        coverage_floor: config.coverage_floor,
        coverage_exponent: config.coverage_exponent,
        proximity_weight: config.proximity_weight,
        order_weight: config.order_weight,
        phrase_weight: config.phrase_weight,
        segment_weight: config.segment_weight,
        proximity_decay_tokens: config.proximity_decay_tokens,
        minimum_candidate_pool: config.minimum_candidate_pool,
        candidate_pool_multiplier: config.candidate_pool_multiplier,
        maximum_candidate_pool: config.maximum_candidate_pool,
        dense_simd_threshold: config.dense_simd_threshold,
        maximum_query_groups: config.maximum_query_groups,
        maximum_expansions_per_group: config.maximum_expansions_per_group,
        learned_ranker_enabled: config.learned_ranker.is_enabled(),
        learned_ranker_identity: hex(config.learned_ranker.identity()),
        mixed_fields: field_configuration(&MIXED_FIELDS),
        longmemeval_fields: field_configuration(&LONGMEMEVAL_FIELDS),
    }
}

fn field_configuration(fields: &[FieldConfig]) -> Vec<FrozenFieldConfiguration> {
    fields
        .iter()
        .map(|field| FrozenFieldConfiguration {
            name: field.name,
            weight: field.weight,
            length_normalization: field.length_normalization,
            exact_match_bonus: field.exact_match_bonus,
        })
        .collect()
}

fn validate_release_inputs(
    manifest: &FreezeManifest,
    workload: &WorkloadArtifact,
    gold: &GoldArtifact,
) -> Result<()> {
    if workload.contract != WORKLOAD_CONTRACT || gold.contract != GOLD_CONTRACT {
        bail!("unsupported LongMemEval typed artifact contract");
    }
    verify_binding(manifest, &workload.source)?;
    verify_binding(manifest, &gold.source)?;
    if workload.source != gold.source || workload.cases.len() != gold.cases.len() {
        bail!("workload and gold do not describe the same frozen cohort");
    }
    let workload_ids = workload
        .cases
        .iter()
        .map(|case| case.question_id.as_str())
        .collect::<HashSet<_>>();
    if workload_ids.len() != workload.cases.len()
        || gold
            .cases
            .iter()
            .any(|case| !workload_ids.contains(case.question_id.as_str()))
    {
        bail!("workload and gold question identities are incomplete or duplicated");
    }
    Ok(())
}

fn gates(
    artifacts_match: bool,
    config: &FrozenV2Configuration,
    suite: &MixedSuite,
    workload: &WorkloadArtifact,
    mixed: &CohortBaseline,
    long: &CohortBaseline,
) -> BaselineGates {
    let every_query_recorded =
        mixed.queries.len() == suite.queries.len() && long.queries.len() == workload.cases.len();
    let cohorts = [&mixed.queries, &long.queries];
    let every_candidate_recorded = cohorts.iter().all(|queries| {
        queries
            .iter()
            .all(|query| query.candidate_pool.len() == query.execution.reranked_candidates as usize)
    });
    let all_evidence_finite = cohorts.iter().all(|queries| {
        queries
            .iter()
            .flat_map(|query| &query.candidate_pool)
            .all(CandidateEvidenceV2::is_finite)
    });
    BaselineGates {
        artifact_hashes_match_frozen_inputs: artifacts_match,
        v2_ranker_disabled: !config.learned_ranker_enabled,
        candidate_cap_is_160: config.maximum_candidate_pool == CANDIDATE_CAP
            && mixed.maximum_candidate_pool <= CANDIDATE_CAP
            && long.maximum_candidate_pool <= CANDIDATE_CAP,
        every_frozen_query_recorded: every_query_recorded,
        every_reranked_candidate_recorded: every_candidate_recorded,
        all_evidence_finite,
        zero_warm_allocation_growth: mixed.warm_allocation_growths == 0
            && long.warm_allocation_growths == 0,
        deterministic_ranking_failures_are_zero: mixed.deterministic_ranking_failures == 0
            && long.deterministic_ranking_failures == 0,
        mixed_hit_at_10_is_1: approx(mixed.metrics.hit_at_10, 1.0),
        mixed_mrr_is_1: approx(mixed.metrics.mean_reciprocal_rank, 1.0),
        longmemeval_hit_at_10_is_098: approx(long.metrics.hit_at_10, 0.98),
        longmemeval_mrr_is_0891005: approx(
            long.metrics.mean_reciprocal_rank,
            0.891_004_761_904_761_6,
        ),
    }
}

fn oracle_locations<'a>(
    expected: impl Iterator<Item = &'a str>,
    candidates: &[CandidateEvidenceV2],
) -> Vec<OracleLocation> {
    expected
        .map(|identity| {
            let rank = candidates
                .iter()
                .position(|candidate| candidate.document_identity == identity)
                .map(|rank| u16::try_from(rank + 1).unwrap_or(u16::MAX));
            OracleLocation {
                document_identity: identity.to_owned(),
                candidate_pool_rank: rank,
                top_10_rank: rank.filter(|rank| usize::from(*rank) <= TOP_K),
            }
        })
        .collect()
}

fn maximum_pool(queries: &[QueryBaseline]) -> usize {
    queries
        .iter()
        .map(|query| query.candidate_pool.len())
        .max()
        .unwrap_or(0)
}

fn matches_order(hits: &[SearchHit], expected: &[u64]) -> bool {
    hits.len() == expected.len()
        && hits
            .iter()
            .zip(expected)
            .all(|(hit, expected)| hit.external_id == *expected)
}

fn mixed_document_version(document: &MixedDocument) -> String {
    let mut hasher = Sha256::new();
    update_len_prefixed(&mut hasher, document.stable_id.as_bytes());
    update_len_prefixed(&mut hasher, document.title.as_bytes());
    update_len_prefixed(&mut hasher, document.body.as_bytes());
    hex(hasher.finalize().into())
}

fn session_version(session: &HistorySession) -> String {
    let mut hasher = Sha256::new();
    update_len_prefixed(&mut hasher, session.stable_id.as_bytes());
    update_len_prefixed(&mut hasher, session.date.as_bytes());
    for turn in &session.turns {
        update_len_prefixed(&mut hasher, turn.role.as_bytes());
        update_len_prefixed(&mut hasher, turn.content.as_bytes());
    }
    hex(hasher.finalize().into())
}

fn update_len_prefixed(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

fn load_suite(path: &Path) -> Result<MixedSuite> {
    let bytes = fs::read(path).with_context(|| format!("read mixed suite {}", path.display()))?;
    let suite: MixedSuite = serde_json::from_slice(&bytes)
        .with_context(|| format!("decode mixed suite {}", path.display()))?;
    suite.validate()?;
    Ok(suite)
}

fn file_identity(path: &Path) -> Result<FileIdentity> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let bytes = file.metadata()?.len();
    let mut reader = BufReader::with_capacity(1024 * 1024, file);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(FileIdentity {
        path: path.display().to_string(),
        bytes,
        sha256: hex(hasher.finalize().into()),
    })
}

fn current_binary_identity() -> Result<FileIdentity> {
    file_identity(&std::env::current_exe().context("resolve current V2 binary")?)
}

fn write_json_atomic<T: Serialize>(path: &Path, receipt: &T) -> Result<()> {
    let parent = path.parent().context("baseline output has no parent")?;
    fs::create_dir_all(parent)?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("baseline output filename is not UTF-8")?;
    let temporary = parent.join(format!(".{name}.tmp-{}", std::process::id()));
    let result = (|| {
        let file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        let mut writer = BufWriter::new(file);
        serde_json::to_writer(&mut writer, receipt)?;
        writer.write_all(b"\n")?;
        writer.flush()?;
        writer.get_ref().sync_all()?;
        fs::rename(&temporary, path)
            .with_context(|| format!("publish baseline receipt {}", path.display()))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let digest: [u8; 32] = Sha256::digest(bytes).into();
    hex(digest)
}

fn hex(bytes: [u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn elapsed_nanos(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX)
}

fn approx(left: f64, right: f64) -> bool {
    (left - right).abs() <= 1.0e-12
}

#[derive(Default)]
struct QualityAccumulator {
    answerable: usize,
    hits: usize,
    reciprocal_rank: f64,
    top_1: usize,
    no_result: usize,
    correct_no_result: usize,
}

impl QualityAccumulator {
    fn observe_rank(&mut self, rank: Option<usize>, is_no_result: bool, returned_empty: bool) {
        if is_no_result {
            self.no_result += 1;
            self.correct_no_result += usize::from(returned_empty);
            return;
        }
        self.answerable += 1;
        if let Some(rank) = rank.filter(|rank| *rank < TOP_K) {
            self.hits += 1;
            self.reciprocal_rank += 1.0 / (rank + 1) as f64;
            self.top_1 += usize::from(rank == 0);
        }
    }

    fn finish(self) -> QualityMetrics {
        QualityMetrics {
            answerable_queries: self.answerable,
            hit_at_10: ratio(self.hits, self.answerable),
            mean_reciprocal_rank: self.reciprocal_rank / self.answerable.max(1) as f64,
            top_1_accuracy: ratio(self.top_1, self.answerable),
            no_result_queries: self.no_result,
            no_result_accuracy: ratio(self.correct_no_result, self.no_result),
        }
    }
}

fn ratio(numerator: usize, denominator: usize) -> f64 {
    numerator as f64 / denominator.max(1) as f64
}

#[derive(Debug, Deserialize)]
struct MixedSuite {
    contract: String,
    documents: Vec<MixedDocument>,
    queries: Vec<MixedQuery>,
}

impl MixedSuite {
    fn validate(&self) -> Result<()> {
        if self.contract != MIXED_CONTRACT || self.documents.is_empty() || self.queries.is_empty() {
            bail!("invalid mixed qualification suite contract or empty cohort");
        }
        let documents = self
            .documents
            .iter()
            .map(|document| document.stable_id.as_str())
            .collect::<HashSet<_>>();
        if documents.len() != self.documents.len()
            || self.queries.iter().any(|query| {
                query
                    .expected
                    .as_deref()
                    .is_some_and(|expected| !documents.contains(expected))
            })
        {
            bail!("mixed suite has duplicate documents or unknown gold identities");
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct MixedDocument {
    stable_id: String,
    title: String,
    body: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct MixedQuery {
    stable_id: String,
    shape: MixedQueryShape,
    query: String,
    #[serde(default)]
    groups: Vec<Vec<MixedExpansion>>,
    expected: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct MixedExpansion {
    term: String,
    quality: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum MixedQueryShape {
    Ordinary,
    Phrase,
    Fuzzy,
    NoResult,
}

impl MixedQueryShape {
    fn label(self) -> &'static str {
        match self {
            Self::Ordinary => "ordinary",
            Self::Phrase => "phrase",
            Self::Fuzzy => "fuzzy",
            Self::NoResult => "no_result",
        }
    }
}

struct PreparedMixedQuery<'a> {
    query: &'a MixedQuery,
    expansions: Vec<Vec<Expansion<'a>>>,
}

impl<'a> PreparedMixedQuery<'a> {
    fn new(query: &'a MixedQuery) -> Self {
        let expansions = query
            .groups
            .iter()
            .map(|group| {
                group
                    .iter()
                    .map(|expansion| Expansion {
                        term: expansion.term.as_str(),
                        quality: expansion.quality,
                    })
                    .collect()
            })
            .collect();
        Self { query, expansions }
    }

    fn group_views(&self) -> Vec<QueryGroup<'_>> {
        self.expansions
            .iter()
            .map(|expansions| QueryGroup { expansions })
            .collect()
    }

    fn search(
        &self,
        groups: &[QueryGroup<'_>],
        index: &QpsIndex,
        scratch: &mut SearchScratch,
        hits: &mut Vec<SearchHit>,
    ) -> Result<SearchReceipt> {
        if self.expansions.is_empty() {
            index
                .search_into(&self.query.query, TOP_K, scratch, hits)
                .map_err(Into::into)
        } else {
            index
                .search_groups_into(groups, TOP_K, scratch, hits)
                .map_err(Into::into)
        }
    }

    fn search_evidence(
        &self,
        groups: &[QueryGroup<'_>],
        index: &QpsIndex,
        scratch: &mut SearchScratch,
        hits: &mut Vec<SearchHit>,
    ) -> Result<SearchReceipt> {
        if self.expansions.is_empty() {
            index
                .search_evidence_into(&self.query.query, TOP_K, scratch, hits)
                .map_err(Into::into)
        } else {
            index
                .search_groups_evidence_into(groups, TOP_K, scratch, hits)
                .map_err(Into::into)
        }
    }
}

#[cfg(test)]
#[path = "baseline_tests.rs"]
mod tests;
