//! Label-blind P1M2 structural preflight across the frozen corpus cohort.

use crate::lt9_la2p1m1_core::*;
use anyhow::{ensure, Context, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

pub const SCREEN_SCHEMA: &str = "phoenix.lexical.lt9-la2p1m2-screen/v1";
pub const DATE: &str = "2026-09-23";
pub const SHARDS: usize = 8;
pub const SCREEN_ORDER: [&str; 8] = [
    "cqadupstack",
    "arguana",
    "nfcorpus",
    "quora",
    "scidocs",
    "trec-covid",
    "scifact",
    "webis-touche2020",
];
pub const DISCOVERY_IDS: [&str; 7] = [
    "cqadupstack",
    "arguana",
    "nfcorpus",
    "quora",
    "scidocs",
    "trec-covid",
    "scifact",
];
pub const QUALIFICATION_ID: &str = "webis-touche2020";
pub const MIN_DISCOVERY_CONTESTED: usize = 240;
pub const MIN_CORPORA_WITH_15_CONTESTED: usize = 3;
pub const MIN_DISCOVERY_RELATIONS: usize = 4;
pub const MIN_DISCOVERY_CORPUS_SHARDS: usize = 8;

fn shard(document: u64, documents: u64) -> usize {
    ((document.saturating_mul(SHARDS as u64)) / documents).min((SHARDS - 1) as u64) as usize
}

fn unique_winner(counts: [u16; 3]) -> Option<usize> {
    let max = counts.iter().copied().max()?;
    if max == 0 {
        return None;
    }
    let mut winners = counts
        .iter()
        .enumerate()
        .filter(|(_, count)| **count == max);
    let winner = winners.next()?.0;
    winners.next().is_none().then_some(winner)
}

fn self_vote_count(candidate: usize, family: usize, marker_bit: usize) -> u16 {
    let marker = marker_names(family)[marker_bit];
    let spec = CANDIDATES[candidate];
    u16::from(spec.source.eq_ignore_ascii_case(marker))
        + u16::from(spec.target.eq_ignore_ascii_case(marker))
}

fn context_only_counts(event: Event) -> [u16; 3] {
    std::array::from_fn(|family| {
        marker_names(family)
            .iter()
            .enumerate()
            .filter(|(bit, _)| {
                let all_occurrences = event.features.family[family].marker_occurrences[*bit];
                all_occurrences > self_vote_count(event.candidate, family, *bit)
            })
            .count() as u16
    })
}

fn context_only_route(episode: Episode) -> Option<usize> {
    let left = unique_winner(context_only_counts(episode.nomination));
    let right = unique_winner(context_only_counts(episode.witness));
    match (left, right) {
        (Some(a), Some(b)) if a == b => Some(a),
        _ => None,
    }
}

fn active_families(event: Event) -> usize {
    event
        .features
        .family
        .iter()
        .filter(|family| family.distinct_count() > 0)
        .count()
}

#[derive(Default, Serialize)]
struct RelationCount {
    routed: usize,
    exclusive: usize,
    contested: usize,
    self_vote_changed_route: usize,
}

#[derive(Default, Serialize)]
struct StructuralSummary {
    event_count: usize,
    directional_episode_count: usize,
    physical_document_pair_count: usize,
    unique_plurality_episode_count: usize,
    exclusive_unique_episode_count: usize,
    contested_unique_episode_count: usize,
    contested_physical_document_pair_count: usize,
    contested_by_shard: [usize; SHARDS],
    routed_by_relation: BTreeMap<String, RelationCount>,
    self_vote_changed_episode_count: usize,
    self_vote_endpoint_route_changes: usize,
    context_only_pair_abstain_count: usize,
    context_only_endpoint_abstain_count: usize,
    context_only_endpoint_disagreement_count: usize,
}

impl StructuralSummary {
    fn add(&mut self, episode: Episode, route: Option<usize>) {
        let Some(route) = route else { return };
        self.unique_plurality_episode_count += 1;
        let nomination_active = active_families(episode.nomination);
        let witness_active = active_families(episode.witness);
        let exclusive = nomination_active == 1 && witness_active == 1;
        let relation = CANDIDATES[episode.candidate].id.to_owned();
        let per_relation = self.routed_by_relation.entry(relation).or_default();
        per_relation.routed += 1;
        if exclusive {
            self.exclusive_unique_episode_count += 1;
            per_relation.exclusive += 1;
        } else {
            self.contested_unique_episode_count += 1;
            per_relation.contested += 1;
        }
        let counterfactual = context_only_route(episode);
        if counterfactual != Some(route) {
            self.self_vote_changed_episode_count += 1;
            per_relation.self_vote_changed_route += 1;
        }
        if counterfactual.is_none() {
            self.context_only_pair_abstain_count += 1;
        }
        let nomination_context = unique_winner(context_only_counts(episode.nomination));
        let witness_context = unique_winner(context_only_counts(episode.witness));
        if nomination_context.is_none() || witness_context.is_none() {
            self.context_only_endpoint_abstain_count += 1;
        } else if nomination_context != witness_context {
            self.context_only_endpoint_disagreement_count += 1;
        }
        self.self_vote_endpoint_route_changes += usize::from(
            unique_winner(episode.nomination.features.counts(true))
                != unique_winner(context_only_counts(episode.nomination)),
        );
        self.self_vote_endpoint_route_changes += usize::from(
            unique_winner(episode.witness.features.counts(true))
                != unique_winner(context_only_counts(episode.witness)),
        );
    }
}

#[derive(Serialize)]
struct CorpusScreen {
    corpus_id: String,
    assigned_role: &'static str,
    corpus_path: String,
    archive_path: String,
    archive_sha256: String,
    archive_bytes: u64,
    corpus_sha256: String,
    corpus_bytes: u64,
    document_count: u64,
    structural: StructuralSummary,
}

#[derive(Serialize)]
struct ScreenReceipt {
    schema: &'static str,
    date: &'static str,
    scope: &'static str,
    corpus_text_only: bool,
    validity_labels_opened: bool,
    qrels_or_queries_opened: bool,
    frozen_order: Vec<&'static str>,
    discovery_ids: Vec<&'static str>,
    qualification_id: &'static str,
    structural_preflight: StructuralPreflight,
    corpora: Vec<CorpusScreen>,
}

#[derive(Serialize)]
struct StructuralPreflight {
    minimum_discovery_contested_episodes: usize,
    minimum_corpora_with_15_contested_episodes: usize,
    minimum_candidate_relations_with_contested_episodes: usize,
    minimum_corpus_shard_cells: usize,
    observed_discovery_unique_episodes: usize,
    observed_discovery_contested_episodes: usize,
    observed_corpora_with_15_contested_episodes: usize,
    observed_candidate_relations_with_contested_episodes: usize,
    observed_corpus_shard_cells: usize,
    passed: bool,
}

fn structural_preflight(corpora: &[CorpusScreen]) -> StructuralPreflight {
    let mut unique = 0usize;
    let mut contested = 0usize;
    let mut corpora_with_15 = 0usize;
    let mut relations = BTreeMap::<String, usize>::new();
    let mut corpus_shard_cells = 0usize;
    for corpus in corpora
        .iter()
        .filter(|row| DISCOVERY_IDS.contains(&row.corpus_id.as_str()))
    {
        unique += corpus.structural.unique_plurality_episode_count;
        let corpus_contested = corpus.structural.contested_unique_episode_count;
        contested += corpus_contested;
        corpora_with_15 += usize::from(corpus_contested >= 15);
        corpus_shard_cells += corpus
            .structural
            .contested_by_shard
            .iter()
            .filter(|&&count| count > 0)
            .count();
        for (relation, counts) in &corpus.structural.routed_by_relation {
            *relations.entry(relation.clone()).or_default() += counts.contested;
        }
    }
    let relation_count = relations.values().filter(|&&count| count > 0).count();
    let passed = contested >= MIN_DISCOVERY_CONTESTED
        && corpora_with_15 >= MIN_CORPORA_WITH_15_CONTESTED
        && relation_count >= MIN_DISCOVERY_RELATIONS
        && corpus_shard_cells >= MIN_DISCOVERY_CORPUS_SHARDS;
    StructuralPreflight {
        minimum_discovery_contested_episodes: MIN_DISCOVERY_CONTESTED,
        minimum_corpora_with_15_contested_episodes: MIN_CORPORA_WITH_15_CONTESTED,
        minimum_candidate_relations_with_contested_episodes: MIN_DISCOVERY_RELATIONS,
        minimum_corpus_shard_cells: MIN_DISCOVERY_CORPUS_SHARDS,
        observed_discovery_unique_episodes: unique,
        observed_discovery_contested_episodes: contested,
        observed_corpora_with_15_contested_episodes: corpora_with_15,
        observed_candidate_relations_with_contested_episodes: relation_count,
        observed_corpus_shard_cells: corpus_shard_cells,
        passed,
    }
}

fn hash_file(path: &Path) -> Result<String> {
    let file = File::open(path).with_context(|| format!("open archive {}", path.display()))?;
    let mut reader = BufReader::with_capacity(1024 * 1024, file);
    let mut hasher = Sha256::new();
    let mut chunk = vec![0u8; 1024 * 1024];
    loop {
        let count = reader.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        hasher.update(&chunk[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub fn run(args: &[String]) -> Result<()> {
    ensure!(
        args.len() == SCREEN_ORDER.len() + 1,
        "usage: --screen <receipt.json> <id=corpus.jsonl=archive.zip>... (exact frozen list required)"
    );
    let output = Path::new(&args[0]);
    let mut corpora = Vec::with_capacity(SCREEN_ORDER.len());
    for (index, spec) in args[1..].iter().enumerate() {
        let mut parts = spec.splitn(3, '=');
        let id = parts.next().context("missing corpus id")?;
        let corpus_path = parts.next().context("missing corpus.jsonl path")?;
        let archive_path = parts.next().context("missing source archive path")?;
        ensure!(id == SCREEN_ORDER[index], "{id} violates frozen order");
        let corpus_path = Path::new(corpus_path);
        let archive_path = Path::new(archive_path);
        let archive_sha256 = hash_file(archive_path)?;
        let archive_bytes = std::fs::metadata(archive_path)?.len();
        let corpus_bytes = std::fs::metadata(corpus_path)?.len();
        let (events, document_count, corpus_sha256) = load_events(corpus_path)?;
        let episodes = make_episodes(&events);
        let mut structural = StructuralSummary {
            event_count: events.len(),
            directional_episode_count: episodes.len(),
            ..StructuralSummary::default()
        };
        let mut document_pairs = std::collections::BTreeSet::new();
        let mut contested_document_pairs = std::collections::BTreeSet::new();
        for episode in episodes {
            document_pairs.insert((episode.nomination.doc, episode.witness.doc));
            let route = qualified_pair_route(
                episode.nomination.features,
                episode.witness.features,
                true,
                TiePolicy::HardAbstain,
            );
            let before = structural.unique_plurality_episode_count;
            structural.add(episode, route);
            if route.is_some()
                && (active_families(episode.nomination) != 1
                    || active_families(episode.witness) != 1)
            {
                let shard = shard(episode.nomination.doc, document_count);
                structural.contested_by_shard[shard] += 1;
                contested_document_pairs.insert((episode.nomination.doc, episode.witness.doc));
            }
            debug_assert!(
                structural.unique_plurality_episode_count == before
                    || structural.unique_plurality_episode_count == before + 1
            );
        }
        structural.physical_document_pair_count = document_pairs.len();
        structural.contested_physical_document_pair_count = contested_document_pairs.len();
        let role = if DISCOVERY_IDS.contains(&id) {
            "P1M2_DISCOVERY"
        } else {
            "P1M2Q_RESERVED"
        };
        println!(
            "{id}: docs={document_count} episodes={} unique={} exclusive={} contested={} self_vote_changed={}",
            structural.directional_episode_count,
            structural.unique_plurality_episode_count,
            structural.exclusive_unique_episode_count,
            structural.contested_unique_episode_count,
            structural.self_vote_changed_episode_count
        );
        corpora.push(CorpusScreen {
            corpus_id: id.to_owned(),
            assigned_role: role,
            corpus_path: corpus_path.display().to_string(),
            archive_path: archive_path.display().to_string(),
            archive_sha256,
            archive_bytes,
            corpus_sha256,
            corpus_bytes,
            document_count,
            structural,
        });
    }
    let preflight = structural_preflight(&corpora);
    let receipt = ScreenReceipt {
        schema: SCREEN_SCHEMA,
        date: DATE,
        scope: "prospective P1M2 cohort preflight; marker/router structure only; expected context outcomes and qrels remain unopened",
        corpus_text_only: true,
        validity_labels_opened: false,
        qrels_or_queries_opened: false,
        frozen_order: SCREEN_ORDER.to_vec(),
        discovery_ids: DISCOVERY_IDS.to_vec(),
        qualification_id: QUALIFICATION_ID,
        structural_preflight: preflight,
        corpora,
    };
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(output, serde_json::to_vec_pretty(&receipt)?)?;
    Ok(())
}
