//! P1P3E1 engineering transport harness.
//!
//! Runs lexical transport through the real QPS query-group API on the frozen
//! natural-context holdout. This is a paired context-retrieval probe, not a
//! BEIR/qrels result and not a formal P1P3 qualification.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use phoenix_lexical_qps::{
    DocumentInput, Expansion, FieldConfig, MAXIMUM_QUERY_GROUPS, QpsBuilder, QpsConfig, QpsIndex,
    QueryGroup, SearchHit, SearchScratch,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[path = "lt9_la2p1o2_analyze_core.rs"]
mod analysis_core;
#[path = "lt9_la2p1o2_features.rs"]
mod features;
pub use features::PairFeatures;

#[cfg(test)]
#[path = "lt9_la2p1p3e1_transport_tests.rs"]
mod tests;
#[path = "lt9_la2p1p3e1_transport_helpers.rs"]
mod transport_helpers;
#[path = "lt9_la2p1p3e1_transport_types.rs"]
mod transport_types;
use transport_helpers::{focal_term, query_tokens, strip_focal};
use transport_types::*;

mod p1o1 {
    #[derive(Clone, Debug)]
    pub struct Occurrence {
        pub field: String,
        pub excerpt: String,
    }
}

const TOP_K: usize = 100;
const E1_ALPHA: f32 = 0.5;
const MIN_RELATION_SAME: usize = 4;
const MIN_RELATION_DIFFERENT: usize = 4;

fn sha256(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("parse {}", path.display()))
}

fn main() -> Result<()> {
    let args: Vec<_> = env::args().collect();
    ensure!(
        args.len() == 5,
        "usage: lt9_la2p1p3e1_transport <acquisition-sealed> <review-sealed> <e0-receipt.json> <new-output-dir>"
    );
    let acquisition = PathBuf::from(&args[1]);
    let review_dir = PathBuf::from(&args[2]);
    let e0_path = PathBuf::from(&args[3]);
    let output_dir = PathBuf::from(&args[4]);
    ensure!(
        !output_dir.exists(),
        "refusing to overwrite output directory"
    );

    let packet_path = acquisition.join("reviewer-1-review").join("packets.json");
    let ledger_path = acquisition.join("private-ledger.json");
    let labels_paths = [
        review_dir.join("judgments.json"),
        review_dir.join("judgments2.json"),
        review_dir.join("judgments3.json"),
    ];
    let packets: Vec<Packet> = read_json(&packet_path)?;
    let ledger: Vec<LedgerRow> = read_json(&ledger_path)?;
    let review_rows: [Vec<Judgment>; 3] = [
        read_json(&labels_paths[0])?,
        read_json(&labels_paths[1])?,
        read_json(&labels_paths[2])?,
    ];
    ensure!(
        packets.len() == 120 && ledger.len() == 120,
        "expected frozen P1P3 120-pair set"
    );
    for rows in &review_rows {
        ensure!(
            rows.len() == 120,
            "each functional signal file must contain 120 rows"
        );
    }

    let mut packet_by_id = HashMap::with_capacity(packets.len());
    for packet in packets {
        ensure!(
            packet_by_id
                .insert(packet.packet_id.clone(), packet)
                .is_none(),
            "duplicate packet id"
        );
    }
    let mut review_maps: [HashMap<String, DecisionLabel>; 3] =
        std::array::from_fn(|_| HashMap::new());
    for slot in 0..3 {
        for row in &review_rows[slot] {
            ensure!(
                review_maps[slot]
                    .insert(row.packet_id.clone(), row.judgment)
                    .is_none(),
                "duplicate review id"
            );
        }
    }

    let mut examples = Vec::with_capacity(ledger.len());
    let mut packets_by_edge = HashMap::with_capacity(ledger.len());
    let mut labels_by_edge = HashMap::with_capacity(ledger.len());
    let mut candidate_pairs = BTreeMap::new();
    let mut support = BTreeMap::<String, (usize, usize, usize, [String; 2])>::new();
    let mut label_by_node_pair = HashMap::<(String, String), DecisionLabel>::new();

    for row in &ledger {
        ensure!(
            row.split == "fit" || row.split == "holdout",
            "invalid frozen split"
        );
        if let Some(pair) = candidate_pairs.get(&row.candidate_id) {
            ensure!(
                pair == &row.lexical_pair,
                "candidate id maps to multiple lexical pairs"
            );
        } else {
            candidate_pairs.insert(row.candidate_id.clone(), row.lexical_pair.clone());
        }
        let packet_id = &row.reviewer_packet_ids[0];
        let packet = packet_by_id
            .get(packet_id)
            .context("reviewer-1 packet missing")?;
        ensure!(
            packet.lexical_pair == row.lexical_pair,
            "packet/ledger candidate mismatch"
        );
        let labels = std::array::from_fn::<_, 3, _>(|slot| {
            review_maps[slot]
                .get(&row.reviewer_packet_ids[slot])
                .copied()
        });
        ensure!(
            labels.iter().all(Option::is_some),
            "missing aligned functional label"
        );
        let label = labels[0].unwrap();
        ensure!(
            labels.iter().all(|candidate| *candidate == Some(label)),
            "functional signals diverged; frozen E0 alignment no longer holds"
        );
        packets_by_edge.insert(row.edge_key.clone(), packet);
        labels_by_edge.insert(row.edge_key.clone(), label);
        if row.split == "fit" {
            let count = support.entry(row.candidate_id.clone()).or_insert((
                0,
                0,
                0,
                row.lexical_pair.clone(),
            ));
            match label {
                DecisionLabel::Same => count.0 += 1,
                DecisionLabel::Different => count.1 += 1,
                DecisionLabel::Unknown => count.2 += 1,
            }
        }
        if row.split == "holdout" {
            label_by_node_pair.insert((row.left.node_id.clone(), row.right.node_id.clone()), label);
        }

        let left = p1o1::Occurrence {
            field: row.left.field.clone(),
            excerpt: packet.left_context.clone(),
        };
        let right = p1o1::Occurrence {
            field: row.right.field.clone(),
            excerpt: packet.right_context.clone(),
        };
        let pair = features::pair_features(
            &features::context_features(&left, &row.lexical_pair[0], &row.lexical_pair[1]),
            &features::context_features(&right, &row.lexical_pair[0], &row.lexical_pair[1]),
        );
        let mut feature_rows = analysis_core::make_features(&pair, analysis_core::View::FullLocal);
        let example = analysis_core::Example {
            packet_id: row.edge_key.clone(),
            candidate_id: row.candidate_id.clone(),
            split: row.split.clone(),
            overlap_band: "unspecified".to_owned(),
            left_node: row.left.node_id.clone(),
            right_node: row.right.node_id.clone(),
            label: label.into(),
            features: std::mem::take(&mut feature_rows),
        };
        examples.push(example);
    }

    let model = analysis_core::analyze_view(&examples, analysis_core::View::FullLocal);
    let tree = model.tree;
    let holdout_rows: Vec<_> = ledger.iter().filter(|row| row.split == "holdout").collect();
    ensure!(holdout_rows.len() == 60, "expected frozen 60-pair holdout");
    let baseline_holdout_allows: Vec<HoldoutAllowAudit> = holdout_rows
        .iter()
        .filter_map(|row| {
            let example = examples
                .iter()
                .find(|example| example.packet_id == row.edge_key)?;
            let (leaf, predicted, tree_path) = explain_tree(&tree, &example.features);
            if DecisionLabel::from(predicted) != DecisionLabel::Same {
                return None;
            }
            let packet = packets_by_edge.get(&row.edge_key)?;
            let evidence = features::pair_features(
                &features::context_features(
                    &p1o1::Occurrence {
                        field: row.left.field.clone(),
                        excerpt: packet.left_context.clone(),
                    },
                    &row.lexical_pair[0],
                    &row.lexical_pair[1],
                ),
                &features::context_features(
                    &p1o1::Occurrence {
                        field: row.right.field.clone(),
                        excerpt: packet.right_context.clone(),
                    },
                    &row.lexical_pair[0],
                    &row.lexical_pair[1],
                ),
            );
            Some(HoldoutAllowAudit {
                edge_key: row.edge_key.clone(),
                candidate_id: row.candidate_id.clone(),
                lexical_pair: row.lexical_pair.clone(),
                actual: DecisionLabel::from(example.label),
                predicted: DecisionLabel::from(predicted),
                tree_path,
                leaf_counts: leaf.counts().expect("tree traversal ends at a leaf"),
                pair_evidence: evidence,
            })
        })
        .collect();
    ensure!(
        baseline_holdout_allows.len() == 6,
        "frozen E0 full-local holdout should have six ALLOW decisions"
    );
    let mut docs = Vec::with_capacity(holdout_rows.len());
    let mut queries = Vec::with_capacity(holdout_rows.len());
    let mut holdout_labels = LabelCounts::default();

    for (index, row) in holdout_rows.iter().enumerate() {
        let packet = packets_by_edge
            .get(&row.edge_key)
            .context("packet missing by edge")?;
        let label = *labels_by_edge
            .get(&row.edge_key)
            .context("label missing by edge")?;
        holdout_labels.add(label);
        let left_focal = focal_term(&packet.left_context, &row.lexical_pair)
            .context("left context lacks candidate focal marker")?;
        let right_focal = focal_term(&packet.right_context, &row.lexical_pair)
            .context("right context lacks candidate focal marker")?;
        let replacement = if left_focal.eq_ignore_ascii_case(&row.lexical_pair[0]) {
            row.lexical_pair[1].clone()
        } else if left_focal.eq_ignore_ascii_case(&row.lexical_pair[1]) {
            row.lexical_pair[0].clone()
        } else {
            bail!("left focal token is outside the declared candidate pair")
        };
        ensure!(
            right_focal.eq_ignore_ascii_case(&row.lexical_pair[0])
                || right_focal.eq_ignore_ascii_case(&row.lexical_pair[1]),
            "right focal token is outside the declared candidate pair"
        );
        let transport_eligible = right_focal.eq_ignore_ascii_case(&replacement);
        docs.push(ContextDoc {
            external_id: index as u64,
            candidate_id: row.candidate_id.clone(),
            focal: right_focal,
            context: packet.right_context.clone(),
            field: row.right.field.clone(),
            node_id: row.right.node_id.clone(),
        });
        queries.push(ProbeQuery {
            candidate_id: row.candidate_id.clone(),
            lexical_pair: row.lexical_pair.clone(),
            focal: left_focal,
            replacement,
            context: packet.left_context.clone(),
            field: row.left.field.clone(),
            query_node: row.left.node_id.clone(),
            target_document: index as u64,
            target_label: label,
            transport_eligible,
        });
    }

    let qps = build_index(&docs)?;
    let candidate_support = support
        .iter()
        .map(
            |(candidate_id, (same, different, unknown, lexical_pair))| CandidateSupport {
                candidate_id: candidate_id.clone(),
                lexical_pair: lexical_pair.clone(),
                fit_same: *same,
                fit_different: *different,
                fit_unknown: *unknown,
                e1_relation_status: if relation_support_qualified(*same, *different) {
                    "MINIMUM_ENGINEERING_SUPPORT"
                } else if *same == 0 {
                    "INSUFFICIENT_POSITIVES"
                } else if *different == 0 {
                    "INSUFFICIENT_NEGATIVES"
                } else {
                    "PARTIAL_SUPPORT"
                },
            },
        )
        .collect::<Vec<_>>();
    let transport_eligible_labels = queries
        .iter()
        .filter(|query| query.transport_eligible)
        .fold(LabelCounts::default(), |mut counts, query| {
            counts.add(query.target_label);
            counts
        });
    let mut lane_receipts = Vec::with_capacity(Lane::ALL.len());
    for lane in Lane::ALL {
        lane_receipts.push(run_lane(
            lane,
            &queries,
            &docs,
            &qps,
            &tree,
            &support,
            &label_by_node_pair,
        )?);
    }

    let acquisition_root = acquisition.join("pre-review-root.json");
    let receipt = Receipt {
        schema: "phoenix.lexical.lt9-la2-p1p3e1-context-transport/v1",
        date: "2026-09-28",
        status: "ENGINEERING_CONTEXT_RETRIEVAL_COMPLETED_NOT_QUALIFICATION",
        evidence_boundary: "Natural excerpt context-pair retrieval over the frozen P1P3 holdout; functional labels are proxy evidence; not BEIR topical retrieval, independent-human validation, authority promotion, or serving qualification.",
        acquisition_root_sha256: sha256(&acquisition_root)?,
        judgments_sha256: [
            sha256(&labels_paths[0])?,
            sha256(&labels_paths[1])?,
            sha256(&labels_paths[2])?,
        ],
        engineering_e0_receipt_sha256: sha256(&e0_path)?,
        packet_sha256: sha256(&packet_path)?,
        ledger_sha256: sha256(&ledger_path)?,
        relation_support_floor: [MIN_RELATION_SAME, MIN_RELATION_DIFFERENT],
        selective_e1_alpha: E1_ALPHA,
        context_queries: queries.len(),
        context_documents: docs.len(),
        holdout_labels,
        transport_eligible_queries: queries
            .iter()
            .filter(|query| query.transport_eligible)
            .count(),
        transport_eligible_labels,
        candidate_support,
        full_local_tree: tree.clone(),
        baseline_holdout_allows,
        lanes: lane_receipts,
        authority_updated: false,
        product_serving_changed: false,
    };
    fs::create_dir_all(&output_dir)?;
    let receipt_path = output_dir.join("transport-receipt.json");
    fs::write(&receipt_path, serde_json::to_vec_pretty(&receipt)?)?;
    println!("{}", serde_json::to_string_pretty(&receipt)?);
    println!("transport_receipt_sha256={}", sha256(&receipt_path)?);
    Ok(())
}

fn build_index(documents: &[ContextDoc]) -> Result<QpsIndex> {
    let fields = [FieldConfig::new("context", 1.0, 0.75, 0.0)];
    let mut config = QpsConfig {
        maximum_query_groups: MAXIMUM_QUERY_GROUPS,
        maximum_candidate_pool: documents.len().max(64),
        minimum_candidate_pool: documents.len().min(64),
        proximity_weight: 0.0,
        order_weight: 0.0,
        phrase_weight: 0.0,
        segment_weight: 0.0,
        ..QpsConfig::default()
    };
    config.maximum_expansions_per_group = 2;
    let mut builder = QpsBuilder::new(Vec::from(fields).into_boxed_slice(), config)?;
    for document in documents {
        let context = strip_focal(&document.context);
        builder.insert(DocumentInput {
            external_id: document.external_id,
            fields: &[context.as_str()],
        })?;
    }
    builder.build().map_err(Into::into)
}

fn run_lane(
    lane: Lane,
    queries: &[ProbeQuery],
    docs: &[ContextDoc],
    qps: &QpsIndex,
    tree: &analysis_core::Tree,
    support: &BTreeMap<String, (usize, usize, usize, [String; 2])>,
    oracle_labels: &HashMap<(String, String), DecisionLabel>,
) -> Result<LaneReceipt> {
    let mut receipt = LaneReceipt {
        lane: lane.name(),
        alpha: lane.alpha(),
        queries: queries.len(),
        transport_eligible_queries: queries
            .iter()
            .filter(|query| query.transport_eligible)
            .count(),
        labels: LabelCounts::default(),
        baseline_target_hits: 0,
        target_hits_after_lane: 0,
        same_target_recoveries: 0,
        different_target_false_admissions: 0,
        unknown_target_false_admissions: 0,
        newly_recovered_target_docs: 0,
        gate_allow: 0,
        gate_refuse: 0,
        gate_abstain: 0,
        observed_decisions: ObservedDecisions::default(),
        unlabeled_transport_candidates: 0,
        mean_same_target_reciprocal_rank: 0.0,
    };
    let mut scratch = SearchScratch::default();
    let mut base_hits = Vec::<SearchHit>::with_capacity(TOP_K);
    let mut expanded_hits = Vec::<SearchHit>::with_capacity(TOP_K);
    let mut same_rr_sum = 0.0;
    let mut same_count = 0usize;

    for query in queries {
        if query.transport_eligible {
            receipt.labels.add(query.target_label);
        }
        search_query(qps, query, None, 1.0, &mut scratch, &mut base_hits)?;
        let base_ids = base_hits
            .iter()
            .map(|hit| hit.external_id)
            .collect::<HashSet<_>>();
        let baseline_target_hit = base_ids.contains(&query.target_document);
        if query.transport_eligible {
            receipt.baseline_target_hits += usize::from(baseline_target_hit);
        }

        let mut merged = base_hits
            .iter()
            .map(|hit| ScoredHit {
                external_id: hit.external_id,
                score: hit.score,
            })
            .collect::<Vec<_>>();
        if lane != Lane::Original {
            search_query(
                qps,
                query,
                Some(&query.replacement),
                lane.alpha(),
                &mut scratch,
                &mut expanded_hits,
            )?;
            for hit in &expanded_hits {
                if base_ids.contains(&hit.external_id) {
                    continue;
                }
                let Some(document) = docs.get(hit.external_id as usize) else {
                    continue;
                };
                if document.candidate_id != query.candidate_id {
                    continue;
                }
                if !document.focal.eq_ignore_ascii_case(&query.replacement) {
                    continue;
                }
                let decision = match lane {
                    Lane::Original => DecisionLabel::Unknown,
                    Lane::Unconditional => DecisionLabel::Same,
                    Lane::CartE0 => predict_pair(tree, query, document),
                    Lane::SelectiveE1 => selective_decision(tree, query, document, support),
                    Lane::Oracle => oracle_labels
                        .get(&(query.query_node.clone(), document.node_id.clone()))
                        .copied()
                        .unwrap_or(DecisionLabel::Unknown),
                };
                if let Some(actual) = oracle_labels
                    .get(&(query.query_node.clone(), document.node_id.clone()))
                    .copied()
                {
                    receipt.observed_decisions.add(actual, decision);
                } else {
                    receipt.unlabeled_transport_candidates += 1;
                }
                match decision {
                    DecisionLabel::Same => {
                        receipt.gate_allow += 1;
                        merged.push(ScoredHit {
                            external_id: hit.external_id,
                            score: hit.score,
                        });
                    }
                    DecisionLabel::Different => receipt.gate_refuse += 1,
                    DecisionLabel::Unknown => receipt.gate_abstain += 1,
                }
            }
        }
        merged.sort_unstable_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| a.external_id.cmp(&b.external_id))
        });
        merged.dedup_by_key(|hit| hit.external_id);
        let lane_ids = merged
            .iter()
            .map(|hit| hit.external_id)
            .collect::<HashSet<_>>();
        let target_after = lane_ids.contains(&query.target_document);
        if query.transport_eligible {
            receipt.target_hits_after_lane += usize::from(target_after);
        }
        if query.transport_eligible && !baseline_target_hit && target_after {
            receipt.newly_recovered_target_docs += 1;
            match query.target_label {
                DecisionLabel::Same => receipt.same_target_recoveries += 1,
                DecisionLabel::Different => receipt.different_target_false_admissions += 1,
                DecisionLabel::Unknown => receipt.unknown_target_false_admissions += 1,
            }
        }
        if query.transport_eligible && query.target_label == DecisionLabel::Same {
            same_count += 1;
            if let Some(position) = merged
                .iter()
                .position(|hit| hit.external_id == query.target_document)
            {
                same_rr_sum += 1.0 / (position + 1) as f64;
            }
        }
    }
    receipt.mean_same_target_reciprocal_rank = same_rr_sum / same_count.max(1) as f64;
    Ok(receipt)
}

fn search_query(
    qps: &QpsIndex,
    query: &ProbeQuery,
    alternate: Option<&str>,
    alpha: f32,
    scratch: &mut SearchScratch,
    hits: &mut Vec<SearchHit>,
) -> Result<()> {
    let tokens = query_tokens(&query.context);
    ensure!(
        !tokens.is_empty() && tokens.len() <= MAXIMUM_QUERY_GROUPS,
        "context query exceeds QPS group bound"
    );
    let alternate_storage = alternate.map(str::to_owned);
    let mut expansions = Vec::<Vec<Expansion<'_>>>::with_capacity(tokens.len());
    let mut inserted = false;
    for (token, focal) in &tokens {
        if *focal {
            inserted = true;
            if let Some(alt) = alternate_storage.as_deref() {
                expansions.push(vec![
                    Expansion {
                        term: token.as_str(),
                        quality: 1.0,
                    },
                    Expansion {
                        term: alt,
                        quality: alpha,
                    },
                ]);
            } else {
                expansions.push(vec![Expansion {
                    term: token.as_str(),
                    quality: 1.0,
                }]);
            }
        } else {
            expansions.push(vec![Expansion {
                term: token.as_str(),
                quality: 1.0,
            }]);
        }
    }
    ensure!(inserted, "focal token not found in tokenized context");
    let groups = expansions
        .iter()
        .map(|values| QueryGroup { expansions: values })
        .collect::<Vec<_>>();
    hits.clear();
    qps.search_groups_into(&groups, TOP_K, scratch, hits)?;
    Ok(())
}

fn predict_pair(
    tree: &analysis_core::Tree,
    query: &ProbeQuery,
    document: &ContextDoc,
) -> DecisionLabel {
    if document.focal.eq_ignore_ascii_case(&query.focal) {
        return DecisionLabel::Unknown;
    }
    let pair = features::pair_features(
        &features::context_features(
            &p1o1::Occurrence {
                field: query.field.clone(),
                excerpt: query.context.clone(),
            },
            &query.lexical_pair[0],
            &query.lexical_pair[1],
        ),
        &features::context_features(
            &p1o1::Occurrence {
                field: document.field.clone(),
                excerpt: document.context.clone(),
            },
            &query.lexical_pair[0],
            &query.lexical_pair[1],
        ),
    );
    let features = analysis_core::make_features(&pair, analysis_core::View::FullLocal);
    predict_tree(tree, &features).1.into()
}

fn selective_decision(
    tree: &analysis_core::Tree,
    query: &ProbeQuery,
    document: &ContextDoc,
    support: &BTreeMap<String, (usize, usize, usize, [String; 2])>,
) -> DecisionLabel {
    let Some((same, different, _, _)) = support.get(&query.candidate_id) else {
        return DecisionLabel::Unknown;
    };
    if !relation_support_qualified(*same, *different) {
        return DecisionLabel::Unknown;
    }
    let pair_prediction = predict_pair(tree, query, document);
    if pair_prediction != DecisionLabel::Same {
        return pair_prediction;
    }
    let pair = features::pair_features(
        &features::context_features(
            &p1o1::Occurrence {
                field: query.field.clone(),
                excerpt: query.context.clone(),
            },
            &query.lexical_pair[0],
            &query.lexical_pair[1],
        ),
        &features::context_features(
            &p1o1::Occurrence {
                field: document.field.clone(),
                excerpt: document.context.clone(),
            },
            &query.lexical_pair[0],
            &query.lexical_pair[1],
        ),
    );
    let feature_row = analysis_core::make_features(&pair, analysis_core::View::FullLocal);
    let (leaf, _) = predict_tree(tree, &feature_row);
    if leaf
        .counts()
        .is_some_and(|counts| counts.same > 0 && counts.different == 0 && counts.unknown == 0)
    {
        DecisionLabel::Same
    } else {
        DecisionLabel::Unknown
    }
}

fn relation_support_qualified(same: usize, different: usize) -> bool {
    same >= MIN_RELATION_SAME && different >= MIN_RELATION_DIFFERENT
}

fn predict_tree<'a>(
    tree: &'a analysis_core::Tree,
    features: &BTreeMap<String, f64>,
) -> (&'a analysis_core::Tree, analysis_core::Label) {
    match tree {
        analysis_core::Tree::Leaf { prediction, .. } => (tree, *prediction),
        analysis_core::Tree::Split {
            feature,
            threshold,
            left,
            right,
            ..
        } => {
            let value = *features.get(feature).unwrap_or(&0.0);
            if value <= *threshold {
                predict_tree(left, features)
            } else {
                predict_tree(right, features)
            }
        }
    }
}

fn explain_tree<'a>(
    tree: &'a analysis_core::Tree,
    features: &BTreeMap<String, f64>,
) -> (
    &'a analysis_core::Tree,
    analysis_core::Label,
    Vec<TreePathStep>,
) {
    let mut current = tree;
    let mut path = Vec::new();
    loop {
        match current {
            analysis_core::Tree::Leaf { prediction, .. } => return (current, *prediction, path),
            analysis_core::Tree::Split {
                feature,
                threshold,
                left,
                right,
                ..
            } => {
                let observed = *features.get(feature).unwrap_or(&0.0);
                let take_left = observed <= *threshold;
                path.push(TreePathStep {
                    feature: feature.clone(),
                    threshold: *threshold,
                    observed,
                    branch: if take_left { "LEFT" } else { "RIGHT" },
                });
                current = if take_left { left } else { right };
            }
        }
    }
}

trait LeafCountsAccess {
    fn counts(&self) -> Option<LeafCounts>;
}

impl LeafCountsAccess for analysis_core::Tree {
    fn counts(&self) -> Option<LeafCounts> {
        match self {
            analysis_core::Tree::Leaf { counts, .. } => Some(LeafCounts {
                same: counts.same,
                different: counts.different,
                unknown: counts.unknown,
            }),
            analysis_core::Tree::Split { .. } => None,
        }
    }
}
