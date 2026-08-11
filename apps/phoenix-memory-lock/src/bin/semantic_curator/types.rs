use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize)]
pub(super) struct BundleBatch {
    pub(super) contract: String,
    pub(super) schema_version: u32,
    pub(super) bundles: Vec<QueryBundle>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(super) struct QueryBundle {
    pub(super) bundle_identity: String,
    pub(super) dataset: String,
    pub(super) query_id: String,
    pub(super) query: String,
    pub(super) reference_answer: String,
    pub(super) positive: Document,
    pub(super) challengers: Vec<Challenger>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(super) struct Document {
    pub(super) id: String,
    pub(super) title: String,
    pub(super) text: String,
    #[serde(default)]
    pub(super) source_time_label: String,
    #[serde(default)]
    pub(super) reviewer_context: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(super) struct Challenger {
    pub(super) judgment_identity: String,
    pub(super) negative: Document,
    pub(super) suggested_reason: String,
}

#[derive(Clone)]
pub(super) struct RequestWork {
    pub(super) index: usize,
    pub(super) bundles: Vec<QueryBundle>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(super) struct CachedReview {
    pub(super) contract: String,
    pub(super) prompt_version: String,
    pub(super) model: String,
    #[serde(default)]
    pub(super) pairwise_prompt_version: String,
    #[serde(default)]
    pub(super) model_call_count: usize,
    pub(super) request_index: usize,
    pub(super) bundles: Vec<QueryBundle>,
    pub(super) positive_evidence_review: PositiveEvidenceReview,
    pub(super) forward_review: ModelReview,
    pub(super) reverse_review: ModelReview,
}

#[derive(Debug, Deserialize, Serialize)]
pub(super) struct PositiveEvidenceReview {
    pub(super) bundles: Vec<PositiveEvidenceAdjudication>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(super) struct PositiveEvidenceAdjudication {
    pub(super) bundle_slot: usize,
    pub(super) input_consistent_and_answerable: bool,
    pub(super) positive_fully_supports_information_need: bool,
    pub(super) positive_requires_unsupported_inference: bool,
    #[serde(default)]
    pub(super) query_entities_match_positive: bool,
    #[serde(default)]
    pub(super) all_requested_parts_supported: bool,
    #[serde(default)]
    pub(super) reference_component_count: u8,
    #[serde(default)]
    pub(super) supported_reference_component_count: u8,
    pub(super) confidence: f64,
    pub(super) rationale: String,
}

impl PositiveEvidenceAdjudication {
    pub(super) fn admits(&self, confidence_floor: f64, has_reference_answer: bool) -> bool {
        self.input_consistent_and_answerable
            && self.positive_fully_supports_information_need
            && !self.positive_requires_unsupported_inference
            && self.query_entities_match_positive
            && self.all_requested_parts_supported
            && self.reference_component_count == self.supported_reference_component_count
            && (!has_reference_answer || self.reference_component_count > 0)
            && self.confidence >= confidence_floor
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub(super) struct ModelReview {
    pub(super) bundles: Vec<BundleReview>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(super) struct BundleReview {
    pub(super) bundle_slot: usize,
    pub(super) decisions: Vec<Adjudication>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(super) struct Adjudication {
    pub(super) challenger_slot: usize,
    pub(super) verdict: CandidateVerdict,
    pub(super) candidate_a_relevance: u8,
    pub(super) candidate_b_relevance: u8,
    pub(super) candidate_a_fully_supports_information_need: bool,
    pub(super) candidate_b_fully_supports_information_need: bool,
    pub(super) candidate_a_requires_unsupported_inference: bool,
    pub(super) candidate_b_requires_unsupported_inference: bool,
    pub(super) input_consistent_and_answerable: bool,
    pub(super) query_discriminates_candidates: bool,
    pub(super) candidates_cover_disjoint_valid_facets: bool,
    pub(super) same_or_versioned_evidence: bool,
    pub(super) confidence: f64,
    pub(super) rationale: String,
}

impl Adjudication {
    pub(super) fn pair_relevance(&self, reversed: bool) -> (u8, u8) {
        if reversed {
            (self.candidate_b_relevance, self.candidate_a_relevance)
        } else {
            (self.candidate_a_relevance, self.candidate_b_relevance)
        }
    }

    pub(super) fn pair_support(&self, reversed: bool) -> (bool, bool) {
        if reversed {
            (
                self.candidate_b_fully_supports_information_need,
                self.candidate_a_fully_supports_information_need,
            )
        } else {
            (
                self.candidate_a_fully_supports_information_need,
                self.candidate_b_fully_supports_information_need,
            )
        }
    }

    pub(super) fn pair_unsupported_inference(&self, reversed: bool) -> (bool, bool) {
        if reversed {
            (
                self.candidate_b_requires_unsupported_inference,
                self.candidate_a_requires_unsupported_inference,
            )
        } else {
            (
                self.candidate_a_requires_unsupported_inference,
                self.candidate_b_requires_unsupported_inference,
            )
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum CandidateVerdict {
    CandidateAPreferred,
    CandidateBPreferred,
    Abstain,
}

impl CandidateVerdict {
    pub(super) fn as_pair(self, reversed: bool) -> Option<PairVerdict> {
        match self {
            Self::CandidateAPreferred if reversed => Some(PairVerdict::NegativePreferred),
            Self::CandidateAPreferred => Some(PairVerdict::PositivePreferred),
            Self::CandidateBPreferred if reversed => Some(PairVerdict::PositivePreferred),
            Self::CandidateBPreferred => Some(PairVerdict::NegativePreferred),
            Self::Abstain => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PairVerdict {
    PositivePreferred,
    NegativePreferred,
}

impl PairVerdict {
    pub(super) fn as_contract(self) -> &'static str {
        match self {
            Self::PositivePreferred => "positive_preferred",
            Self::NegativePreferred => "negative_preferred",
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct ExistingDecisionDocument {
    pub(super) contract: String,
    pub(super) decisions: Vec<ExistingDecision>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ExistingDecision {
    pub(super) judgment_identity: String,
}

#[derive(Serialize)]
pub(super) struct DecisionDocument<'a> {
    pub(super) contract: &'static str,
    pub(super) schema_version: u32,
    pub(super) reviewer_identity: &'a str,
    pub(super) reviewed_at_unix_seconds: u64,
    pub(super) attestation: &'static str,
    pub(super) authorization_context: &'static str,
    pub(super) decisions: &'a [Decision],
}

#[derive(Debug, Serialize)]
pub(super) struct Decision {
    pub(super) judgment_identity: String,
    pub(super) verdict: String,
    pub(super) reason: String,
    pub(super) source: String,
    pub(super) confidence: f64,
}

#[derive(Serialize)]
pub(super) struct CurationReceipt {
    pub(super) contract: &'static str,
    pub(super) schema_version: u32,
    pub(super) provider: &'static str,
    pub(super) prompt_version: &'static str,
    pub(super) producer_binary: FileIdentity,
    pub(super) packet_path: String,
    pub(super) packet_sha256: String,
    pub(super) model: String,
    pub(super) reviewer_identity: String,
    pub(super) authorization_context: &'static str,
    pub(super) existing_decisions_skipped: usize,
    pub(super) semantic_duplicate_bundles_skipped: usize,
    pub(super) duplicate_evidence_bundles_rejected: usize,
    pub(super) candidate_bundles_available: usize,
    pub(super) bundle_offset: usize,
    pub(super) dataset_filter: Option<String>,
    pub(super) bundles_reviewed: usize,
    pub(super) pairs_reviewed: usize,
    pub(super) positive_preferred: usize,
    pub(super) negative_preferred: usize,
    pub(super) abstained: usize,
    pub(super) below_confidence_floor: usize,
    pub(super) bidirectional_disagreements: usize,
    pub(super) grade_guard_rejections: usize,
    pub(super) semantic_guard_rejections: usize,
    pub(super) positive_evidence_rejections: usize,
    pub(super) duplicate_evidence_pair_rejections: usize,
    pub(super) decisive_decisions: usize,
    pub(super) confidence_floor: f64,
    pub(super) requests_per_minute: usize,
    pub(super) pairwise_cache_root: Option<String>,
    pub(super) pairwise_cache_reused_requests: usize,
    pub(super) request_count: usize,
    pub(super) model_call_count: usize,
    pub(super) decision_cuts: Vec<FileIdentity>,
    pub(super) forbidden_inputs_exposed: bool,
}

#[derive(Serialize)]
pub(super) struct FileIdentity {
    pub(super) path: String,
    pub(super) bytes: u64,
    pub(super) sha256: String,
}

pub(super) struct Args(Vec<(String, String)>);

impl Args {
    pub(super) fn parse(raw: impl Iterator<Item = String>) -> Result<Self> {
        let values = raw.collect::<Vec<_>>();
        if values.len() % 2 != 0 {
            bail!("arguments must be --name value pairs");
        }
        let mut pairs = Vec::with_capacity(values.len() / 2);
        for pair in values.chunks_exact(2) {
            if !pair[0].starts_with("--") || pairs.iter().any(|(name, _)| name == &pair[0]) {
                bail!("invalid or duplicate argument {}", pair[0]);
            }
            pairs.push((pair[0].clone(), pair[1].clone()));
        }
        Ok(Self(pairs))
    }

    pub(super) fn required(&self, name: &str) -> Result<&str> {
        self.0
            .iter()
            .find(|(candidate, _)| candidate == name)
            .map(|(_, value)| value.as_str())
            .with_context(|| format!("missing {name}"))
    }

    pub(super) fn required_path(&self, name: &str) -> Result<PathBuf> {
        self.required(name).map(PathBuf::from)
    }

    fn optional(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(candidate, _)| candidate == name)
            .map(|(_, value)| value.as_str())
    }

    pub(super) fn usize_or(&self, name: &str, default: usize) -> Result<usize> {
        self.optional(name)
            .map(|value| value.parse().with_context(|| format!("parse {name}")))
            .transpose()
            .map(|value| value.unwrap_or(default))
    }

    pub(super) fn optional_usize(&self, name: &str) -> Result<Option<usize>> {
        self.optional(name)
            .map(|value| value.parse().with_context(|| format!("parse {name}")))
            .transpose()
    }

    pub(super) fn optional_string(&self, name: &str) -> Option<String> {
        self.optional(name).map(str::to_owned)
    }

    pub(super) fn optional_path(&self, name: &str) -> Option<PathBuf> {
        self.optional(name).map(PathBuf::from)
    }

    pub(super) fn f64_or(&self, name: &str, default: f64) -> Result<f64> {
        self.optional(name)
            .map(|value| value.parse().with_context(|| format!("parse {name}")))
            .transpose()
            .map(|value| value.unwrap_or(default))
    }
}
