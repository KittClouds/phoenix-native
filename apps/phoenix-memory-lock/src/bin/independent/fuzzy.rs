use anyhow::Result;
use phoenix_lexical_qps::{
    rank_evidence_schema_identity_v3, Expansion, JudgmentReasonV3, JudgmentSourceV3, KeyedIdentity,
    PairwiseJudgmentDraftV3, PairwiseJudgmentV3, QueryGroup, RelevanceLedgerV3, SearchScratch,
    WorkspaceIdentityKey,
};

use super::artifact::ReviewItem;
use super::build::{
    candidate_pool, hard_negatives, review_document, split_groups, DatasetAudit, PreparedDataset,
    NEGATIVES_PER_POSITIVE, TOP_K,
};
use super::partition::MiningPartitions;
use super::source::{SourceDataset, SourceQuery};

const TARGET_FUZZY_JUDGMENTS_PER_DATASET: usize = 120;

#[allow(clippy::too_many_arguments)]
pub(super) fn append_fuzzy_review_candidates(
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
    let mut hits = Vec::with_capacity(160);
    let mut audit = DatasetAudit::new(dataset);
    for query in &dataset.queries {
        if audit.judgments >= TARGET_FUZZY_JUDGMENTS_PER_DATASET {
            break;
        }
        let Some(plan) = FuzzyPlan::new(&query.text) else {
            continue;
        };
        let expansions = plan.expansions();
        let groups = expansions
            .iter()
            .map(|values| QueryGroup { expansions: values })
            .collect::<Vec<_>>();
        hits.clear();
        prepared
            .index
            .search_groups_evidence_into(&groups, TOP_K, &mut scratch, &mut hits)?;
        let Some(positive_position) = hits.iter().position(|hit| {
            query
                .relevant
                .contains_key(&prepared.documents[hit.external_id as usize - 1].id)
        }) else {
            audit.oracle_missing_from_pool += 1;
            continue;
        };
        let positive = hits[positive_position];
        let negatives = hard_negatives(&hits, query, prepared, positive_position, partitions);
        if negatives.len() != NEGATIVES_PER_POSITIVE {
            audit.insufficient_same_tier_negatives += 1;
            continue;
        }
        let fuzzy_query = fuzzy_query(query, &plan.rendered_query);
        let pool = candidate_pool(&hits, prepared);
        for negative_position in negatives {
            if audit.judgments >= TARGET_FUZZY_JUDGMENTS_PER_DATASET {
                break;
            }
            let negative = hits[negative_position];
            let judgment = PairwiseJudgmentV3::from_draft(PairwiseJudgmentDraftV3 {
                workspace_identity: KeyedIdentity::derive(
                    key,
                    b"workspace",
                    b"qps-v3-independent-data",
                ),
                query_identity: KeyedIdentity::derive(
                    key,
                    b"private-query",
                    format!("{}:{}", dataset.name, fuzzy_query.id).as_bytes(),
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
                split_groups: split_groups(
                    dataset,
                    &fuzzy_query,
                    prepared,
                    positive,
                    negative,
                    key,
                ),
                frozen_holdout: None,
                v2_model_identity,
                challenger_model_identity: rank_evidence_schema_identity_v3(),
                reason: JudgmentReasonV3::FuzzyCollision,
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
                query_id: fuzzy_query.id.clone(),
                query: fuzzy_query.text.clone(),
                reference_answer: fuzzy_query.reference_answer.clone(),
                positive: review_document(&prepared.documents[positive.external_id as usize - 1]),
                negative: review_document(&prepared.documents[negative.external_id as usize - 1]),
                positive_v2_position: positive_position,
                negative_v2_position: negative_position,
                suggested_reason: JudgmentReasonV3::FuzzyCollision,
            });
            ledger.append(judgment).map_err(anyhow::Error::msg)?;
            *generation += 1;
            audit.judgments += 1;
        }
        audit.eligible_queries += 1;
    }
    audit
        .reasons
        .insert(JudgmentReasonV3::FuzzyCollision, audit.judgments);
    Ok(audit)
}

fn fuzzy_query(query: &SourceQuery, rendered: &str) -> SourceQuery {
    let mut fuzzy = query.clone();
    fuzzy.id = format!("fuzzy:{}", query.id);
    fuzzy.text = rendered.to_owned();
    fuzzy.family = format!("{}:fuzzy", query.family);
    fuzzy.entity_family = format!("{}:fuzzy", query.entity_family);
    fuzzy.collection_cohort = format!("{}:fuzzy", query.collection_cohort);
    fuzzy
}

struct FuzzyPlan {
    terms: Vec<String>,
    typo: String,
    target: usize,
    rendered_query: String,
}

impl FuzzyPlan {
    fn new(query: &str) -> Option<Self> {
        let terms = query
            .split(|character: char| !character.is_alphanumeric())
            .filter(|term| !term.is_empty())
            .map(str::to_ascii_lowercase)
            .collect::<Vec<_>>();
        let target = terms
            .iter()
            .position(|term| term.len() >= 6 && !STOP_WORDS.contains(&term.as_str()))?;
        let mut typo = terms[target].clone();
        typo.remove(typo.len() / 2);
        if typo.len() < 4 {
            return None;
        }
        let mut rendered = terms.clone();
        rendered[target] = typo.clone();
        Some(Self {
            terms,
            typo,
            target,
            rendered_query: rendered.join(" "),
        })
    }

    fn expansions(&self) -> Vec<Vec<Expansion<'_>>> {
        self.terms
            .iter()
            .enumerate()
            .map(|(index, term)| {
                if index == self.target {
                    vec![
                        Expansion {
                            term: &self.typo,
                            quality: 1.0,
                        },
                        Expansion {
                            term,
                            quality: 0.85,
                        },
                    ]
                } else {
                    vec![Expansion { term, quality: 1.0 }]
                }
            })
            .collect()
    }
}

const STOP_WORDS: &[&str] = &[
    "about", "after", "before", "could", "should", "their", "there", "these", "those", "which",
    "would",
];

fn hex<const N: usize>(bytes: [u8; N]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_plan_preserves_original_as_lower_quality_expansion() {
        let plan = FuzzyPlan::new("Which astronomical telescope found the signal").unwrap();
        let expansions = plan.expansions();
        let target = &expansions[plan.target];
        assert_eq!(target.len(), 2);
        assert_eq!(target[0].term, plan.typo);
        assert_eq!(target[0].quality, 1.0);
        assert_eq!(target[1].term, plan.terms[plan.target]);
        assert_eq!(target[1].quality, 0.85);
        assert_ne!(plan.typo, plan.terms[plan.target]);
        assert!(plan.rendered_query.contains(&plan.typo));
    }

    #[test]
    fn fuzzy_plan_rejects_queries_without_substantive_terms() {
        assert!(FuzzyPlan::new("which there about").is_none());
    }
}
