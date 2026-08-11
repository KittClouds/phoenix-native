use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use hashbrown::HashSet;
use phoenix_lexical_qps::JudgmentReasonV3;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

const PACKET_CONTRACT_V1: &str = "phoenix.qps.relevance-review-packet/v1";
const PACKET_CONTRACT_V2: &str = "phoenix.qps.relevance-review-packet/v2";
const PAIR_BATCH_CONTRACT: &str = "phoenix.qps.semantic-review-batch/v2";
const BUNDLE_BATCH_CONTRACT: &str = "phoenix.qps.semantic-review-bundle-batch/v1";
const BUNDLE_AUDIT_CONTRACT: &str = "phoenix.qps.semantic-review-bundle-audit/v1";
const MAX_BUNDLES: usize = 2_000;
const MIN_CHALLENGERS: usize = 3;
const MAX_CHALLENGERS: usize = 5;
const CLASS_TARGET: usize = 100;

const REASONS: [JudgmentReasonV3; 11] = [
    JudgmentReasonV3::PartialMatchSaturation,
    JudgmentReasonV3::ScatteredTerms,
    JudgmentReasonV3::PhraseOrderFailure,
    JudgmentReasonV3::IdentifierCollision,
    JudgmentReasonV3::FuzzyCollision,
    JudgmentReasonV3::WeakFieldEvidence,
    JudgmentReasonV3::CommonTermDominance,
    JudgmentReasonV3::LengthPriorFailure,
    JudgmentReasonV3::WrongConceptProximity,
    JudgmentReasonV3::DocumentConversationConfusion,
    JudgmentReasonV3::LongQueryFailure,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Selection {
    DeficitFirst,
    Confirmation,
}

impl Selection {
    fn description(self) -> &'static str {
        match self {
            Self::DeficitFirst => {
                "deterministic deficit-first class coverage, then V2 disagreement impact, LoCoMo transfer value, stable identity"
            }
            Self::Confirmation => {
                "deterministic deficit-first class coverage over complete V2-correct challenger bundles, then minimum V2 separation and stable identity"
            }
        }
    }
}

pub(crate) fn prepare(
    packet_path: &Path,
    locomo_path: &Path,
    exclude_batch_path: Option<&Path>,
    phase_5_path: &Path,
    limit: usize,
    output_path: &Path,
) -> Result<Publication> {
    prepare_with_selection(
        packet_path,
        locomo_path,
        exclude_batch_path,
        phase_5_path,
        limit,
        output_path,
        Selection::DeficitFirst,
    )
}

pub(crate) fn prepare_confirmation(
    packet_path: &Path,
    locomo_path: &Path,
    exclude_batch_path: &Path,
    phase_5_path: &Path,
    limit: usize,
    output_path: &Path,
) -> Result<Publication> {
    prepare_with_selection(
        packet_path,
        locomo_path,
        Some(exclude_batch_path),
        phase_5_path,
        limit,
        output_path,
        Selection::Confirmation,
    )
}

#[allow(clippy::too_many_arguments)]
fn prepare_with_selection(
    packet_path: &Path,
    locomo_path: &Path,
    exclude_batch_path: Option<&Path>,
    phase_5_path: &Path,
    limit: usize,
    output_path: &Path,
    selection: Selection,
) -> Result<Publication> {
    if limit == 0 || limit > MAX_BUNDLES {
        bail!("bundle limit must be between 1 and {MAX_BUNDLES}");
    }
    refuse_overwrite(output_path)?;
    let packet: ReviewPacket = read_json(packet_path, "review packet")?;
    let valid_packet = (packet.contract == PACKET_CONTRACT_V1 && packet.schema_version == 1)
        || (packet.contract == PACKET_CONTRACT_V2 && packet.schema_version == 2);
    if !valid_packet || packet.items.is_empty() {
        bail!("invalid or empty review packet");
    }
    let phase_5: Phase5Receipt = read_json(phase_5_path, "Phase 5 receipt")?;
    if phase_5.technical_failure_classes.is_empty() {
        bail!("Phase 5 receipt has no technical failure-class accounting");
    }
    let initial_counts = current_class_counts(&phase_5)?;

    let mut excluded = HashSet::new();
    let mut visited = HashSet::new();
    let excluded_batch = if let Some(path) = exclude_batch_path {
        collect_exclusion_chain(path, &mut visited, &mut excluded)?;
        Some(file_identity(path)?)
    } else {
        None
    };
    let source_items = packet.items.len();
    let mut remaining = packet
        .items
        .into_iter()
        .filter(|item| !excluded.contains(&item.judgment_identity))
        .collect::<Vec<_>>();
    let excluded_items = source_items - remaining.len();
    if selection == Selection::Confirmation {
        remaining.retain(pair_is_v2_confirmation);
    }
    let mut candidates = build_bundles(remaining)?;
    if selection == Selection::Confirmation {
        candidates.sort_by(confirmation_bundle_priority);
    }
    let mut selected = select_bundles(candidates, initial_counts, limit);
    enrich_locomo_context(&mut selected, locomo_path)?;
    validate_bundles(&selected)?;

    let selected_pairs = selected.iter().map(|bundle| bundle.challengers.len()).sum();
    let selected_counts = reason_counts(&selected);
    let projected_counts = REASONS
        .into_iter()
        .enumerate()
        .map(|(index, reason)| ClassProjection {
            reason,
            initial: initial_counts[index],
            selected: selected_counts[index],
            projected: initial_counts[index] + selected_counts[index],
        })
        .collect();
    let v2_disagreements = selected
        .iter()
        .flat_map(|bundle| &bundle.challengers)
        .filter(|challenger| challenger.positive_v2_position > challenger.negative_v2_position)
        .count();
    let batch = BundleBatch {
        contract: BUNDLE_BATCH_CONTRACT.to_owned(),
        schema_version: 1,
        source_packet: file_identity(packet_path)?,
        phase_5: file_identity(phase_5_path)?,
        excluded_batch,
        excluded_items,
        policy: BundlePolicy {
            authoritative_evidence_created: false,
            requires_pairwise_semantic_review: true,
            bundle_semantics: "one annotated positive and three to five same-query, same-tier hard-negative challengers".to_owned(),
            selection: selection.description().to_owned(),
            decision_output: "each challenger remains an independent positive_preferred, negative_preferred, or abstain decision".to_owned(),
            attestation: "agent decisions require agent_curated_with_user_authorization and explicit authorization context".to_owned(),
        },
        requested_bundles: limit,
        selected_bundles: selected.len(),
        selected_pairs,
        v2_disagreements,
        bundles_with_three_to_five_challengers: selected
            .iter()
            .filter(|bundle| {
                (MIN_CHALLENGERS..=MAX_CHALLENGERS).contains(&bundle.challengers.len())
            })
            .count(),
        class_projection: projected_counts,
        bundles: selected,
    };
    write_json_atomic(output_path, &batch)?;
    Ok(Publication {
        contract: BUNDLE_BATCH_CONTRACT,
        output: file_identity(output_path)?,
        selected_bundles: batch.selected_bundles,
        selected_pairs,
        v2_disagreements,
        authoritative_evidence_created: false,
    })
}

pub(crate) fn audit(batch_path: &Path, output_path: &Path) -> Result<AuditPublication> {
    refuse_overwrite(output_path)?;
    let batch: BundleBatch = read_json(batch_path, "bundle batch")?;
    if batch.contract != BUNDLE_BATCH_CONTRACT || batch.schema_version != 1 {
        bail!("invalid bundle batch contract");
    }
    validate_bundles(&batch.bundles)?;
    let identities = batch
        .bundles
        .iter()
        .flat_map(|bundle| &bundle.challengers)
        .map(|challenger| challenger.judgment_identity.as_str())
        .collect::<HashSet<_>>();
    let pairs = batch
        .bundles
        .iter()
        .map(|bundle| bundle.challengers.len())
        .sum::<usize>();
    if batch.selected_bundles != batch.bundles.len()
        || batch.selected_pairs != pairs
        || identities.len() != pairs
        || batch.bundles_with_three_to_five_challengers != batch.bundles.len()
    {
        bail!("bundle batch accounting is inconsistent");
    }
    let receipt = BundleAudit {
        contract: BUNDLE_AUDIT_CONTRACT,
        schema_version: 1,
        batch: file_identity(batch_path)?,
        bundles: batch.bundles.len(),
        pairs,
        unique_queries: batch
            .bundles
            .iter()
            .map(|bundle| (bundle.dataset.as_str(), bundle.query_id.as_str()))
            .collect::<HashSet<_>>()
            .len(),
        unique_judgment_identities: identities.len(),
        bundles_with_three_to_five_challengers: batch.bundles.len(),
        all_classes_projected_to_target: batch
            .class_projection
            .iter()
            .all(|projection| projection.projected >= CLASS_TARGET),
        semantic_decisions_emitted: false,
    };
    write_json_atomic(output_path, &receipt)?;
    Ok(AuditPublication {
        contract: BUNDLE_AUDIT_CONTRACT,
        output: file_identity(output_path)?,
        bundles: receipt.bundles,
        pairs,
        all_classes_projected_to_target: receipt.all_classes_projected_to_target,
    })
}

fn build_bundles(items: Vec<ReviewItem>) -> Result<Vec<QueryBundle>> {
    let mut grouped = BTreeMap::<GroupKey, Vec<ReviewItem>>::new();
    for item in items {
        let key = GroupKey {
            dataset: item.dataset.clone(),
            query_id: item.query_id.clone(),
            positive_id: item.positive.id.clone(),
        };
        grouped.entry(key).or_default().push(item);
    }
    let mut bundles = Vec::with_capacity(grouped.len());
    for (_, mut items) in grouped {
        items.sort_by(pair_priority);
        let first = items.first().context("empty query group")?;
        if items.iter().any(|item| {
            item.query != first.query
                || item.reference_answer != first.reference_answer
                || item.positive != first.positive
                || item.positive_v2_position != first.positive_v2_position
        }) {
            bail!("query group changes its query, positive document, or V2 position");
        }
        let mut judgment_ids = HashSet::with_capacity(items.len());
        let mut negative_ids = HashSet::with_capacity(items.len());
        items.retain(|item| {
            judgment_ids.insert(item.judgment_identity.clone())
                && negative_ids.insert(item.negative.id.clone())
        });
        if items.len() < MIN_CHALLENGERS {
            continue;
        }
        items.truncate(MAX_CHALLENGERS);
        let bundle_identity = bundle_identity(&items);
        let first = items.first().context("deduplicated query group is empty")?;
        bundles.push(QueryBundle {
            bundle_identity,
            dataset: first.dataset.clone(),
            query_id: first.query_id.clone(),
            query: first.query.clone(),
            reference_answer: first.reference_answer.clone(),
            positive: first.positive.clone(),
            challengers: items
                .into_iter()
                .map(|item| BundleChallenger {
                    judgment_identity: item.judgment_identity,
                    negative: item.negative,
                    positive_v2_position: item.positive_v2_position,
                    negative_v2_position: item.negative_v2_position,
                    suggested_reason: item.suggested_reason,
                })
                .collect(),
        });
    }
    bundles.sort_by(bundle_priority);
    Ok(bundles)
}

fn select_bundles(
    candidates: Vec<QueryBundle>,
    initial_counts: [usize; REASONS.len()],
    limit: usize,
) -> Vec<QueryBundle> {
    let mut selected = Vec::with_capacity(limit.min(candidates.len()));
    let mut used = vec![false; candidates.len()];
    let mut projected = initial_counts;
    while selected.len() < limit {
        let target = (0..REASONS.len())
            .filter(|index| projected[*index] < CLASS_TARGET)
            .filter(|index| {
                candidates.iter().enumerate().any(|(candidate, bundle)| {
                    !used[candidate] && bundle_contains_reason(bundle, REASONS[*index])
                })
            })
            .min_by_key(|index| (projected[*index], *index));
        let candidate = candidates.iter().enumerate().position(|(index, bundle)| {
            !used[index]
                && target.is_none_or(|reason| bundle_contains_reason(bundle, REASONS[reason]))
        });
        let Some(candidate) = candidate else {
            break;
        };
        used[candidate] = true;
        for challenger in &candidates[candidate].challengers {
            projected[reason_index(challenger.suggested_reason)] += 1;
        }
        selected.push(candidates[candidate].clone());
    }
    selected
}

fn validate_bundles(bundles: &[QueryBundle]) -> Result<()> {
    if bundles.is_empty() {
        bail!("bundle batch is empty");
    }
    let mut bundle_ids = HashSet::with_capacity(bundles.len());
    let mut query_ids = HashSet::with_capacity(bundles.len());
    let mut judgment_ids = HashSet::with_capacity(bundles.len() * MAX_CHALLENGERS);
    for bundle in bundles {
        if bundle.bundle_identity.len() != 64
            || !bundle_ids.insert(bundle.bundle_identity.as_str())
            || bundle.dataset.trim().is_empty()
            || bundle.query_id.trim().is_empty()
            || bundle.query.trim().is_empty()
            || bundle.positive.id.trim().is_empty()
            || !query_ids.insert((bundle.dataset.as_str(), bundle.query_id.as_str()))
            || !(MIN_CHALLENGERS..=MAX_CHALLENGERS).contains(&bundle.challengers.len())
        {
            bail!("invalid or duplicate query bundle");
        }
        let mut negative_ids = HashSet::with_capacity(bundle.challengers.len());
        for challenger in &bundle.challengers {
            if challenger.judgment_identity.len() != 64
                || !judgment_ids.insert(challenger.judgment_identity.as_str())
                || challenger.negative.id.trim().is_empty()
                || !negative_ids.insert(challenger.negative.id.as_str())
            {
                bail!("invalid or duplicate bundle challenger");
            }
        }
    }
    Ok(())
}

fn enrich_locomo_context(bundles: &mut [QueryBundle], path: &Path) -> Result<()> {
    let dataset = super::source::load_locomo(path, 0..10)?;
    let answers = dataset
        .queries
        .iter()
        .map(|query| (query.id.as_str(), query.reference_answer.as_str()))
        .collect::<hashbrown::HashMap<_, _>>();
    let context = dataset
        .documents
        .iter()
        .map(|document| {
            (
                document.id.as_str(),
                (
                    document.collected_at,
                    document.source_time_label.as_str(),
                    document.reviewer_context.as_str(),
                ),
            )
        })
        .collect::<hashbrown::HashMap<_, _>>();
    for bundle in bundles
        .iter_mut()
        .filter(|bundle| bundle.dataset == "locomo")
    {
        enrich_document(&mut bundle.positive, &context)?;
        for challenger in &mut bundle.challengers {
            enrich_document(&mut challenger.negative, &context)?;
        }
        let query_id = bundle
            .query_id
            .strip_prefix("fuzzy:")
            .unwrap_or(&bundle.query_id);
        bundle.reference_answer = answers
            .get(query_id)
            .with_context(|| format!("missing reference answer for {}", bundle.query_id))?
            .to_string();
    }
    Ok(())
}

fn enrich_document(
    document: &mut ReviewDocument,
    context: &hashbrown::HashMap<&str, (u64, &str, &str)>,
) -> Result<()> {
    let (collected_at, label, reviewer_context) = context
        .get(document.id.as_str())
        .with_context(|| format!("missing LoCoMo context for {}", document.id))?;
    document.collected_at_unix_seconds = *collected_at;
    document.source_time_label = (*label).to_owned();
    document.reviewer_context = (*reviewer_context).to_owned();
    Ok(())
}

fn collect_exclusion_chain(
    path: &Path,
    visited: &mut HashSet<PathBuf>,
    identities: &mut HashSet<String>,
) -> Result<()> {
    let canonical = fs::canonicalize(path)
        .with_context(|| format!("resolve excluded batch {}", path.display()))?;
    if !visited.insert(canonical.clone()) {
        bail!("excluded review-batch chain contains a cycle");
    }
    let value: Value = read_json(&canonical, "excluded review batch")?;
    let contract = value
        .get("contract")
        .and_then(Value::as_str)
        .context("excluded review batch has no contract")?;
    let (parent, excluded) = match contract {
        PAIR_BATCH_CONTRACT => {
            let batch: ExclusionBatch = serde_json::from_value(value)?;
            if batch.items.is_empty() {
                bail!("invalid or empty excluded pair batch");
            }
            (
                batch.excluded_batch,
                batch
                    .items
                    .into_iter()
                    .map(|item| item.judgment_identity)
                    .collect::<Vec<_>>(),
            )
        }
        BUNDLE_BATCH_CONTRACT => {
            let batch: BundleExclusionBatch = serde_json::from_value(value)?;
            if batch.bundles.is_empty() {
                bail!("invalid or empty excluded bundle batch");
            }
            (
                batch.excluded_batch,
                batch
                    .bundles
                    .into_iter()
                    .flat_map(|bundle| bundle.challengers)
                    .map(|item| item.judgment_identity)
                    .collect::<Vec<_>>(),
            )
        }
        _ => bail!("unsupported excluded review batch contract {contract}"),
    };
    identities.extend(excluded);
    if let Some(parent) = parent {
        verify_file_identity(&parent)?;
        collect_exclusion_chain(&parent.path, visited, identities)?;
    }
    Ok(())
}

fn verify_file_identity(expected: &ExpectedFileIdentity) -> Result<()> {
    let actual = file_identity(&expected.path)?;
    if actual.bytes != expected.bytes || actual.sha256 != expected.sha256 {
        bail!(
            "excluded review-batch identity changed: {}",
            expected.path.display()
        );
    }
    Ok(())
}

fn current_class_counts(phase_5: &Phase5Receipt) -> Result<[usize; REASONS.len()]> {
    let mut counts = [0_usize; REASONS.len()];
    let mut seen = [false; REASONS.len()];
    for class in &phase_5.technical_failure_classes {
        let index = reason_index(class.reason);
        if seen[index] {
            bail!("duplicate Phase 5 technical failure class");
        }
        seen[index] = true;
        counts[index] = class.authoritative_reviewed;
    }
    if seen.into_iter().any(|value| !value) {
        bail!("Phase 5 receipt omits a technical failure class");
    }
    Ok(counts)
}

fn reason_counts(bundles: &[QueryBundle]) -> [usize; REASONS.len()] {
    let mut counts = [0_usize; REASONS.len()];
    for challenger in bundles.iter().flat_map(|bundle| &bundle.challengers) {
        counts[reason_index(challenger.suggested_reason)] += 1;
    }
    counts
}

fn reason_index(reason: JudgmentReasonV3) -> usize {
    REASONS
        .iter()
        .position(|candidate| *candidate == reason)
        .expect("all technical reasons are enumerated")
}

fn bundle_contains_reason(bundle: &QueryBundle, reason: JudgmentReasonV3) -> bool {
    bundle
        .challengers
        .iter()
        .any(|challenger| challenger.suggested_reason == reason)
}

fn pair_priority(left: &ReviewItem, right: &ReviewItem) -> Ordering {
    pair_is_v2_disagreement(right)
        .cmp(&pair_is_v2_disagreement(left))
        .then_with(|| pair_displacement(right).cmp(&pair_displacement(left)))
        .then_with(|| left.judgment_identity.cmp(&right.judgment_identity))
}

fn bundle_priority(left: &QueryBundle, right: &QueryBundle) -> Ordering {
    bundle_v2_disagreements(right)
        .cmp(&bundle_v2_disagreements(left))
        .then_with(|| bundle_max_displacement(right).cmp(&bundle_max_displacement(left)))
        .then_with(|| u8::from(left.dataset != "locomo").cmp(&u8::from(right.dataset != "locomo")))
        .then_with(|| left.bundle_identity.cmp(&right.bundle_identity))
}

fn confirmation_bundle_priority(left: &QueryBundle, right: &QueryBundle) -> Ordering {
    bundle_min_displacement(right)
        .cmp(&bundle_min_displacement(left))
        .then_with(|| left.bundle_identity.cmp(&right.bundle_identity))
}

fn pair_is_v2_disagreement(item: &ReviewItem) -> bool {
    item.positive_v2_position > item.negative_v2_position
}

fn pair_is_v2_confirmation(item: &ReviewItem) -> bool {
    item.positive_v2_position < item.negative_v2_position
}

fn pair_displacement(item: &ReviewItem) -> usize {
    item.positive_v2_position
        .abs_diff(item.negative_v2_position)
}

fn bundle_v2_disagreements(bundle: &QueryBundle) -> usize {
    bundle
        .challengers
        .iter()
        .filter(|challenger| challenger.positive_v2_position > challenger.negative_v2_position)
        .count()
}

fn bundle_max_displacement(bundle: &QueryBundle) -> usize {
    bundle
        .challengers
        .iter()
        .map(|challenger| {
            challenger
                .positive_v2_position
                .abs_diff(challenger.negative_v2_position)
        })
        .max()
        .unwrap_or(0)
}

fn bundle_min_displacement(bundle: &QueryBundle) -> usize {
    bundle
        .challengers
        .iter()
        .map(|challenger| {
            challenger
                .positive_v2_position
                .abs_diff(challenger.negative_v2_position)
        })
        .min()
        .unwrap_or(0)
}

fn bundle_identity(items: &[ReviewItem]) -> String {
    let mut judgments = items
        .iter()
        .map(|item| item.judgment_identity.as_str())
        .collect::<Vec<_>>();
    judgments.sort_unstable();
    let mut hasher = Sha256::new();
    hasher.update(b"phoenix.qps.semantic-review-bundle/v1\0");
    for judgment in judgments {
        hasher.update(judgment.as_bytes());
        hasher.update([0]);
    }
    format!("{:x}", hasher.finalize())
}

fn refuse_overwrite(path: &Path) -> Result<()> {
    if path.exists() {
        bail!("refusing to overwrite {}", path.display());
    }
    Ok(())
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path, label: &str) -> Result<T> {
    serde_json::from_reader(BufReader::new(
        File::open(path).with_context(|| format!("open {label} {}", path.display()))?,
    ))
    .with_context(|| format!("decode {label} {}", path.display()))
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let parent = path.parent().context("output path has no parent")?;
    fs::create_dir_all(parent)?;
    let name = path.file_name().context("output path has no file name")?;
    let temporary = parent.join(format!(".{}.tmp", name.to_string_lossy()));
    let result = (|| {
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
        Result::<()>::Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn file_identity(path: &Path) -> Result<FileIdentity> {
    let bytes = fs::read(path).with_context(|| format!("read artifact {}", path.display()))?;
    Ok(FileIdentity {
        path: path.to_path_buf(),
        bytes: bytes.len() as u64,
        sha256: format!("{:x}", Sha256::digest(&bytes)),
    })
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct GroupKey {
    dataset: String,
    query_id: String,
    positive_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ReviewItem {
    judgment_identity: String,
    dataset: String,
    query_id: String,
    query: String,
    #[serde(default)]
    reference_answer: String,
    positive: ReviewDocument,
    negative: ReviewDocument,
    positive_v2_position: usize,
    negative_v2_position: usize,
    suggested_reason: JudgmentReasonV3,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
struct ReviewDocument {
    id: String,
    title: String,
    text: String,
    #[serde(default)]
    collected_at_unix_seconds: u64,
    #[serde(default)]
    source_time_label: String,
    #[serde(default)]
    reviewer_context: String,
}

#[derive(Debug, Deserialize)]
struct ReviewPacket {
    contract: String,
    schema_version: u16,
    items: Vec<ReviewItem>,
}

#[derive(Debug, Deserialize)]
struct ExclusionBatch {
    #[serde(default)]
    excluded_batch: Option<ExpectedFileIdentity>,
    items: Vec<ExclusionItem>,
}

#[derive(Debug, Deserialize)]
struct ExclusionItem {
    judgment_identity: String,
}

#[derive(Debug, Deserialize)]
struct BundleExclusionBatch {
    #[serde(default)]
    excluded_batch: Option<ExpectedFileIdentity>,
    bundles: Vec<BundleExclusion>,
}

#[derive(Debug, Deserialize)]
struct BundleExclusion {
    challengers: Vec<ExclusionItem>,
}

#[derive(Debug, Deserialize)]
struct ExpectedFileIdentity {
    path: PathBuf,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Deserialize)]
struct Phase5Receipt {
    technical_failure_classes: Vec<Phase5ClassCount>,
}

#[derive(Debug, Deserialize)]
struct Phase5ClassCount {
    reason: JudgmentReasonV3,
    authoritative_reviewed: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct QueryBundle {
    bundle_identity: String,
    dataset: String,
    query_id: String,
    query: String,
    reference_answer: String,
    positive: ReviewDocument,
    challengers: Vec<BundleChallenger>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct BundleChallenger {
    judgment_identity: String,
    negative: ReviewDocument,
    positive_v2_position: usize,
    negative_v2_position: usize,
    suggested_reason: JudgmentReasonV3,
}

#[derive(Debug, Deserialize, Serialize)]
struct BundleBatch {
    contract: String,
    schema_version: u16,
    source_packet: FileIdentity,
    phase_5: FileIdentity,
    excluded_batch: Option<FileIdentity>,
    excluded_items: usize,
    policy: BundlePolicy,
    requested_bundles: usize,
    selected_bundles: usize,
    selected_pairs: usize,
    v2_disagreements: usize,
    bundles_with_three_to_five_challengers: usize,
    class_projection: Vec<ClassProjection>,
    bundles: Vec<QueryBundle>,
}

#[derive(Debug, Deserialize, Serialize)]
struct BundlePolicy {
    authoritative_evidence_created: bool,
    requires_pairwise_semantic_review: bool,
    bundle_semantics: String,
    selection: String,
    decision_output: String,
    attestation: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct ClassProjection {
    reason: JudgmentReasonV3,
    initial: usize,
    selected: usize,
    projected: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct FileIdentity {
    path: PathBuf,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Serialize)]
struct BundleAudit {
    contract: &'static str,
    schema_version: u16,
    batch: FileIdentity,
    bundles: usize,
    pairs: usize,
    unique_queries: usize,
    unique_judgment_identities: usize,
    bundles_with_three_to_five_challengers: usize,
    all_classes_projected_to_target: bool,
    semantic_decisions_emitted: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct Publication {
    contract: &'static str,
    output: FileIdentity,
    selected_bundles: usize,
    selected_pairs: usize,
    v2_disagreements: usize,
    authoritative_evidence_created: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct AuditPublication {
    contract: &'static str,
    output: FileIdentity,
    bundles: usize,
    pairs: usize,
    all_classes_projected_to_target: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundle(id: char, reasons: &[JudgmentReasonV3]) -> QueryBundle {
        QueryBundle {
            bundle_identity: id.to_string().repeat(64),
            dataset: "fixture".into(),
            query_id: id.to_string(),
            query: "query".into(),
            reference_answer: String::new(),
            positive: document("positive"),
            challengers: reasons
                .iter()
                .enumerate()
                .map(|(index, reason)| BundleChallenger {
                    judgment_identity: format!("{id}{index}").repeat(32),
                    negative: document(&format!("negative-{index}")),
                    positive_v2_position: 5,
                    negative_v2_position: index,
                    suggested_reason: *reason,
                })
                .collect(),
        }
    }

    fn document(id: &str) -> ReviewDocument {
        ReviewDocument {
            id: id.into(),
            title: String::new(),
            text: id.into(),
            collected_at_unix_seconds: 0,
            source_time_label: String::new(),
            reviewer_context: String::new(),
        }
    }

    #[test]
    fn selection_closes_the_rarest_class_first() {
        let common = bundle('a', &[JudgmentReasonV3::WeakFieldEvidence; 3]);
        let rare = bundle('b', &[JudgmentReasonV3::FuzzyCollision; 3]);
        let mut counts = [99; REASONS.len()];
        counts[reason_index(JudgmentReasonV3::FuzzyCollision)] = 0;
        let selected = select_bundles(vec![common, rare], counts, 1);
        assert_eq!(selected[0].query_id, "b");
    }

    #[test]
    fn validation_rejects_underfilled_bundles() {
        let invalid = bundle('a', &[JudgmentReasonV3::PhraseOrderFailure; 2]);
        assert!(validate_bundles(&[invalid]).is_err());
    }

    #[test]
    fn identity_is_independent_of_pair_order() {
        let mut items = vec![review_item("a"), review_item("b"), review_item("c")];
        let first = bundle_identity(&items);
        items.reverse();
        assert_eq!(first, bundle_identity(&items));
    }

    #[test]
    fn confirmation_filter_and_priority_require_clear_v2_correct_pairs() {
        let mut correct = review_item("a");
        correct.positive_v2_position = 1;
        correct.negative_v2_position = 8;
        let incorrect = review_item("b");
        assert!(pair_is_v2_confirmation(&correct));
        assert!(!pair_is_v2_confirmation(&incorrect));

        let mut closer = bundle('c', &[JudgmentReasonV3::PhraseOrderFailure; 3]);
        for challenger in &mut closer.challengers {
            challenger.positive_v2_position = 1;
            challenger.negative_v2_position = 3;
        }
        let mut clearer = bundle('d', &[JudgmentReasonV3::PhraseOrderFailure; 3]);
        for challenger in &mut clearer.challengers {
            challenger.positive_v2_position = 1;
            challenger.negative_v2_position = 9;
        }
        let mut candidates = [closer, clearer];
        candidates.sort_by(confirmation_bundle_priority);
        assert_eq!(candidates[0].query_id, "d");
    }

    fn review_item(id: &str) -> ReviewItem {
        ReviewItem {
            judgment_identity: id.repeat(64),
            dataset: "fixture".into(),
            query_id: "query".into(),
            query: "query".into(),
            reference_answer: String::new(),
            positive: document("positive"),
            negative: document(id),
            positive_v2_position: 4,
            negative_v2_position: 0,
            suggested_reason: JudgmentReasonV3::ScatteredTerms,
        }
    }
}
