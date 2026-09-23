//! Label-blind structural screen for the frozen P1M2R cohort.

#[allow(dead_code)]
mod lt9_la2p1m1_core;

use anyhow::{ensure, Context, Result};
use lt9_la2p1m1_core::*;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

const DATE: &str = "2026-09-23";
const SHARDS: usize = 8;
const DISCOVERY_IDS: [&str; 12] = [
    "cqadupstack",
    "arguana",
    "nfcorpus",
    "quora",
    "scidocs",
    "trec-covid",
    "scifact",
    "lotte-writing",
    "lotte-recreation",
    "lotte-science",
    "lotte-technology",
    "lotte-lifestyle",
];
const MIN_CONTESTED: usize = 240;
const MIN_CORPORA_WITH_15: usize = 3;
const MIN_RELATIONS: usize = 4;
const MIN_CORPUS_SHARDS: usize = 8;
const RESERVED_QUALIFICATION_ID: &str = "webis-touche2020";

fn shard(document: u64, documents: u64) -> usize {
    ((document.saturating_mul(SHARDS as u64)) / documents).min((SHARDS - 1) as u64) as usize
}

fn active_families(event: Event) -> usize {
    event
        .features
        .family
        .iter()
        .filter(|family| family.distinct_count() > 0)
        .count()
}

fn self_votes(candidate: usize, family: usize, marker: usize) -> u16 {
    let name = marker_names(family)[marker];
    let spec = CANDIDATES[candidate];
    u16::from(spec.source.eq_ignore_ascii_case(name))
        + u16::from(spec.target.eq_ignore_ascii_case(name))
}

fn context_only_counts(event: Event) -> [u16; 3] {
    std::array::from_fn(|family| {
        marker_names(family)
            .iter()
            .enumerate()
            .filter(|(marker, _)| {
                event.features.family[family].marker_occurrences[*marker]
                    > self_votes(event.candidate, family, *marker)
            })
            .count() as u16
    })
}

fn context_only_route(episode: Episode) -> Option<usize> {
    match (
        unique_winner(context_only_counts(episode.nomination)),
        unique_winner(context_only_counts(episode.witness)),
    ) {
        (Some(left), Some(right)) if left == right => Some(left),
        _ => None,
    }
}

fn unique_winner(counts: [u16; 3]) -> Option<usize> {
    let maximum = counts.iter().copied().max()?;
    if maximum == 0 {
        return None;
    }
    let mut winners = counts
        .iter()
        .enumerate()
        .filter(|(_, count)| **count == maximum);
    let winner = winners.next()?.0;
    winners.next().is_none().then_some(winner)
}

fn hash_file(path: &Path) -> Result<(String, u64)> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut reader = BufReader::with_capacity(1 << 20, file);
    let mut buffer = vec![0u8; 1 << 20];
    let mut hasher = Sha256::new();
    let mut bytes = 0u64;
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
        bytes += count as u64;
    }
    Ok((format!("{:x}", hasher.finalize()), bytes))
}

#[derive(Default, Serialize)]
struct RelationCounts {
    unique: usize,
    exclusive: usize,
    contested: usize,
}

#[derive(Default, Serialize)]
struct CorpusCounts {
    event_count: usize,
    directional_episode_count: usize,
    physical_document_pair_count: usize,
    unique_plurality_episode_count: usize,
    exclusive_unique_episode_count: usize,
    contested_unique_episode_count: usize,
    contested_physical_document_pair_count: usize,
    contested_by_shard: [usize; SHARDS],
    self_vote_changed_episode_count: usize,
    routed_by_relation: BTreeMap<String, RelationCounts>,
}

#[derive(Serialize)]
struct CorpusReceipt {
    corpus_id: String,
    role: &'static str,
    corpus_path: String,
    archive_path: String,
    archive_sha256: String,
    archive_bytes: u64,
    corpus_sha256: String,
    corpus_bytes: u64,
    document_count: u64,
    structural: CorpusCounts,
}

#[derive(Serialize)]
struct Gate {
    minimum_contested_episodes: usize,
    minimum_corpora_with_15_contested: usize,
    minimum_candidate_relations: usize,
    minimum_corpus_shard_cells: usize,
    observed_unique_episodes: usize,
    observed_contested_episodes: usize,
    observed_corpora_with_15_contested: usize,
    observed_candidate_relations: usize,
    observed_corpus_shard_cells: usize,
    passed: bool,
}

#[derive(Serialize)]
struct Receipt {
    schema: &'static str,
    date: &'static str,
    scope: &'static str,
    corpus_text_only: bool,
    validity_labels_opened: bool,
    qrels_or_queries_opened: bool,
    reserved_qualification_id: &'static str,
    reserved_qualification_labels_opened: bool,
    frozen_order: Vec<&'static str>,
    structural_preflight: Gate,
    corpora: Vec<CorpusReceipt>,
}

fn parse_spec(spec: &str) -> Result<(&str, &Path, &Path)> {
    let mut parts = spec.splitn(3, '=');
    Ok((
        parts.next().context("missing corpus ID")?,
        Path::new(parts.next().context("missing normalized corpus path")?),
        Path::new(parts.next().context("missing source archive path")?),
    ))
}

fn screen_one(
    corpus_id: &str,
    corpus_path: &Path,
    archive_path: &Path,
    archive_cache: &mut BTreeMap<PathBuf, (String, u64)>,
) -> Result<CorpusReceipt> {
    let (events, document_count, corpus_sha256) = load_events(corpus_path)?;
    let episodes = make_episodes(&events);
    let mut structural = CorpusCounts {
        event_count: events.len(),
        directional_episode_count: episodes.len(),
        ..CorpusCounts::default()
    };
    let mut physical_pairs = BTreeSet::new();
    let mut contested_physical_pairs = BTreeSet::new();

    for episode in episodes {
        let route = qualified_pair_route(
            episode.nomination.features,
            episode.witness.features,
            true,
            TiePolicy::HardAbstain,
        );
        let Some(route) = route else { continue };
        structural.unique_plurality_episode_count += 1;
        let nomination_active = active_families(episode.nomination);
        let witness_active = active_families(episode.witness);
        let contested = nomination_active != 1 || witness_active != 1;
        let relation = CANDIDATES[episode.candidate].id.to_owned();
        let per_relation = structural.routed_by_relation.entry(relation).or_default();
        per_relation.unique += 1;
        if contested {
            structural.contested_unique_episode_count += 1;
            per_relation.contested += 1;
            structural.contested_by_shard[shard(episode.nomination.doc, document_count)] += 1;
            contested_physical_pairs.insert((episode.nomination.doc, episode.witness.doc));
        } else {
            structural.exclusive_unique_episode_count += 1;
            per_relation.exclusive += 1;
        }
        if context_only_route(episode) != Some(route) {
            structural.self_vote_changed_episode_count += 1;
        }
        physical_pairs.insert((episode.nomination.doc, episode.witness.doc));
    }
    structural.physical_document_pair_count = physical_pairs.len();
    structural.contested_physical_document_pair_count = contested_physical_pairs.len();

    let archive_key = archive_path.to_path_buf();
    let (archive_sha256, archive_bytes) = if let Some(value) = archive_cache.get(&archive_key) {
        value.clone()
    } else {
        let value = hash_file(archive_path)?;
        archive_cache.insert(archive_key, value.clone());
        value
    };
    let corpus_bytes = std::fs::metadata(corpus_path)?.len();
    println!(
        "{corpus_id}: docs={document_count} episodes={} unique={} contested={} self_vote_changed={}",
        structural.directional_episode_count,
        structural.unique_plurality_episode_count,
        structural.contested_unique_episode_count,
        structural.self_vote_changed_episode_count,
    );
    Ok(CorpusReceipt {
        corpus_id: corpus_id.to_owned(),
        role: "P1M2R_DISCOVERY",
        corpus_path: corpus_path.display().to_string(),
        archive_path: archive_path.display().to_string(),
        archive_sha256,
        archive_bytes,
        corpus_sha256,
        corpus_bytes,
        document_count,
        structural,
    })
}

fn evaluate_gate(corpora: &[CorpusReceipt]) -> Gate {
    let mut unique = 0usize;
    let mut contested = 0usize;
    let mut corpora_with_15 = 0usize;
    let mut relations = BTreeSet::new();
    let mut corpus_shards = 0usize;
    for corpus in corpora {
        unique += corpus.structural.unique_plurality_episode_count;
        let count = corpus.structural.contested_unique_episode_count;
        contested += count;
        corpora_with_15 += usize::from(count >= 15);
        corpus_shards += corpus
            .structural
            .contested_by_shard
            .iter()
            .filter(|&&n| n > 0)
            .count();
        relations.extend(
            corpus
                .structural
                .routed_by_relation
                .iter()
                .filter(|(_, counts)| counts.contested > 0)
                .map(|(relation, _)| relation.clone()),
        );
    }
    let passed = contested >= MIN_CONTESTED
        && corpora_with_15 >= MIN_CORPORA_WITH_15
        && relations.len() >= MIN_RELATIONS
        && corpus_shards >= MIN_CORPUS_SHARDS;
    Gate {
        minimum_contested_episodes: MIN_CONTESTED,
        minimum_corpora_with_15_contested: MIN_CORPORA_WITH_15,
        minimum_candidate_relations: MIN_RELATIONS,
        minimum_corpus_shard_cells: MIN_CORPUS_SHARDS,
        observed_unique_episodes: unique,
        observed_contested_episodes: contested,
        observed_corpora_with_15_contested: corpora_with_15,
        observed_candidate_relations: relations.len(),
        observed_corpus_shard_cells: corpus_shards,
        passed,
    }
}

fn run(args: &[String]) -> Result<()> {
    ensure!(
        args.len() == DISCOVERY_IDS.len() + 2 && args[0] == "--screen",
        "usage: lt9_la2p1m2r_screen --screen <receipt.json> <id=corpus.jsonl=source-archive>... (frozen 12 inputs)"
    );
    let mut cache = BTreeMap::new();
    let mut corpora = Vec::with_capacity(DISCOVERY_IDS.len());
    for (index, spec) in args[2..].iter().enumerate() {
        let (id, corpus, archive) = parse_spec(spec)?;
        ensure!(
            id == DISCOVERY_IDS[index],
            "unexpected or reordered corpus {id}"
        );
        corpora.push(screen_one(id, corpus, archive, &mut cache)?);
    }
    let receipt = Receipt {
        schema: "phoenix.lexical.lt9-la2-p1m2r-screen/v1",
        date: DATE,
        scope: "prospective P1M2R structural preflight; frozen twelve-corpus cohort; expected-context labels and qrels remain unopened",
        corpus_text_only: true,
        validity_labels_opened: false,
        qrels_or_queries_opened: false,
        reserved_qualification_id: RESERVED_QUALIFICATION_ID,
        reserved_qualification_labels_opened: false,
        frozen_order: DISCOVERY_IDS.to_vec(),
        structural_preflight: evaluate_gate(&corpora),
        corpora,
    };
    let out = Path::new(&args[1]);
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out, serde_json::to_vec_pretty(&receipt)?)?;
    Ok(())
}

fn main() -> Result<()> {
    run(&std::env::args().skip(1).collect::<Vec<_>>())
}
