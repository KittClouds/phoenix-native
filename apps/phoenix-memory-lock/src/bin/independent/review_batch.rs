use std::cmp::Ordering;
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use hashbrown::{HashMap, HashSet};
use phoenix_lexical_qps::JudgmentReasonV3;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const PACKET_CONTRACT_V1: &str = "phoenix.qps.relevance-review-packet/v1";
const PACKET_CONTRACT_V2: &str = "phoenix.qps.relevance-review-packet/v2";
const BATCH_CONTRACT: &str = "phoenix.qps.semantic-review-batch/v2";
const MAX_BATCH_ITEMS: usize = 1_100;
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

pub(crate) fn prepare(
    packet_path: &Path,
    locomo_path: &Path,
    exclude_batch_path: Option<&Path>,
    limit: usize,
    output_path: &Path,
) -> Result<Publication> {
    if limit == 0 || limit > MAX_BATCH_ITEMS {
        bail!("review batch limit must be between 1 and {MAX_BATCH_ITEMS}");
    }
    if output_path.exists() {
        bail!("refusing to overwrite {}", output_path.display());
    }
    let mut packet: ReviewPacket = serde_json::from_reader(BufReader::new(
        File::open(packet_path)
            .with_context(|| format!("open review packet {}", packet_path.display()))?,
    ))
    .with_context(|| format!("decode review packet {}", packet_path.display()))?;
    let valid_contract = (packet.contract == PACKET_CONTRACT_V1 && packet.schema_version == 1)
        || (packet.contract == PACKET_CONTRACT_V2 && packet.schema_version == 2);
    if !valid_contract || packet.items.is_empty() {
        bail!("invalid or empty review packet");
    }
    enrich_locomo_temporal_context(&mut packet.items, locomo_path)?;
    let (excluded_batch, excluded_items) = if let Some(path) = exclude_batch_path {
        let excluded: ExclusionBatch = serde_json::from_reader(BufReader::new(
            File::open(path).with_context(|| format!("open excluded batch {}", path.display()))?,
        ))
        .with_context(|| format!("decode excluded batch {}", path.display()))?;
        if excluded.contract != BATCH_CONTRACT || excluded.items.is_empty() {
            bail!("invalid or empty excluded review batch");
        }
        let identities = excluded
            .items
            .into_iter()
            .map(|item| item.judgment_identity)
            .collect::<HashSet<_>>();
        let before = packet.items.len();
        packet
            .items
            .retain(|item| !identities.contains(&item.judgment_identity));
        (Some(file_identity(path)?), before - packet.items.len())
    } else {
        (None, 0)
    };

    let selected = select_items(packet.items, limit);
    let locomo_items_with_source_time_labels = selected
        .iter()
        .filter(|item| {
            item.dataset == "locomo"
                && !item.positive.source_time_label.is_empty()
                && !item.negative.source_time_label.is_empty()
        })
        .count();
    let locomo_items_with_reference_answers = selected
        .iter()
        .filter(|item| item.dataset == "locomo" && !item.reference_answer.is_empty())
        .count();
    let locomo_items_with_multimodal_context = selected
        .iter()
        .filter(|item| {
            item.dataset == "locomo"
                && (!item.positive.reviewer_context.is_empty()
                    || !item.negative.reviewer_context.is_empty())
        })
        .count();
    if locomo_items_with_source_time_labels != locomo_items_with_reference_answers {
        bail!("selected LoCoMo review context is incomplete");
    }
    let v2_disagreements = selected
        .iter()
        .filter(|item| item.positive_v2_position > item.negative_v2_position)
        .count();
    let reason_counts = REASONS
        .into_iter()
        .map(|reason| ReasonCount {
            reason,
            count: selected
                .iter()
                .filter(|item| item.suggested_reason == reason)
                .count(),
        })
        .collect::<Vec<_>>();
    let mut datasets = selected
        .iter()
        .map(|item| item.dataset.as_str())
        .collect::<Vec<_>>();
    datasets.sort_unstable();
    let mut dataset_counts: Vec<DatasetCount> = Vec::with_capacity(datasets.len());
    for dataset in datasets {
        if let Some(last) = dataset_counts.last_mut() {
            if last.dataset == dataset {
                last.count += 1;
                continue;
            }
        }
        dataset_counts.push(DatasetCount {
            dataset: dataset.to_owned(),
            count: 1,
        });
    }
    let batch = SemanticReviewBatch {
        contract: BATCH_CONTRACT,
        schema_version: 1,
        source_packet: file_identity(packet_path)?,
        excluded_batch,
        excluded_items,
        policy: BatchPolicy {
            verdicts_emitted: false,
            authoritative_evidence_created: false,
            requires_pairwise_semantic_review: true,
            priority: "deterministic interleave of high-impact and close-rank cases, LoCoMo transfer value, stable identity",
            class_balancing:
                "deterministic round-robin across 11 technical failure classes and rank-displacement extremes",
            temporal_context: "LoCoMo source timestamps copied from the official conversation sessions",
            reference_answer_context: "LoCoMo reference answers are reviewer-only context and never ranker features",
            multimodal_context: "LoCoMo image captions and image queries are reviewer-only context and never ranker features",
        },
        requested_items: limit,
        selected_items: selected.len(),
        v2_disagreements,
        reason_counts,
        dataset_counts,
        locomo_items_with_source_time_labels,
        locomo_items_with_reference_answers,
        locomo_items_with_multimodal_context,
        items: selected,
    };
    write_json_atomic(output_path, &batch)?;
    Ok(Publication {
        contract: BATCH_CONTRACT,
        output: file_identity(output_path)?,
        selected_items: batch.selected_items,
        v2_disagreements,
        authoritative_evidence_created: false,
    })
}

pub(crate) fn audit(batch_path: &Path, output_path: &Path) -> Result<AuditPublication> {
    if output_path.exists() {
        bail!("refusing to overwrite {}", output_path.display());
    }
    let batch: AuditBatch = serde_json::from_reader(BufReader::new(
        File::open(batch_path)
            .with_context(|| format!("open review batch {}", batch_path.display()))?,
    ))
    .with_context(|| format!("decode review batch {}", batch_path.display()))?;
    if batch.contract != BATCH_CONTRACT || batch.items.is_empty() {
        bail!("invalid or empty semantic review batch");
    }
    let metrics = audit_metrics(&batch.items);
    let receipt = ReviewBatchAudit {
        contract: "phoenix.qps.semantic-review-batch-audit/v1",
        schema_version: 1,
        batch: file_identity(batch_path)?,
        selected_items: batch.items.len(),
        v2_disagreements: batch
            .items
            .iter()
            .filter(|item| item.positive_v2_position > item.negative_v2_position)
            .count(),
        unique_queries: batch
            .items
            .iter()
            .map(|item| item.query_id.as_str())
            .collect::<HashSet<_>>()
            .len(),
        rank_displacement: metrics.rank_displacement,
        first_round_class_min: metrics.first_round_class_min,
        first_round_class_max: metrics.first_round_class_max,
        max_consecutive_same_class: metrics.max_consecutive_same_class,
        semantic_difficulty_claimed: false,
        limitation: "rank displacement measures review impact, not semantic ambiguity; only completed semantic decisions can calibrate true difficulty",
    };
    write_json_atomic(output_path, &receipt)?;
    Ok(AuditPublication {
        contract: receipt.contract,
        output: file_identity(output_path)?,
        selected_items: receipt.selected_items,
        v2_disagreements: receipt.v2_disagreements,
        semantic_difficulty_claimed: false,
    })
}

fn enrich_locomo_temporal_context(items: &mut [ReviewItem], path: &Path) -> Result<()> {
    let dataset = super::source::load_locomo(path, 0..10)?;
    let answers = dataset
        .queries
        .iter()
        .map(|query| (query.id.as_str(), query.reference_answer.as_str()))
        .collect::<HashMap<_, _>>();
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
        .collect::<HashMap<_, _>>();
    for item in items.iter_mut().filter(|item| item.dataset == "locomo") {
        enrich_document(&mut item.positive, &context)?;
        enrich_document(&mut item.negative, &context)?;
        let query_id = item
            .query_id
            .strip_prefix("fuzzy:")
            .unwrap_or(&item.query_id);
        item.reference_answer = answers
            .get(query_id)
            .with_context(|| format!("missing reference answer for {}", item.query_id))?
            .to_string();
        if item.positive.source_time_label.is_empty() || item.negative.source_time_label.is_empty()
        {
            bail!(
                "LoCoMo review item {} lacks source time labels",
                item.judgment_identity
            );
        }
    }
    Ok(())
}

fn enrich_document(
    document: &mut ReviewDocument,
    context: &HashMap<&str, (u64, &str, &str)>,
) -> Result<()> {
    let (collected_at, label, reviewer_context) = context
        .get(document.id.as_str())
        .with_context(|| format!("missing temporal context for {}", document.id))?;
    document.collected_at_unix_seconds = *collected_at;
    document.source_time_label = (*label).to_owned();
    document.reviewer_context = (*reviewer_context).to_owned();
    Ok(())
}

fn select_items(mut items: Vec<ReviewItem>, limit: usize) -> Vec<ReviewItem> {
    items.sort_by(priority_order);
    let mut buckets = REASONS
        .into_iter()
        .map(|reason| {
            interleave_extremes(
                items
                    .iter()
                    .filter(|item| item.suggested_reason == reason)
                    .cloned()
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();
    let mut cursors = vec![0_usize; buckets.len()];
    let mut selected = Vec::with_capacity(limit.min(items.len()));
    while selected.len() < limit {
        let mut advanced = false;
        for (bucket, cursor) in buckets.iter_mut().zip(&mut cursors) {
            if let Some(item) = bucket.get(*cursor) {
                selected.push(item.clone());
                *cursor += 1;
                advanced = true;
                if selected.len() == limit {
                    break;
                }
            }
        }
        if !advanced {
            break;
        }
    }
    selected
}

fn interleave_extremes(items: Vec<ReviewItem>) -> Vec<ReviewItem> {
    let mut interleaved = Vec::with_capacity(items.len());
    let mut items = items.into_iter();
    while let Some(front) = items.next() {
        interleaved.push(front);
        if let Some(back) = items.next_back() {
            interleaved.push(back);
        }
    }
    interleaved
}

fn audit_metrics(items: &[ReviewItem]) -> QueueMetrics {
    let mut rank_displacement = RankDisplacementCounts::default();
    for item in items {
        match rank_displacement_value(item) {
            0 | 1 => rank_displacement.close_rank_0_to_1 += 1,
            2 | 3 => rank_displacement.medium_rank_2_to_3 += 1,
            _ => rank_displacement.wide_rank_4_plus += 1,
        }
    }
    let first_round = &items[..items.len().min(REASONS.len() * 5)];
    let class_counts = REASONS.map(|reason| {
        first_round
            .iter()
            .filter(|item| item.suggested_reason == reason)
            .count()
    });
    let mut max_consecutive_same_class = 0_usize;
    let mut current_run = 0_usize;
    let mut previous = None;
    for item in items {
        if previous == Some(item.suggested_reason) {
            current_run += 1;
        } else {
            previous = Some(item.suggested_reason);
            current_run = 1;
        }
        max_consecutive_same_class = max_consecutive_same_class.max(current_run);
    }
    QueueMetrics {
        rank_displacement,
        first_round_class_min: class_counts.into_iter().min().unwrap_or(0),
        first_round_class_max: class_counts.into_iter().max().unwrap_or(0),
        max_consecutive_same_class,
    }
}

fn priority_order(left: &ReviewItem, right: &ReviewItem) -> Ordering {
    let left_wrong = left.positive_v2_position > left.negative_v2_position;
    let right_wrong = right.positive_v2_position > right.negative_v2_position;
    right_wrong
        .cmp(&left_wrong)
        .then_with(|| rank_displacement(right).cmp(&rank_displacement(left)))
        .then_with(|| dataset_priority(left).cmp(&dataset_priority(right)))
        .then_with(|| left.judgment_identity.cmp(&right.judgment_identity))
}

fn rank_displacement(item: &ReviewItem) -> usize {
    rank_displacement_value(item)
}

fn rank_displacement_value(item: &ReviewItem) -> usize {
    item.positive_v2_position
        .abs_diff(item.negative_v2_position)
}

fn dataset_priority(item: &ReviewItem) -> u8 {
    u8::from(item.dataset != "locomo")
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let parent = path.parent().context("review batch output has no parent")?;
    fs::create_dir_all(parent)?;
    let name = path
        .file_name()
        .context("review batch output has no name")?;
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

#[derive(Clone, Debug, Deserialize, Serialize)]
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
    contract: String,
    items: Vec<ExclusionItem>,
}

#[derive(Debug, Deserialize)]
struct ExclusionItem {
    judgment_identity: String,
}

#[derive(Debug, Deserialize)]
struct AuditBatch {
    contract: String,
    items: Vec<ReviewItem>,
}

#[derive(Debug, Serialize)]
struct SemanticReviewBatch {
    contract: &'static str,
    schema_version: u16,
    source_packet: FileIdentity,
    excluded_batch: Option<FileIdentity>,
    excluded_items: usize,
    policy: BatchPolicy,
    requested_items: usize,
    selected_items: usize,
    v2_disagreements: usize,
    reason_counts: Vec<ReasonCount>,
    dataset_counts: Vec<DatasetCount>,
    locomo_items_with_source_time_labels: usize,
    locomo_items_with_reference_answers: usize,
    locomo_items_with_multimodal_context: usize,
    items: Vec<ReviewItem>,
}

#[derive(Debug, Serialize)]
struct BatchPolicy {
    verdicts_emitted: bool,
    authoritative_evidence_created: bool,
    requires_pairwise_semantic_review: bool,
    priority: &'static str,
    class_balancing: &'static str,
    temporal_context: &'static str,
    reference_answer_context: &'static str,
    multimodal_context: &'static str,
}

#[derive(Debug, Serialize)]
struct ReasonCount {
    reason: JudgmentReasonV3,
    count: usize,
}

#[derive(Debug, Serialize)]
struct DatasetCount {
    dataset: String,
    count: usize,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
struct RankDisplacementCounts {
    close_rank_0_to_1: usize,
    medium_rank_2_to_3: usize,
    wide_rank_4_plus: usize,
}

#[derive(Clone, Copy, Debug)]
struct QueueMetrics {
    rank_displacement: RankDisplacementCounts,
    first_round_class_min: usize,
    first_round_class_max: usize,
    max_consecutive_same_class: usize,
}

#[derive(Debug, Serialize)]
struct ReviewBatchAudit {
    contract: &'static str,
    schema_version: u16,
    batch: FileIdentity,
    selected_items: usize,
    v2_disagreements: usize,
    unique_queries: usize,
    rank_displacement: RankDisplacementCounts,
    first_round_class_min: usize,
    first_round_class_max: usize,
    max_consecutive_same_class: usize,
    semantic_difficulty_claimed: bool,
    limitation: &'static str,
}

#[derive(Debug, Serialize)]
pub(crate) struct AuditPublication {
    contract: &'static str,
    output: FileIdentity,
    selected_items: usize,
    v2_disagreements: usize,
    semantic_difficulty_claimed: bool,
}

#[derive(Debug, Serialize)]
struct FileIdentity {
    path: PathBuf,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct Publication {
    contract: &'static str,
    output: FileIdentity,
    selected_items: usize,
    v2_disagreements: usize,
    authoritative_evidence_created: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, reason: JudgmentReasonV3, dataset: &str, positive: usize) -> ReviewItem {
        ReviewItem {
            judgment_identity: id.repeat(64),
            dataset: dataset.into(),
            query_id: id.into(),
            query: "query".into(),
            reference_answer: "answer".into(),
            positive: ReviewDocument {
                id: "positive".into(),
                title: String::new(),
                text: "positive".into(),
                collected_at_unix_seconds: 1,
                source_time_label: "one".into(),
                reviewer_context: String::new(),
            },
            negative: ReviewDocument {
                id: "negative".into(),
                title: String::new(),
                text: "negative".into(),
                collected_at_unix_seconds: 2,
                source_time_label: "two".into(),
                reviewer_context: String::new(),
            },
            positive_v2_position: positive,
            negative_v2_position: 0,
            suggested_reason: reason,
        }
    }

    #[test]
    fn selection_balances_classes_before_taking_a_second_item() {
        let selected = select_items(
            vec![
                item("a", JudgmentReasonV3::PhraseOrderFailure, "scifact", 5),
                item("b", JudgmentReasonV3::PhraseOrderFailure, "locomo", 6),
                item("c", JudgmentReasonV3::ScatteredTerms, "scifact", 1),
            ],
            3,
        );
        assert_eq!(
            selected[0].suggested_reason,
            JudgmentReasonV3::ScatteredTerms
        );
        assert_eq!(
            selected[1].suggested_reason,
            JudgmentReasonV3::PhraseOrderFailure
        );
        assert_eq!(
            selected[2].suggested_reason,
            JudgmentReasonV3::PhraseOrderFailure
        );
        assert_eq!(selected[1].dataset, "locomo");
    }

    #[test]
    fn selection_interleaves_high_impact_and_close_rank_items() {
        let selected = select_items(
            vec![
                item("a", JudgmentReasonV3::PhraseOrderFailure, "scifact", 10),
                item("b", JudgmentReasonV3::PhraseOrderFailure, "scifact", 8),
                item("c", JudgmentReasonV3::PhraseOrderFailure, "scifact", 2),
                item("d", JudgmentReasonV3::PhraseOrderFailure, "scifact", 1),
            ],
            4,
        );
        assert_eq!(rank_displacement(&selected[0]), 10);
        assert_eq!(rank_displacement(&selected[1]), 1);
        assert_eq!(rank_displacement(&selected[2]), 8);
        assert_eq!(rank_displacement(&selected[3]), 2);
    }

    #[test]
    fn queue_audit_never_claims_semantic_difficulty() {
        let items = vec![
            item("a", JudgmentReasonV3::PhraseOrderFailure, "scifact", 8),
            item("b", JudgmentReasonV3::ScatteredTerms, "locomo", 2),
            item("c", JudgmentReasonV3::ScatteredTerms, "locomo", 1),
        ];
        let metrics = audit_metrics(&items);
        assert_eq!(metrics.rank_displacement.close_rank_0_to_1, 1);
        assert_eq!(metrics.rank_displacement.medium_rank_2_to_3, 1);
        assert_eq!(metrics.rank_displacement.wide_rank_4_plus, 1);
        assert_eq!(metrics.max_consecutive_same_class, 2);
    }
}
