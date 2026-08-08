use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use hashbrown::HashMap;
use phoenix_lexical_qps::{
    rank_evidence_schema_identity_v3, DocumentInput, FieldConfig, FrozenHoldoutV3,
    JudgmentReasonV3, JudgmentSourceV3, KeyedIdentity, LinearRankerV1, PairwiseJudgmentDraftV3,
    PairwiseJudgmentV3, QpsBuilder, QpsConfig, RankEvidenceV3, RelevanceLedgerV3, RelevanceTier,
    SearchHit, SearchScratch, SplitGroupProvenanceV3, WorkspaceIdentityKey, MAXIMUM_QUERY_GROUPS,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::artifact::*;
use super::partition::MiningPartitions;
use super::source::{
    self, near_duplicate_key, QueryKind, SourceDataset, SourceDocument, SourceQuery,
};

const CONTRACT: &str = "phoenix.qps.independent-data-publication/v1";
const GRADED_CONTRACT: &str = "phoenix.qps.graded-evaluation-suite/v3";
pub(super) const CANDIDATE_CAP: usize = 160;
pub(super) const TOP_K: usize = 10;
pub(super) const NEGATIVES_PER_POSITIVE: usize = 4;
const SCIFACT_RELEASE_UNIX: u64 = 1_600_300_800;
const NFCORPUS_RELEASE_UNIX: u64 = 1_435_708_800;
const FIELDS: [FieldConfig; 2] = [
    FieldConfig::new("title", 1.35, 0.3, 0.0),
    FieldConfig::new("body", 1.0, 0.75, 0.0),
];

pub(crate) struct BuildInputs {
    pub workspace_key: PathBuf,
    pub phase_3: PathBuf,
    pub locomo: PathBuf,
    pub scifact: PathBuf,
    pub nfcorpus: PathBuf,
}

pub(crate) struct BuildOutputs {
    pub ledger: PathBuf,
    pub graded: PathBuf,
    pub review: PathBuf,
    pub receipt: PathBuf,
}

pub(crate) fn generate(inputs: &BuildInputs, outputs: &BuildOutputs) -> Result<Publication> {
    ensure_outputs_absent(outputs)?;
    let key = read_workspace_key(&inputs.workspace_key)?;
    let phase_3: FrozenPhase3 = read_json(&inputs.phase_3, "Phase 3 receipt")?;
    if phase_3.contract != "phoenix.memory.qps-v3-constitutional-tiers/v1"
        || !phase_3.phase_3_verified
    {
        bail!("independent generation requires a verified Phase 3 receipt");
    }
    let v2_model_identity = decode_hex_32(&phase_3.v2_configuration_sha256)?;
    let mut ledger = RelevanceLedgerV3::default();
    append_constitutional_holdouts(&mut ledger, &phase_3, &key, v2_model_identity)?;

    let training_sets = [
        source::load_beir(
            &inputs.scifact,
            "scifact",
            "train",
            QueryKind::ScientificClaim,
            SCIFACT_RELEASE_UNIX,
        )?,
        source::load_locomo(&inputs.locomo, 0..8)?,
    ];
    let mut training_audits = Vec::with_capacity(training_sets.len());
    let mut review_items = Vec::new();
    let mut generation = 10_u64;
    for dataset in &training_sets {
        let prepared = PreparedDataset::new(dataset, &key)?;
        let mut partitions = MiningPartitions::new(dataset);
        training_audits.push(append_review_candidates(
            &mut ledger,
            dataset,
            &prepared,
            &mut partitions,
            &key,
            v2_model_identity,
            &mut generation,
            &mut review_items,
        )?);
        training_audits.push(super::fuzzy::append_fuzzy_review_candidates(
            &mut ledger,
            dataset,
            &prepared,
            &mut partitions,
            &key,
            v2_model_identity,
            &mut generation,
            &mut review_items,
        )?);
    }
    ledger.validate().map_err(anyhow::Error::msg)?;

    let graded_sets = [
        source::load_beir(
            &inputs.nfcorpus,
            "nfcorpus",
            "test",
            QueryKind::MedicalInformation,
            NFCORPUS_RELEASE_UNIX,
        )?,
        source::load_locomo(&inputs.locomo, 8..10)?,
    ];
    let (graded, graded_audits) = build_graded(&graded_sets, &key)?;
    if graded.queries.is_empty() {
        bail!("independent graded suite produced no eligible queries");
    }

    write_json_atomic(&outputs.ledger, &ledger)?;
    write_json_atomic(&outputs.graded, &graded)?;
    write_json_atomic(
        &outputs.review,
        &ReviewPacket {
            contract: "phoenix.qps.relevance-review-packet/v1",
            schema_version: 1,
            instructions: "Review each pair against the query and emit a separate decisions file; do not edit keyed identities or feature vectors.",
            negative_label_warning: "The positive is human-annotated gold; the same-tier V2 negative remains non-authoritative until explicitly reviewed.",
            items: review_items,
        },
    )?;
    let receipt = GenerationReceipt {
        contract: CONTRACT,
        schema_version: 1,
        producer_binary: file_identity(&std::env::current_exe()?)?,
        inputs: InputReceipt {
            phase_3: file_identity(&inputs.phase_3)?,
            locomo: file_identity(&inputs.locomo)?,
            scifact_corpus: file_identity(&inputs.scifact.join("corpus.jsonl"))?,
            scifact_queries: file_identity(&inputs.scifact.join("queries.jsonl"))?,
            scifact_qrels: file_identity(&inputs.scifact.join("qrels/train.tsv"))?,
            nfcorpus_corpus: file_identity(&inputs.nfcorpus.join("corpus.jsonl"))?,
            nfcorpus_queries: file_identity(&inputs.nfcorpus.join("queries.jsonl"))?,
            nfcorpus_qrels: file_identity(&inputs.nfcorpus.join("qrels/test.tsv"))?,
            workspace_key_recorded: false,
        },
        outputs: OutputReceipt {
            ledger: file_identity(&outputs.ledger)?,
            graded_suite: file_identity(&outputs.graded)?,
            review_packet: file_identity(&outputs.review)?,
        },
        policy: GenerationPolicy {
            training_sources: ["SciFact train", "LoCoMo conversations 0-7"],
            graded_sources: ["NFCorpus test", "LoCoMo conversations 8-9"],
            negative_policy: "four V2 hard negatives in the positive constitutional tier",
            judgment_source: JudgmentSourceV3::AutomaticallyMinedNegative,
            authoritative_promotion_evidence: false,
            real_user_corrections_inferred: false,
            release_feature_vectors_consumed_for_training: false,
            cross_source_near_duplicates_excluded: true,
            candidate_cap: CANDIDATE_CAP,
        },
        constitutional_holdouts: ledger
            .judgments
            .iter()
            .filter(|judgment| judgment.frozen_holdout.is_some())
            .count(),
        training: training_audits,
        graded: graded_audits,
        ledger_judgments: ledger.judgments.len(),
        graded_queries: graded.queries.len(),
    };
    write_json_atomic(&outputs.receipt, &receipt)?;
    Ok(Publication {
        contract: CONTRACT,
        ledger: file_identity(&outputs.ledger)?,
        graded_suite: file_identity(&outputs.graded)?,
        review_packet: file_identity(&outputs.review)?,
        receipt: file_identity(&outputs.receipt)?,
        ledger_judgments: receipt.ledger_judgments,
        graded_queries: receipt.graded_queries,
        authoritative_promotion_evidence: false,
    })
}

fn append_review_candidates(
    ledger: &mut RelevanceLedgerV3,
    dataset: &SourceDataset,
    prepared: &PreparedDataset<'_>,
    partitions: &mut MiningPartitions,
    key: &WorkspaceIdentityKey,
    v2_model_identity: [u8; 32],
    generation: &mut u64,
    review_items: &mut Vec<ReviewItem>,
) -> Result<DatasetAudit> {
    let mut scratch = SearchScratch::default();
    let mut hits = Vec::with_capacity(CANDIDATE_CAP);
    let mut audit = DatasetAudit::new(dataset);
    for query in &dataset.queries {
        hits.clear();
        prepared
            .index
            .search_evidence_into(&query.text, TOP_K, &mut scratch, &mut hits)?;
        let Some(positive_position) = best_positive(&hits, query, prepared) else {
            audit.oracle_missing_from_pool += 1;
            continue;
        };
        let positive = hits[positive_position];
        let negatives = hard_negatives(&hits, query, prepared, positive_position, partitions);
        if negatives.len() != NEGATIVES_PER_POSITIVE {
            audit.insufficient_same_tier_negatives += 1;
            continue;
        }
        let pool = candidate_pool(&hits, prepared);
        for negative_position in negatives {
            let negative = hits[negative_position];
            let reason = classify_reason(query, positive, negative);
            let judgment = PairwiseJudgmentV3::from_draft(PairwiseJudgmentDraftV3 {
                workspace_identity: KeyedIdentity::derive(
                    key,
                    b"workspace",
                    b"qps-v3-independent-data",
                ),
                query_identity: KeyedIdentity::derive(
                    key,
                    b"private-query",
                    format!("{}:{}", dataset.name, query.id).as_bytes(),
                ),
                positive_document_version: prepared.document_versions
                    [positive.external_id as usize - 1],
                negative_document_version: prepared.document_versions
                    [negative.external_id as usize - 1],
                positive_features: positive.rank_evidence_v3,
                negative_features: negative.rank_evidence_v3,
                positive_tier: positive.relevance_tier,
                negative_tier: negative.relevance_tier,
                candidate_pool: pool.clone().into_boxed_slice(),
                positive_position: positive_position as u16,
                negative_position: negative_position as u16,
                split_groups: split_groups(dataset, query, prepared, positive, negative, key),
                frozen_holdout: None,
                v2_model_identity,
                challenger_model_identity: rank_evidence_schema_identity_v3(),
                reason,
                source: JudgmentSourceV3::AutomaticallyMinedNegative,
                confidence: 0.5,
                weight: 0.5,
                index_generation: *generation,
                supersedes: None,
                contradicts: Box::new([]),
            });
            review_items.push(ReviewItem {
                judgment_identity: hex(judgment.identity.as_bytes()),
                dataset: dataset.name,
                query_id: query.id.clone(),
                query: query.text.clone(),
                positive: review_document(&prepared.documents[positive.external_id as usize - 1]),
                negative: review_document(&prepared.documents[negative.external_id as usize - 1]),
                positive_v2_position: positive_position,
                negative_v2_position: negative_position,
                suggested_reason: reason,
            });
            ledger.append(judgment).map_err(anyhow::Error::msg)?;
            *generation += 1;
            audit
                .reasons
                .entry(reason)
                .and_modify(|count| *count += 1)
                .or_insert(1);
            audit.judgments += 1;
        }
        audit.eligible_queries += 1;
    }
    Ok(audit)
}

fn build_graded(
    datasets: &[SourceDataset],
    key: &WorkspaceIdentityKey,
) -> Result<(GradedEvaluationSuite, Vec<DatasetAudit>)> {
    let mut queries = Vec::new();
    let mut audits = Vec::with_capacity(datasets.len());
    for dataset in datasets {
        let prepared = PreparedDataset::new(dataset, key)?;
        let mut scratch = SearchScratch::default();
        let mut hits = Vec::with_capacity(CANDIDATE_CAP);
        let mut audit = DatasetAudit::new(dataset);
        for query in &dataset.queries {
            hits.clear();
            prepared
                .index
                .search_evidence_into(&query.text, TOP_K, &mut scratch, &mut hits)?;
            if !hits.iter().any(|hit| {
                query
                    .relevant
                    .contains_key(&prepared.documents[hit.external_id as usize - 1].id)
            }) {
                audit.oracle_missing_from_pool += 1;
                continue;
            }
            let candidates = hits
                .iter()
                .enumerate()
                .map(|(v2_order, hit)| {
                    let document = &prepared.documents[hit.external_id as usize - 1];
                    GradedCandidate {
                        document_identity: hex(prepared.document_versions
                            [hit.external_id as usize - 1]
                            .as_bytes()),
                        v2_order,
                        rank_evidence_v3: hit.rank_evidence_v3,
                        relevance_tier: hit.relevance_tier,
                        grade: query
                            .relevant
                            .get(&document.id)
                            .copied()
                            .unwrap_or(0)
                            .min(4),
                    }
                })
                .collect();
            queries.push(GradedQuery {
                query_identity: hex(KeyedIdentity::derive(
                    key,
                    b"graded-query",
                    format!("{}:{}", dataset.name, query.id).as_bytes(),
                )
                .as_bytes()),
                candidates,
            });
            audit.eligible_queries += 1;
        }
        audits.push(audit);
    }
    Ok((
        GradedEvaluationSuite {
            contract: GRADED_CONTRACT,
            schema_version: 3,
            queries,
        },
        audits,
    ))
}

pub(super) struct PreparedDataset<'a> {
    pub(super) index: phoenix_lexical_qps::QpsIndex,
    pub(super) documents: &'a [SourceDocument],
    pub(super) document_versions: Vec<KeyedIdentity>,
    pub(super) source_identities: Vec<KeyedIdentity>,
    pub(super) near_duplicate_identities: Vec<KeyedIdentity>,
}

impl<'a> PreparedDataset<'a> {
    pub(super) fn new(dataset: &'a SourceDataset, key: &WorkspaceIdentityKey) -> Result<Self> {
        let mut builder = QpsBuilder::new(FIELDS.to_vec().into_boxed_slice(), v2_config())?;
        let mut document_versions = Vec::with_capacity(dataset.documents.len());
        let mut source_identities = Vec::with_capacity(dataset.documents.len());
        let mut near_duplicate_identities = Vec::with_capacity(dataset.documents.len());
        for (index, document) in dataset.documents.iter().enumerate() {
            let values = [document.title.as_str(), document.text.as_str()];
            builder.insert(DocumentInput {
                external_id: index as u64 + 1,
                fields: &values,
            })?;
            let version = format!(
                "{}:{}:{}",
                dataset.name,
                document.id,
                sha256_bytes(format!("{}\0{}", document.title, document.text).as_bytes())
            );
            document_versions.push(KeyedIdentity::derive(
                key,
                b"document-version",
                version.as_bytes(),
            ));
            source_identities.push(KeyedIdentity::derive(
                key,
                b"source-document",
                document.source_family.as_bytes(),
            ));
            near_duplicate_identities.push(KeyedIdentity::derive(
                key,
                b"near-duplicate-content",
                near_duplicate_key(document).as_bytes(),
            ));
        }
        Ok(Self {
            index: builder.build()?,
            documents: &dataset.documents,
            document_versions,
            source_identities,
            near_duplicate_identities,
        })
    }
}

pub(super) fn best_positive(
    hits: &[SearchHit],
    query: &SourceQuery,
    prepared: &PreparedDataset<'_>,
) -> Option<usize> {
    hits.iter().position(|hit| {
        query
            .relevant
            .contains_key(&prepared.documents[hit.external_id as usize - 1].id)
    })
}

pub(super) fn hard_negatives(
    hits: &[SearchHit],
    query: &SourceQuery,
    prepared: &PreparedDataset<'_>,
    positive_position: usize,
    partitions: &mut MiningPartitions,
) -> Vec<usize> {
    let Some(component) = partitions.component(&query.id) else {
        return Vec::new();
    };
    let tier = hits[positive_position].relevance_tier;
    let mut above = (0..positive_position)
        .filter(|position| {
            let document = hits[*position].external_id as usize - 1;
            is_negative(hits[*position], query, prepared)
                && hits[*position].relevance_tier == tier
                && partitions.allows(document, component)
        })
        .collect::<Vec<_>>();
    let below = (positive_position + 1..hits.len()).filter(|position| {
        let document = hits[*position].external_id as usize - 1;
        is_negative(hits[*position], query, prepared)
            && hits[*position].relevance_tier == tier
            && partitions.allows(document, component)
    });
    above.extend(below);
    above.truncate(NEGATIVES_PER_POSITIVE);
    if above.len() == NEGATIVES_PER_POSITIVE {
        for &position in &above {
            partitions.claim(hits[position].external_id as usize - 1, component);
        }
    }
    above
}

fn is_negative(hit: SearchHit, query: &SourceQuery, prepared: &PreparedDataset<'_>) -> bool {
    !query
        .relevant
        .contains_key(&prepared.documents[hit.external_id as usize - 1].id)
}

pub(super) fn candidate_pool(
    hits: &[SearchHit],
    prepared: &PreparedDataset<'_>,
) -> Vec<KeyedIdentity> {
    hits.iter()
        .map(|hit| prepared.document_versions[hit.external_id as usize - 1])
        .collect()
}

pub(super) fn split_groups(
    dataset: &SourceDataset,
    query: &SourceQuery,
    prepared: &PreparedDataset<'_>,
    positive: SearchHit,
    negative: SearchHit,
    key: &WorkspaceIdentityKey,
) -> SplitGroupProvenanceV3 {
    let positive_index = positive.external_id as usize - 1;
    let negative_index = negative.external_id as usize - 1;
    SplitGroupProvenanceV3 {
        query_family_identity: KeyedIdentity::derive(key, b"query-family", query.family.as_bytes()),
        positive_source_identity: prepared.source_identities[positive_index],
        negative_source_identity: prepared.source_identities[negative_index],
        positive_near_duplicate_cluster_identity: prepared.near_duplicate_identities
            [positive_index],
        negative_near_duplicate_cluster_identity: prepared.near_duplicate_identities
            [negative_index],
        entity_or_identifier_family_identity: KeyedIdentity::derive(
            key,
            b"entity-family",
            query.entity_family.as_bytes(),
        ),
        collection_cohort_identity: KeyedIdentity::derive(
            key,
            b"collection-cohort",
            query.collection_cohort.as_bytes(),
        ),
        collected_at_unix_seconds: query
            .collected_at
            .max(dataset.documents[positive_index].collected_at)
            .max(dataset.documents[negative_index].collected_at),
    }
}

fn classify_reason(
    query: &SourceQuery,
    positive: SearchHit,
    negative: SearchHit,
) -> JudgmentReasonV3 {
    let positive = positive.rank_evidence_v3;
    let negative = negative.rank_evidence_v3;
    if positive.query_groups > 16 {
        return JudgmentReasonV3::LongQueryFailure;
    }
    if query
        .text
        .chars()
        .any(|character| character.is_ascii_digit())
        && delta(positive, negative, RankEvidenceV3::EXACT_GROUP_FRACTION) > 0.0
    {
        return JudgmentReasonV3::IdentifierCollision;
    }
    if let QueryKind::Conversation { category } = query.kind {
        return match category {
            1 => JudgmentReasonV3::WrongConceptProximity,
            2 => JudgmentReasonV3::PhraseOrderFailure,
            3 => JudgmentReasonV3::ScatteredTerms,
            4 => JudgmentReasonV3::DocumentConversationConfusion,
            _ => JudgmentReasonV3::CommonTermDominance,
        };
    }
    let candidates = [
        (
            JudgmentReasonV3::PartialMatchSaturation,
            (negative.values[RankEvidenceV3::BM25F_LEXICAL]
                - positive.values[RankEvidenceV3::BM25F_LEXICAL])
                .max(0.0)
                + delta(positive, negative, RankEvidenceV3::WEIGHTED_GROUP_COVERAGE),
        ),
        (
            JudgmentReasonV3::ScatteredTerms,
            delta(positive, negative, RankEvidenceV3::COMPLETE_SPAN_QUALITY)
                + delta(positive, negative, RankEvidenceV3::ORDERED_SPAN_QUALITY),
        ),
        (
            JudgmentReasonV3::PhraseOrderFailure,
            delta(positive, negative, RankEvidenceV3::EXACT_PHRASE)
                + delta(positive, negative, RankEvidenceV3::ORDERED_FRACTION),
        ),
        (
            JudgmentReasonV3::FuzzyCollision,
            delta(positive, negative, RankEvidenceV3::MEAN_EXPANSION_QUALITY),
        ),
        (
            JudgmentReasonV3::WeakFieldEvidence,
            delta(positive, negative, RankEvidenceV3::FIELD_COVERAGE_FRACTION)
                + delta(positive, negative, RankEvidenceV3::FIELD_LEXICAL_1),
        ),
        (
            JudgmentReasonV3::CommonTermDominance,
            delta(positive, negative, RankEvidenceV3::MEAN_MATCHED_TERM_RARITY),
        ),
        (
            JudgmentReasonV3::LengthPriorFailure,
            delta(positive, negative, RankEvidenceV3::DOCUMENT_LENGTH_PRIOR),
        ),
        (
            JudgmentReasonV3::WrongConceptProximity,
            delta(positive, negative, RankEvidenceV3::EXACT_GROUP_FRACTION)
                + delta(positive, negative, RankEvidenceV3::MATCHED_GROUP_FRACTION),
        ),
    ];
    candidates
        .into_iter()
        .max_by(|left, right| left.1.total_cmp(&right.1))
        .map(|candidate| candidate.0)
        .unwrap_or(JudgmentReasonV3::WrongConceptProximity)
}

fn delta(positive: RankEvidenceV3, negative: RankEvidenceV3, feature: usize) -> f32 {
    (positive.values[feature] - negative.values[feature]).max(0.0)
}

fn append_constitutional_holdouts(
    ledger: &mut RelevanceLedgerV3,
    phase_3: &FrozenPhase3,
    key: &WorkspaceIdentityKey,
    v2_model_identity: [u8; 32],
) -> Result<()> {
    for (index, query) in phase_3
        .mixed_suite
        .queries
        .iter()
        .filter(|query| {
            query.candidate_pool.len() >= 2
                && query.oracle_locations.first().is_some_and(|oracle| {
                    query
                        .candidate_pool
                        .iter()
                        .any(|candidate| candidate.document_identity == oracle.document_identity)
                })
        })
        .take(9)
        .enumerate()
    {
        let oracle = query
            .oracle_locations
            .first()
            .context("constitutional query lacks oracle")?;
        let positive_position = query
            .candidate_pool
            .iter()
            .position(|candidate| candidate.document_identity == oracle.document_identity)
            .context("constitutional oracle missing from candidate pool")?;
        let negative_position = (0..query.candidate_pool.len())
            .find(|position| *position != positive_position)
            .context("constitutional query lacks negative")?;
        let pool = query
            .candidate_pool
            .iter()
            .map(|candidate| {
                KeyedIdentity::derive(
                    key,
                    b"document-version",
                    candidate.document_version_sha256.as_bytes(),
                )
            })
            .collect::<Vec<_>>();
        let positive = &query.candidate_pool[positive_position];
        let negative = &query.candidate_pool[negative_position];
        let fixture = format!("constitutional:{}", query.query_identity);
        ledger
            .append(PairwiseJudgmentV3::from_draft(PairwiseJudgmentDraftV3 {
                workspace_identity: KeyedIdentity::derive(
                    key,
                    b"workspace",
                    b"qps-v3-independent-data",
                ),
                query_identity: KeyedIdentity::derive(
                    key,
                    b"private-query",
                    query.query_identity.as_bytes(),
                ),
                positive_document_version: pool[positive_position],
                negative_document_version: pool[negative_position],
                positive_features: positive.rank_evidence_v3,
                negative_features: negative.rank_evidence_v3,
                positive_tier: positive.relevance_tier,
                negative_tier: negative.relevance_tier,
                candidate_pool: pool.into_boxed_slice(),
                positive_position: positive_position as u16,
                negative_position: negative_position as u16,
                split_groups: SplitGroupProvenanceV3 {
                    query_family_identity: KeyedIdentity::derive(
                        key,
                        b"query-family",
                        fixture.as_bytes(),
                    ),
                    positive_source_identity: KeyedIdentity::derive(
                        key,
                        b"source-document",
                        positive.document_identity.as_bytes(),
                    ),
                    negative_source_identity: KeyedIdentity::derive(
                        key,
                        b"source-document",
                        negative.document_identity.as_bytes(),
                    ),
                    positive_near_duplicate_cluster_identity: KeyedIdentity::derive(
                        key,
                        b"near-duplicate",
                        positive.document_version_sha256.as_bytes(),
                    ),
                    negative_near_duplicate_cluster_identity: KeyedIdentity::derive(
                        key,
                        b"near-duplicate",
                        negative.document_version_sha256.as_bytes(),
                    ),
                    entity_or_identifier_family_identity: KeyedIdentity::derive(
                        key,
                        b"entity-family",
                        fixture.as_bytes(),
                    ),
                    collection_cohort_identity: KeyedIdentity::derive(
                        key,
                        b"collection-cohort",
                        fixture.as_bytes(),
                    ),
                    collected_at_unix_seconds: 1_700_000_000 + index as u64,
                },
                frozen_holdout: Some(FrozenHoldoutV3::ConstitutionalRegression),
                v2_model_identity,
                challenger_model_identity: rank_evidence_schema_identity_v3(),
                reason: JudgmentReasonV3::WrongConceptProximity,
                source: JudgmentSourceV3::CuratedRegressionCase,
                confidence: 1.0,
                weight: 1.5,
                index_generation: index as u64 + 1,
                supersedes: None,
                contradicts: Box::new([]),
            }))
            .map_err(anyhow::Error::msg)?;
    }
    Ok(())
}

fn v2_config() -> QpsConfig {
    QpsConfig {
        maximum_candidate_pool: CANDIDATE_CAP,
        maximum_query_groups: MAXIMUM_QUERY_GROUPS,
        learned_ranker: LinearRankerV1::disabled(),
        ..QpsConfig::default()
    }
}

fn ensure_outputs_absent(outputs: &BuildOutputs) -> Result<()> {
    for output in [
        &outputs.ledger,
        &outputs.graded,
        &outputs.review,
        &outputs.receipt,
    ] {
        if output.exists() {
            bail!("refusing to overwrite {}", output.display());
        }
    }
    Ok(())
}

fn read_workspace_key(path: &Path) -> Result<WorkspaceIdentityKey> {
    let bytes: [u8; 32] = fs::read(path)
        .with_context(|| format!("read workspace key {}", path.display()))?
        .try_into()
        .map_err(|_| anyhow::anyhow!("workspace key must contain exactly 32 raw bytes"))?;
    WorkspaceIdentityKey::new(bytes).map_err(anyhow::Error::msg)
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path, label: &str) -> Result<T> {
    serde_json::from_reader(
        File::open(path).with_context(|| format!("open {label} {}", path.display()))?,
    )
    .with_context(|| format!("decode {label} {}", path.display()))
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let parent = path.parent().context("output path has no parent")?;
    fs::create_dir_all(parent)?;
    let file_name = path.file_name().context("output path has no file name")?;
    let temporary = parent.join(format!(".{}.tmp", file_name.to_string_lossy()));
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer(&mut writer, value)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    writer.get_ref().sync_all()?;
    drop(writer);
    fs::rename(&temporary, path)?;
    Ok(())
}

fn decode_hex_32(value: &str) -> Result<[u8; 32]> {
    if value.len() != 64 {
        bail!("identity must contain 64 hexadecimal characters");
    }
    let mut bytes = [0_u8; 32];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .context("identity contains invalid hexadecimal")?;
    }
    Ok(bytes)
}

fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn hex<const N: usize>(bytes: [u8; N]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn file_identity(path: &Path) -> Result<FileIdentity> {
    let bytes = fs::read(path).with_context(|| format!("read artifact {}", path.display()))?;
    Ok(FileIdentity {
        path: path.to_path_buf(),
        bytes: bytes.len() as u64,
        sha256: sha256_bytes(&bytes),
    })
}

#[derive(Debug, Serialize)]
pub(super) struct DatasetAudit {
    pub(super) dataset: &'static str,
    pub(super) documents: usize,
    pub(super) queries: usize,
    pub(super) eligible_queries: usize,
    pub(super) judgments: usize,
    pub(super) oracle_missing_from_pool: usize,
    pub(super) insufficient_same_tier_negatives: usize,
    pub(super) reasons: HashMap<JudgmentReasonV3, usize>,
}

impl DatasetAudit {
    pub(super) fn new(dataset: &SourceDataset) -> Self {
        Self {
            dataset: dataset.name,
            documents: dataset.documents.len(),
            queries: dataset.queries.len(),
            eligible_queries: 0,
            judgments: 0,
            oracle_missing_from_pool: 0,
            insufficient_same_tier_negatives: 0,
            reasons: HashMap::new(),
        }
    }
}

pub(super) fn review_document(document: &SourceDocument) -> ReviewDocument {
    ReviewDocument {
        id: document.id.clone(),
        title: document.title.clone(),
        text: document.text.clone(),
    }
}

#[derive(Debug, Deserialize)]
struct FrozenPhase3 {
    contract: String,
    v2_configuration_sha256: String,
    mixed_suite: FrozenCohort,
    phase_3_verified: bool,
}

#[derive(Debug, Deserialize)]
struct FrozenCohort {
    queries: Vec<FrozenQuery>,
}

#[derive(Debug, Deserialize)]
struct FrozenQuery {
    query_identity: String,
    candidate_pool: Vec<FrozenCandidate>,
    oracle_locations: Vec<FrozenOracle>,
}

#[derive(Debug, Deserialize)]
struct FrozenCandidate {
    document_identity: String,
    document_version_sha256: String,
    rank_evidence_v3: RankEvidenceV3,
    relevance_tier: RelevanceTier,
}

#[derive(Debug, Deserialize)]
struct FrozenOracle {
    document_identity: String,
}

#[cfg(test)]
#[path = "build_tests.rs"]
mod tests;
