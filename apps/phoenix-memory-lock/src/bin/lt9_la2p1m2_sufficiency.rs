//! Frozen outcome sufficiency gate; physical document pairs are the unit.

use crate::CorpusReplay;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

const MIN_INVALID_CONTESTED_ACTIONABLE: usize = 12;
const MIN_INVALID_CORPORA: usize = 2;
const MIN_INVALID_RELATIONS: usize = 3;
const MIN_INVALID_CORPUS_SHARDS: usize = 4;
const MIN_VALID_CONTESTED_ACTIONABLE: usize = 40;
const MIN_VALID_CORPORA: usize = 2;
const MIN_VALID_RELATIONS: usize = 3;

#[derive(Serialize)]
pub(crate) struct SufficiencyGate {
    minimum_invalid_actionable_contested: usize,
    minimum_invalid_contributing_corpora: usize,
    minimum_invalid_candidate_relations: usize,
    minimum_invalid_corpus_shards: usize,
    minimum_valid_actionable_contested: usize,
    minimum_valid_contributing_corpora: usize,
    minimum_valid_candidate_relations: usize,
    observed_invalid_actionable_contested_physical: usize,
    observed_invalid_contributing_corpora: usize,
    observed_invalid_candidate_relations: usize,
    observed_invalid_corpus_shards: usize,
    observed_valid_actionable_contested_physical: usize,
    observed_valid_contributing_corpora: usize,
    observed_valid_candidate_relations: usize,
    observed_mixed_directional_physical_episodes: usize,
    observed_invalid_actionable_contested_directional_rows: usize,
    observed_valid_actionable_contested_directional_rows: usize,
    pub(crate) sufficient: bool,
}

pub(crate) fn build(corpora: &[CorpusReplay]) -> SufficiencyGate {
    let mut invalid_count = 0usize;
    let mut valid_count = 0usize;
    let mut invalid_corpora = BTreeSet::new();
    let mut valid_corpora = BTreeSet::new();
    let mut invalid_relations = BTreeSet::new();
    let mut valid_relations = BTreeSet::new();
    let mut invalid_corpus_shards = BTreeSet::new();
    let mut mixed_physical = 0usize;
    let mut invalid_directional_rows = 0usize;
    let mut valid_directional_rows = 0usize;
    for corpus in corpora {
        let mut physical =
            BTreeMap::<(u64, u64), (bool, bool, BTreeSet<&'static str>, usize)>::new();
        for row in &corpus.episodes {
            if !row.actionable || row.route_class != "CONTESTED_UNIQUE" {
                continue;
            }
            let state = physical
                .entry((row.nomination_document, row.witness_document))
                .or_insert_with(|| (false, false, BTreeSet::new(), row.shard));
            state.2.insert(row.relation);
            match row.baseline_outcome {
                "INVALID" => {
                    state.1 = true;
                    invalid_directional_rows += 1;
                }
                "VALID" => {
                    state.0 = true;
                    valid_directional_rows += 1;
                }
                _ => {}
            }
        }
        for (_, (has_valid, has_invalid, relations, row_shard)) in physical {
            match (has_valid, has_invalid) {
                (true, false) => {
                    valid_count += 1;
                    valid_corpora.insert(corpus.corpus_id.as_str());
                    valid_relations.extend(relations);
                }
                (false, true) => {
                    invalid_count += 1;
                    invalid_corpora.insert(corpus.corpus_id.as_str());
                    invalid_relations.extend(relations);
                    invalid_corpus_shards.insert((corpus.corpus_id.as_str(), row_shard));
                }
                (true, true) => mixed_physical += 1,
                (false, false) => {}
            }
        }
    }
    let sufficient = invalid_count >= MIN_INVALID_CONTESTED_ACTIONABLE
        && invalid_corpora.len() >= MIN_INVALID_CORPORA
        && invalid_relations.len() >= MIN_INVALID_RELATIONS
        && invalid_corpus_shards.len() >= MIN_INVALID_CORPUS_SHARDS
        && valid_count >= MIN_VALID_CONTESTED_ACTIONABLE
        && valid_corpora.len() >= MIN_VALID_CORPORA
        && valid_relations.len() >= MIN_VALID_RELATIONS;
    SufficiencyGate {
        minimum_invalid_actionable_contested: MIN_INVALID_CONTESTED_ACTIONABLE,
        minimum_invalid_contributing_corpora: MIN_INVALID_CORPORA,
        minimum_invalid_candidate_relations: MIN_INVALID_RELATIONS,
        minimum_invalid_corpus_shards: MIN_INVALID_CORPUS_SHARDS,
        minimum_valid_actionable_contested: MIN_VALID_CONTESTED_ACTIONABLE,
        minimum_valid_contributing_corpora: MIN_VALID_CORPORA,
        minimum_valid_candidate_relations: MIN_VALID_RELATIONS,
        observed_invalid_actionable_contested_physical: invalid_count,
        observed_invalid_contributing_corpora: invalid_corpora.len(),
        observed_invalid_candidate_relations: invalid_relations.len(),
        observed_invalid_corpus_shards: invalid_corpus_shards.len(),
        observed_valid_actionable_contested_physical: valid_count,
        observed_valid_contributing_corpora: valid_corpora.len(),
        observed_valid_candidate_relations: valid_relations.len(),
        observed_mixed_directional_physical_episodes: mixed_physical,
        observed_invalid_actionable_contested_directional_rows: invalid_directional_rows,
        observed_valid_actionable_contested_directional_rows: valid_directional_rows,
        sufficient,
    }
}
