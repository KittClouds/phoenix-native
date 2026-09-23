#![allow(dead_code)]

#[path = "lt9_la2p1n1_assay.rs"]
mod assay;
#[path = "lt9_la2p1m1_core.rs"]
mod core;

use anyhow::{bail, ensure, Context, Result};
use assay::{
    analyze_event, declared_family, merge_bank_sense, merge_stats, sha256_file, stream_census,
    BankSenseStats, CorpusReceipt, VariantStats,
};
use core::{Event, CANDIDATES};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

const SCREEN_SHA256: &str = "bc32d310b757e8919b45de9c058ebce59892de15da2c5f11527ee800d26b18a0";
const NORMALIZATION_SHA256: &str =
    "b8f87432ea9aa12edc545f7f4a07b3b9389628f76eeceae690aed1da559ad80b";
const SCREEN_SCHEMA: &str = "phoenix.lexical.lt9-la2-p1m2r-screen/v1";
const RESULT_SCHEMA: &str = "phoenix.lexical.lt9-la2-p1n1-result/v1";
const EXPECTED_ORDER: [&str; 12] = [
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

#[derive(Deserialize)]
struct Screen {
    schema: String,
    corpus_text_only: bool,
    validity_labels_opened: bool,
    qrels_or_queries_opened: bool,
    reserved_qualification_labels_opened: bool,
    frozen_order: Vec<String>,
    corpora: Vec<ScreenCorpus>,
}

#[derive(Deserialize)]
struct ScreenCorpus {
    corpus_id: String,
    corpus_path: String,
    corpus_sha256: String,
}

#[derive(Serialize)]
struct RunReceipt {
    schema: &'static str,
    date: &'static str,
    protocol_sha256: String,
    screen_sha256: String,
    normalization_receipt_sha256: String,
    binary_sha256: String,
    source_sha256: BTreeMap<String, String>,
    corpora: Vec<CorpusReceipt>,
    intervention_totals: BTreeMap<String, VariantStats>,
    paired_bank_sense: BankSenseStats,
    scope_flags: ScopeFlags,
}

#[derive(Serialize)]
struct ScopeFlags {
    expected_context_outcomes_read: bool,
    p1m2r_outcome_receipt_read: bool,
    qrels_or_queries_opened: bool,
    reserved_qualification_labels_opened: bool,
    router_selected: bool,
    retrieval_or_ranking_run: bool,
    prospective_corpus_selection: bool,
}

fn run(repo_root: &Path, screen_path: &Path, output_path: &Path) -> Result<()> {
    let protocol_path = repo_root.join("docs/LT9_LA2_P1N1_TOPIC_SENSE_INVARIANCE_20260923.md");
    let normalization_path =
        repo_root.join("experiments/lt9-la2-p1m2r/lotte-normalization-receipt-20260923.json");
    let protocol_sha256 = sha256_file(&protocol_path)?;
    let screen_sha256 = sha256_file(screen_path)?;
    ensure!(
        screen_sha256 == SCREEN_SHA256,
        "P1M2R screen SHA-256 differs from the frozen protocol"
    );
    let normalization_receipt_sha256 = sha256_file(&normalization_path)?;
    ensure!(
        normalization_receipt_sha256 == NORMALIZATION_SHA256,
        "P1M2R normalization receipt SHA-256 differs from the frozen protocol"
    );
    let screen: Screen =
        serde_json::from_slice(&fs::read(screen_path)?).context("decode label-blind screen")?;
    ensure!(screen.schema == SCREEN_SCHEMA, "unexpected screen schema");
    ensure!(screen.corpus_text_only, "screen is not corpus-text-only");
    ensure!(
        !screen.validity_labels_opened,
        "preflight screen opened labels"
    );
    ensure!(
        !screen.qrels_or_queries_opened,
        "preflight screen opened qrels/queries"
    );
    ensure!(
        !screen.reserved_qualification_labels_opened,
        "reserved qualification labels were opened in screen"
    );
    ensure!(
        screen
            .frozen_order
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            == EXPECTED_ORDER,
        "frozen corpus order mismatch"
    );
    ensure!(
        screen.corpora.len() == EXPECTED_ORDER.len(),
        "corpus count mismatch"
    );

    let binary_path = env::current_exe()?;
    let binary_sha256 = sha256_file(&binary_path)?;
    let source_sha256 = BTreeMap::from([
        (
            "lt9_la2p1n1.rs".to_string(),
            format!("{:x}", Sha256::digest(include_bytes!("lt9_la2p1n1.rs"))),
        ),
        (
            "lt9_la2p1m1_core.rs".to_string(),
            format!(
                "{:x}",
                Sha256::digest(include_bytes!("lt9_la2p1m1_core.rs"))
            ),
        ),
    ]);

    let mut corpora = Vec::with_capacity(screen.corpora.len());
    let mut intervention_totals = BTreeMap::<String, VariantStats>::new();
    let mut paired_bank_sense = BankSenseStats::default();
    for (index, corpus) in screen.corpora.iter().enumerate() {
        ensure!(
            corpus.corpus_id == EXPECTED_ORDER[index],
            "corpus order mismatch at {}",
            corpus.corpus_id
        );
        let corpus_path = Path::new(&corpus.corpus_path);
        let (events, document_count, corpus_sha256) =
            core::load_events(corpus_path).with_context(|| corpus.corpus_id.clone())?;
        ensure!(
            corpus_sha256 == corpus.corpus_sha256,
            "normalized corpus hash mismatch for {}",
            corpus.corpus_id
        );

        let mut streams = vec![Vec::<Event>::new(); CANDIDATES.len()];
        for event in &events {
            streams[event.candidate].push(*event);
        }
        let episodes = core::make_episodes(&events);
        let mut pair_routes = vec![Vec::<Option<usize>>::new(); CANDIDATES.len()];
        for episode in &episodes {
            pair_routes[episode.candidate].push(core::qualified_pair_route(
                episode.nomination.features,
                episode.witness.features,
                true,
                core::TiePolicy::HardAbstain,
            ));
        }

        let mut local_interventions = BTreeMap::new();
        let mut local_removal = BTreeMap::new();
        let mut local_bank_sense = BankSenseStats::default();
        let mut fixed_sense_seed_events = 0usize;
        for event in events.iter().copied() {
            if declared_family(CANDIDATES[event.candidate].id).is_some() {
                fixed_sense_seed_events += 1;
            }
            analyze_event(
                event,
                &mut local_interventions,
                &mut local_removal,
                &mut local_bank_sense,
            );
        }

        let census = streams
            .iter()
            .enumerate()
            .map(|(candidate, stream)| stream_census(candidate, stream, &pair_routes[candidate]))
            .collect::<Vec<_>>();
        for (name, stats) in &local_interventions {
            let total = intervention_totals.entry(name.clone()).or_default();
            merge_stats(total, stats);
        }
        merge_bank_sense(&mut paired_bank_sense, &local_bank_sense);
        corpora.push(CorpusReceipt {
            corpus_id: corpus.corpus_id.clone(),
            corpus_sha256,
            document_count,
            event_count: events.len(),
            fixed_sense_seed_events,
            stream_census: census,
            candidate_token_removal: local_removal,
            interventions: local_interventions,
            paired_bank_sense: local_bank_sense,
        });
    }

    let receipt = RunReceipt {
        schema: RESULT_SCHEMA,
        date: "2026-09-23",
        protocol_sha256,
        screen_sha256,
        normalization_receipt_sha256,
        binary_sha256,
        source_sha256,
        corpora,
        intervention_totals,
        paired_bank_sense,
        scope_flags: ScopeFlags {
            expected_context_outcomes_read: false,
            p1m2r_outcome_receipt_read: false,
            qrels_or_queries_opened: false,
            reserved_qualification_labels_opened: false,
            router_selected: false,
            retrieval_or_ranking_run: false,
            prospective_corpus_selection: false,
        },
    };
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file =
        File::create(output_path).with_context(|| format!("create {}", output_path.display()))?;
    serde_json::to_writer_pretty(&mut file, &receipt)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let Some(mode) = args.next() else {
        bail!("usage: lt9_la2p1n1 --run <repo-root> <frozen-screen.json> <output.json>");
    };
    ensure!(mode == "--run", "only --run is supported");
    let Some(repo_root) = args.next() else {
        bail!("missing repository root");
    };
    let Some(screen) = args.next() else {
        bail!("missing frozen screen path");
    };
    let Some(output) = args.next() else {
        bail!("missing output path");
    };
    ensure!(args.next().is_none(), "unexpected extra arguments");
    run(
        Path::new(&repo_root),
        Path::new(&screen),
        Path::new(&output),
    )
}
