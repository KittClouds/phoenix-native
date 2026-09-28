use super::{analysis_core, features};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(super) enum DecisionLabel {
    Same,
    Different,
    Unknown,
}

impl From<DecisionLabel> for analysis_core::Label {
    fn from(value: DecisionLabel) -> Self {
        match value {
            DecisionLabel::Same => Self::Same,
            DecisionLabel::Different => Self::Different,
            DecisionLabel::Unknown => Self::Unknown,
        }
    }
}

impl From<analysis_core::Label> for DecisionLabel {
    fn from(value: analysis_core::Label) -> Self {
        match value {
            analysis_core::Label::Same => Self::Same,
            analysis_core::Label::Different => Self::Different,
            analysis_core::Label::Unknown => Self::Unknown,
        }
    }
}

#[derive(Deserialize)]
pub(super) struct Judgment {
    pub(super) packet_id: String,
    pub(super) judgment: DecisionLabel,
}

#[derive(Deserialize)]
pub(super) struct Packet {
    pub(super) packet_id: String,
    pub(super) lexical_pair: [String; 2],
    pub(super) left_context: String,
    pub(super) right_context: String,
}

#[derive(Deserialize)]
pub(super) struct Endpoint {
    pub(super) node_id: String,
    pub(super) field: String,
}

#[derive(Deserialize)]
pub(super) struct LedgerRow {
    pub(super) edge_key: String,
    pub(super) candidate_id: String,
    pub(super) lexical_pair: [String; 2],
    pub(super) split: String,
    pub(super) reviewer_packet_ids: [String; 3],
    pub(super) left: Endpoint,
    pub(super) right: Endpoint,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
pub(super) struct LeafCounts {
    #[serde(default)]
    pub(super) same: usize,
    #[serde(default)]
    pub(super) different: usize,
    #[serde(default)]
    pub(super) unknown: usize,
}

#[derive(Clone)]
pub(super) struct ContextDoc {
    pub(super) external_id: u64,
    pub(super) candidate_id: String,
    pub(super) focal: String,
    pub(super) context: String,
    pub(super) field: String,
    pub(super) node_id: String,
}

#[derive(Clone)]
pub(super) struct ProbeQuery {
    pub(super) candidate_id: String,
    pub(super) lexical_pair: [String; 2],
    pub(super) focal: String,
    pub(super) replacement: String,
    pub(super) context: String,
    pub(super) field: String,
    pub(super) query_node: String,
    pub(super) target_document: u64,
    pub(super) target_label: DecisionLabel,
    pub(super) transport_eligible: bool,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ScoredHit {
    pub(super) external_id: u64,
    pub(super) score: f32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Lane {
    Original,
    Unconditional,
    CartE0,
    SelectiveE1,
    Oracle,
}

impl Lane {
    pub(super) const ALL: [Self; 5] = [
        Self::Original,
        Self::Unconditional,
        Self::CartE0,
        Self::SelectiveE1,
        Self::Oracle,
    ];

    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::Original => "L0_ORIGINAL",
            Self::Unconditional => "L1_UNCONDITIONAL",
            Self::CartE0 => "L2_CART_E0",
            Self::SelectiveE1 => "L3_SELECTIVE_E1",
            Self::Oracle => "L4_LABEL_ORACLE",
        }
    }

    pub(super) const fn alpha(self) -> f32 {
        match self {
            Self::Original => 0.0,
            Self::Unconditional | Self::CartE0 | Self::Oracle => 1.0,
            Self::SelectiveE1 => super::E1_ALPHA,
        }
    }
}

#[derive(Serialize)]
pub(super) struct CandidateSupport {
    pub(super) candidate_id: String,
    pub(super) lexical_pair: [String; 2],
    pub(super) fit_same: usize,
    pub(super) fit_different: usize,
    pub(super) fit_unknown: usize,
    pub(super) e1_relation_status: &'static str,
}

#[derive(Serialize)]
pub(super) struct LaneReceipt {
    pub(super) lane: &'static str,
    pub(super) alpha: f32,
    pub(super) queries: usize,
    pub(super) transport_eligible_queries: usize,
    pub(super) labels: LabelCounts,
    pub(super) baseline_target_hits: usize,
    pub(super) target_hits_after_lane: usize,
    pub(super) same_target_recoveries: usize,
    pub(super) different_target_false_admissions: usize,
    pub(super) unknown_target_false_admissions: usize,
    pub(super) newly_recovered_target_docs: usize,
    pub(super) gate_allow: usize,
    pub(super) gate_refuse: usize,
    pub(super) gate_abstain: usize,
    pub(super) observed_decisions: ObservedDecisions,
    pub(super) unlabeled_transport_candidates: usize,
    pub(super) mean_same_target_reciprocal_rank: f64,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub(super) struct DecisionCounts {
    pub(super) allow: usize,
    pub(super) refuse: usize,
    pub(super) abstain: usize,
}

impl DecisionCounts {
    pub(super) fn add(&mut self, decision: DecisionLabel) {
        match decision {
            DecisionLabel::Same => self.allow += 1,
            DecisionLabel::Different => self.refuse += 1,
            DecisionLabel::Unknown => self.abstain += 1,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub(super) struct ObservedDecisions {
    pub(super) same: DecisionCounts,
    pub(super) different: DecisionCounts,
    pub(super) unknown: DecisionCounts,
}

impl ObservedDecisions {
    pub(super) fn add(&mut self, actual: DecisionLabel, decision: DecisionLabel) {
        match actual {
            DecisionLabel::Same => self.same.add(decision),
            DecisionLabel::Different => self.different.add(decision),
            DecisionLabel::Unknown => self.unknown.add(decision),
        }
    }
}

#[derive(Serialize)]
pub(super) struct TreePathStep {
    pub(super) feature: String,
    pub(super) threshold: f64,
    pub(super) observed: f64,
    pub(super) branch: &'static str,
}

#[derive(Serialize)]
pub(super) struct HoldoutAllowAudit {
    pub(super) edge_key: String,
    pub(super) candidate_id: String,
    pub(super) lexical_pair: [String; 2],
    pub(super) actual: DecisionLabel,
    pub(super) predicted: DecisionLabel,
    pub(super) tree_path: Vec<TreePathStep>,
    pub(super) leaf_counts: LeafCounts,
    pub(super) pair_evidence: features::PairFeatures,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub(super) struct LabelCounts {
    pub(super) same: usize,
    pub(super) different: usize,
    pub(super) unknown: usize,
}

impl LabelCounts {
    pub(super) fn add(&mut self, label: DecisionLabel) {
        match label {
            DecisionLabel::Same => self.same += 1,
            DecisionLabel::Different => self.different += 1,
            DecisionLabel::Unknown => self.unknown += 1,
        }
    }
}

#[derive(Serialize)]
pub(super) struct Receipt {
    pub(super) schema: &'static str,
    pub(super) date: &'static str,
    pub(super) status: &'static str,
    pub(super) evidence_boundary: &'static str,
    pub(super) acquisition_root_sha256: String,
    pub(super) judgments_sha256: [String; 3],
    pub(super) engineering_e0_receipt_sha256: String,
    pub(super) packet_sha256: String,
    pub(super) ledger_sha256: String,
    pub(super) relation_support_floor: [usize; 2],
    pub(super) selective_e1_alpha: f32,
    pub(super) context_queries: usize,
    pub(super) context_documents: usize,
    pub(super) holdout_labels: LabelCounts,
    pub(super) transport_eligible_queries: usize,
    pub(super) transport_eligible_labels: LabelCounts,
    pub(super) candidate_support: Vec<CandidateSupport>,
    pub(super) full_local_tree: analysis_core::Tree,
    pub(super) baseline_holdout_allows: Vec<HoldoutAllowAudit>,
    pub(super) lanes: Vec<LaneReceipt>,
    pub(super) authority_updated: bool,
    pub(super) product_serving_changed: bool,
}
