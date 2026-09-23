use crate::lt9_la2p1m1_core::*;
use crate::{route_class, shard, RouteClass, SCREEN_SCHEMA, SHARDS};
use anyhow::{ensure, Context, Result};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

pub const SCREEN_ORDER: [&str; 3] = ["cqadupstack", "climate-fever", "nq"];
pub const MIN_UNIQUE: usize = 40;
pub const MIN_CONTESTED: usize = 20;
pub const MIN_EXCLUSIVE: usize = 20;
pub const MIN_CONTESTED_PER_RELATION: usize = 10;
pub const MIN_EXCLUSIVE_PER_RELATION: usize = 5;
pub const MIN_RELATIONS: usize = 2;
pub const MIN_SHARDS: usize = 3;

pub fn validate_frozen_screen(receipt: &Value) -> Result<()> {
    ensure!(
        receipt["frozen_order"] == serde_json::json!(SCREEN_ORDER),
        "screen corpus order differs from frozen P1M1 protocol"
    );
    for (field, expected) in [
        ("min_unique_plurality", MIN_UNIQUE),
        ("min_contested_unique", MIN_CONTESTED),
        ("min_exclusive_unique", MIN_EXCLUSIVE),
        ("min_contested_per_relation", MIN_CONTESTED_PER_RELATION),
        ("min_exclusive_per_relation", MIN_EXCLUSIVE_PER_RELATION),
        ("min_relations", MIN_RELATIONS),
        ("min_document_shards", MIN_SHARDS),
    ] {
        ensure!(
            receipt[field].as_u64() == Some(expected as u64),
            "screen floor {field} differs from frozen P1M1 protocol"
        );
    }
    Ok(())
}

#[derive(Default, Serialize)]
struct RelationStructure {
    unique: usize,
    exclusive: usize,
    contested: usize,
}

#[derive(Default, Serialize)]
struct StructuralCounts {
    unique: usize,
    exclusive: usize,
    contested: usize,
    unique_by_shard: [usize; SHARDS],
    exclusive_by_shard: [usize; SHARDS],
    contested_by_shard: [usize; SHARDS],
    by_candidate: BTreeMap<String, RelationStructure>,
}

impl StructuralCounts {
    fn add(&mut self, class: RouteClass, candidate: &str, shard: usize) {
        self.unique += 1;
        self.unique_by_shard[shard] += 1;
        let relation = self.by_candidate.entry(candidate.to_owned()).or_default();
        relation.unique += 1;
        match class {
            RouteClass::ExclusiveUnique => {
                self.exclusive += 1;
                self.exclusive_by_shard[shard] += 1;
                relation.exclusive += 1;
            }
            RouteClass::ContestedUnique => {
                self.contested += 1;
                self.contested_by_shard[shard] += 1;
                relation.contested += 1;
            }
        }
    }

    fn eligible(&self) -> bool {
        let contested_relations = self
            .by_candidate
            .values()
            .filter(|r| r.contested >= MIN_CONTESTED_PER_RELATION)
            .count();
        let exclusive_relations = self
            .by_candidate
            .values()
            .filter(|r| r.exclusive >= MIN_EXCLUSIVE_PER_RELATION)
            .count();
        let unique_shards = self.unique_by_shard.iter().filter(|&&n| n > 0).count();
        let contested_shards = self.contested_by_shard.iter().filter(|&&n| n > 0).count();
        self.unique >= MIN_UNIQUE
            && self.contested >= MIN_CONTESTED
            && self.exclusive >= MIN_EXCLUSIVE
            && contested_relations >= MIN_RELATIONS
            && exclusive_relations >= MIN_RELATIONS
            && unique_shards >= MIN_SHARDS
            && contested_shards >= MIN_SHARDS
    }
}

#[derive(Serialize)]
struct ScreenRow {
    corpus_id: String,
    corpus_path: String,
    corpus_sha256: String,
    document_count: u64,
    event_count: usize,
    episode_count: usize,
    structural: StructuralCounts,
    eligible: bool,
    assigned_role: &'static str,
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
    min_unique_plurality: usize,
    min_contested_unique: usize,
    min_exclusive_unique: usize,
    min_contested_per_relation: usize,
    min_exclusive_per_relation: usize,
    min_relations: usize,
    min_document_shards: usize,
    discovery_corpus: Option<String>,
    qualification_corpus: Option<String>,
    corpora: Vec<ScreenRow>,
    conclusion: &'static str,
}

fn screen_corpus(events: &[Event], documents: u64) -> StructuralCounts {
    let mut counts = StructuralCounts::default();
    for episode in make_episodes(events) {
        if qualified_pair_route(
            episode.nomination.features,
            episode.witness.features,
            true,
            TiePolicy::HardAbstain,
        )
        .is_none()
        {
            continue;
        }
        counts.add(
            route_class(episode),
            CANDIDATES[episode.candidate].id,
            shard(episode.nomination.doc, documents),
        );
    }
    counts
}

pub fn run_screen(args: &[String]) -> Result<()> {
    ensure!(
        args.len() >= 2,
        "usage: --screen <receipt.json> <id=corpus.jsonl>..."
    );
    let output = Path::new(&args[0]);
    let mut rows = Vec::with_capacity(args.len() - 1);
    let mut discovery = None;
    let mut qualification = None;
    for (index, spec) in args[1..].iter().enumerate() {
        ensure!(index < SCREEN_ORDER.len(), "P1M1 corpus order exhausted");
        ensure!(qualification.is_none(), "stop after two eligible corpora");
        let (id, path) = spec.split_once('=').context("expected id=corpus.jsonl")?;
        ensure!(id == SCREEN_ORDER[index], "{id} violates frozen order");
        let corpus = Path::new(path);
        let (events, documents, hash) = load_events(corpus)?;
        let episodes = make_episodes(&events);
        let structural = screen_corpus(&events, documents);
        let eligible = structural.eligible();
        let role = if eligible && discovery.is_none() {
            discovery = Some(id.to_owned());
            "P1M1_DISCOVERY"
        } else if eligible && qualification.is_none() {
            qualification = Some(id.to_owned());
            "P1M1Q_QUALIFICATION"
        } else {
            "NOT_ASSIGNED"
        };
        println!(
            "{id}: docs={documents} episodes={} unique={} exclusive={} contested={} eligible={eligible} role={role}",
            episodes.len(), structural.unique, structural.exclusive, structural.contested
        );
        rows.push(ScreenRow {
            corpus_id: id.to_owned(),
            corpus_path: corpus.display().to_string(),
            corpus_sha256: hash,
            document_count: documents,
            event_count: events.len(),
            episode_count: episodes.len(),
            structural,
            eligible,
            assigned_role: role,
        });
    }
    let conclusion = if qualification.is_some() {
        "TWO_FRESH_CORPORA_PREASSIGNED_LABEL_BLIND"
    } else {
        "UNDERPOWERED_SCREEN; continue frozen order before opening outcomes"
    };
    let receipt = ScreenReceipt {
        schema: SCREEN_SCHEMA,
        date: "2026-09-23",
        scope: "P1M1 label-blind unique-plurality and exclusive/contested structural coverage",
        corpus_text_only: true,
        validity_labels_opened: false,
        qrels_or_queries_opened: false,
        frozen_order: SCREEN_ORDER.to_vec(),
        min_unique_plurality: MIN_UNIQUE,
        min_contested_unique: MIN_CONTESTED,
        min_exclusive_unique: MIN_EXCLUSIVE,
        min_contested_per_relation: MIN_CONTESTED_PER_RELATION,
        min_exclusive_per_relation: MIN_EXCLUSIVE_PER_RELATION,
        min_relations: MIN_RELATIONS,
        min_document_shards: MIN_SHARDS,
        discovery_corpus: discovery,
        qualification_corpus: qualification,
        corpora: rows,
        conclusion,
    };
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(output, serde_json::to_vec_pretty(&receipt)?)?;
    Ok(())
}
