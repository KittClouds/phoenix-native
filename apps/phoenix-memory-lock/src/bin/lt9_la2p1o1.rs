//! P1O1: label-blind relation-variability intake and natural context-graph build.
//! This binary assigns no compatibility labels and performs no learning.

use anyhow::{Context, Result, ensure};
use core::*;
use serde::Serialize;
use std::env;
use std::fs;
use std::path::PathBuf;

#[path = "lt9_la2p1o1_core.rs"]
mod core;

#[derive(Serialize)]
struct RunReceipt {
    schema: &'static str,
    date: &'static str,
    status: &'static str,
    wordnet_archive_sha256: String,
    protocol_sha256: String,
    source_main_sha256: String,
    source_core_sha256: String,
    source_scan_sha256: String,
    harness_manifest_sha256: String,
    harness_lockfile_sha256: String,
    binary_sha256: String,
    corpus_roster_sha256: String,
    exclusion_list_sha256: String,
    p1n3_ledger_sha256: String,
    p1n4_ledger_sha256: String,
    corpus_hashes_verified: bool,
    corpora_scanned: Vec<CorpusScanReceipt>,
    prior_review_documents_excluded: usize,
    candidate_seed_pairs: usize,
    candidate_seed_pool_sha256: String,
    eligible_candidates: usize,
    candidates_examined_in_hash_order: usize,
    selected_candidates: usize,
    selected_pair_ids: Vec<String>,
    context_nodes: usize,
    graph_edges: usize,
    packets_sha256: String,
    rubric_sha256: String,
    judgments_template_sha256: String,
    private_ledger_sha256: String,
    candidate_intake_sha256: String,
    queries_or_qrels_read: bool,
    prior_judgments_read: bool,
    prior_feature_fields_read: bool,
    reserved_corpora_opened: bool,
    model_fit: bool,
    authority_updated: bool,
    retrieval_run: bool,
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    ensure!(
        args.len() == 10,
        "usage: lt9_la2p1o1 <wordnet-dict> <corpus-roster.json> <excluded-candidates.json> <p1n3-private-ledger.json> <p1n4-private-ledger.json> <output-dir> <wordnet-archive> <rubric.md> <protocol.md>"
    );
    let wordnet = PathBuf::from(&args[1]);
    let roster_path = PathBuf::from(&args[2]);
    let excluded_candidates_path = PathBuf::from(&args[3]);
    let p1n3_ledger = PathBuf::from(&args[4]);
    let p1n4_ledger = PathBuf::from(&args[5]);
    let output = PathBuf::from(&args[6]);
    let wordnet_archive = PathBuf::from(&args[7]);
    let rubric_source = PathBuf::from(&args[8]);
    let protocol_source = PathBuf::from(&args[9]);

    fs::create_dir_all(&output).context("create output directory")?;
    for name in [
        "candidate-seed-pool.json",
        "candidate-intake.json",
        "private-ledger.json",
        "pre-review-root.json",
        "acquisition-receipt.json",
    ] {
        ensure!(
            !output.join(name).exists(),
            "refusing to overwrite existing output {}",
            output.join(name).display()
        );
    }
    let blind = output.join("blind-review");
    fs::create_dir_all(&blind)?;
    for name in ["packets.json", "judgments-template.json", "rubric.md"] {
        ensure!(
            !blind.join(name).exists(),
            "refusing to overwrite reviewer file {}",
            blind.join(name).display()
        );
    }

    let roster_bytes = fs::read(&roster_path).context("read frozen corpus roster")?;
    let roster: CorpusRoster = serde_json::from_slice(&roster_bytes)?;
    ensure!(
        roster.corpora.len() == 13,
        "P1O1 roster must contain the frozen 12-corpus roster plus FiQA"
    );
    ensure!(
        roster.corpora.iter().any(|c| c.corpus_id == "fiqa"),
        "FiQA corpus-only source missing"
    );
    ensure!(
        !roster
            .corpora
            .iter()
            .any(|c| ["webis-touche2020", "nq", "hotpotqa"].contains(&c.corpus_id.as_str())),
        "reserved corpus present in discovery roster"
    );

    let excluded_bytes = fs::read(&excluded_candidates_path)?;
    let excluded_candidates: CandidateExclusions = serde_json::from_slice(&excluded_bytes)?;
    let excluded_lemmas: hashbrown::HashSet<String> =
        excluded_candidates.excluded_lemmas.into_iter().collect();
    let pool = wordnet_candidate_pool(&wordnet, &excluded_lemmas)?;
    ensure!(!pool.is_empty(), "WordNet proposal pool is empty");

    let excluded_docs = load_reviewed_document_hashes(&[p1n3_ledger.clone(), p1n4_ledger.clone()])?;
    let mut term_map = hashbrown::HashMap::<String, usize>::new();
    for candidate in &pool {
        let next = term_map.len();
        term_map.entry(candidate.lemma_a.clone()).or_insert(next);
        let next = term_map.len();
        term_map.entry(candidate.lemma_b.clone()).or_insert(next);
    }
    let mut contexts = vec![vec![Vec::<Occurrence>::new(); roster.corpora.len()]; term_map.len()];
    let mut scan_receipts = Vec::with_capacity(roster.corpora.len());
    for (corpus_index, corpus) in roster.corpora.iter().enumerate() {
        let receipt = scan_corpus(
            corpus,
            corpus_index,
            &term_map,
            &excluded_docs,
            &mut contexts,
        )?;
        scan_receipts.push(receipt);
    }

    let selection = select_candidate_graphs(&pool, &term_map, &contexts);
    let chosen = &selection.chosen;
    write_json(&output.join("candidate-seed-pool.json"), &pool)?;
    let public_packets = materialize_packets(chosen);
    let private_rows = materialize_private_rows(chosen);
    let intake = make_intake_report(&pool, &selection, &term_map, &contexts);
    let packets_path = blind.join("packets.json");
    let ledger_path = output.join("private-ledger.json");
    let template_path = blind.join("judgments-template.json");
    let rubric_path = blind.join("rubric.md");
    let intake_path = output.join("candidate-intake.json");
    write_json(&packets_path, &public_packets)?;
    write_json(&ledger_path, &private_rows)?;
    let template: Vec<JudgmentRow> = public_packets
        .iter()
        .map(|p| JudgmentRow {
            packet_id: p.packet_id.clone(),
            judgment: None::<String>,
        })
        .collect();
    write_json(&template_path, &template)?;
    fs::copy(&rubric_source, &rubric_path)?;
    write_json(&intake_path, &intake)?;

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let executable = env::current_exe().context("locate running executable")?;
    let selected_ids: Vec<String> = chosen
        .iter()
        .map(|c| c.candidate.candidate_id.clone())
        .collect();
    let receipt = RunReceipt {
        schema: "phoenix.lexical.lt9-la2-p1o1-acquisition/v1",
        date: DATE,
        status: if chosen.len() == TARGET_CANDIDATES {
            "BLIND_PACKETS_READY"
        } else {
            "BLIND_PACKETS_READY_WITH_COHORT_SHORTFALL"
        },
        wordnet_archive_sha256: hash_file(&wordnet_archive)?,
        protocol_sha256: hash_file(&protocol_source)?,
        source_main_sha256: hash_file(&manifest_dir.join("src/main.rs"))?,
        source_core_sha256: hash_file(&manifest_dir.join("src/lt9_la2p1o1_core.rs"))?,
        source_scan_sha256: hash_file(&manifest_dir.join("src/lt9_la2p1o1_scan.rs"))?,
        harness_manifest_sha256: hash_file(&manifest_dir.join("Cargo.toml"))?,
        harness_lockfile_sha256: hash_file(&manifest_dir.join("Cargo.lock"))?,
        binary_sha256: hash_file(&executable)?,
        corpus_roster_sha256: sha256(&roster_bytes),
        exclusion_list_sha256: sha256(&excluded_bytes),
        p1n3_ledger_sha256: hash_file(&p1n3_ledger)?,
        p1n4_ledger_sha256: hash_file(&p1n4_ledger)?,
        corpus_hashes_verified: scan_receipts.iter().all(|r| r.hash_verified),
        corpora_scanned: scan_receipts,
        prior_review_documents_excluded: excluded_docs.len(),
        candidate_seed_pairs: pool.len(),
        candidate_seed_pool_sha256: hash_file(&output.join("candidate-seed-pool.json"))?,
        eligible_candidates: selection.eligible_candidates,
        candidates_examined_in_hash_order: selection.candidates_examined,
        selected_candidates: chosen.len(),
        selected_pair_ids: selected_ids,
        context_nodes: chosen.len() * CONTEXTS_PER_CANDIDATE,
        graph_edges: public_packets.len(),
        packets_sha256: hash_file(&packets_path)?,
        rubric_sha256: hash_file(&rubric_path)?,
        judgments_template_sha256: hash_file(&template_path)?,
        private_ledger_sha256: hash_file(&ledger_path)?,
        candidate_intake_sha256: hash_file(&intake_path)?,
        queries_or_qrels_read: false,
        prior_judgments_read: false,
        prior_feature_fields_read: false,
        reserved_corpora_opened: false,
        model_fit: false,
        authority_updated: false,
        retrieval_run: false,
    };
    write_json(&output.join("acquisition-receipt.json"), &receipt)?;

    let root = PreReviewRoot {
        schema: "phoenix.lexical.lt9-la2-p1o1-pre-review-root/v1",
        status: receipt.status,
        wordnet_archive_sha256: receipt.wordnet_archive_sha256.clone(),
        protocol_sha256: receipt.protocol_sha256.clone(),
        source_main_sha256: receipt.source_main_sha256.clone(),
        source_core_sha256: receipt.source_core_sha256.clone(),
        source_scan_sha256: receipt.source_scan_sha256.clone(),
        harness_manifest_sha256: receipt.harness_manifest_sha256.clone(),
        harness_lockfile_sha256: receipt.harness_lockfile_sha256.clone(),
        binary_sha256: receipt.binary_sha256.clone(),
        corpus_roster_sha256: receipt.corpus_roster_sha256.clone(),
        exclusion_list_sha256: receipt.exclusion_list_sha256.clone(),
        packets_sha256: receipt.packets_sha256.clone(),
        rubric_sha256: receipt.rubric_sha256.clone(),
        judgments_template_sha256: receipt.judgments_template_sha256.clone(),
        private_ledger_sha256: receipt.private_ledger_sha256.clone(),
        candidate_seed_pool_sha256: receipt.candidate_seed_pool_sha256.clone(),
        candidate_intake_sha256: receipt.candidate_intake_sha256.clone(),
        acquisition_receipt_sha256: hash_file(&output.join("acquisition-receipt.json"))?,
        reviewer_receives_sampling_metadata: false,
        labels_assigned: false,
        feature_fit_authorized: false,
        memory_or_retrieval_authorized: false,
    };
    write_json(&output.join("pre-review-root.json"), &root)?;
    println!(
        "{} candidates, {} packets -> {}",
        chosen.len(),
        public_packets.len(),
        output.display()
    );
    Ok(())
}

#[derive(Serialize)]
struct JudgmentRow {
    packet_id: String,
    judgment: Option<String>,
}

#[derive(Serialize)]
struct PreReviewRoot {
    schema: &'static str,
    status: &'static str,
    wordnet_archive_sha256: String,
    protocol_sha256: String,
    source_main_sha256: String,
    source_core_sha256: String,
    source_scan_sha256: String,
    harness_manifest_sha256: String,
    harness_lockfile_sha256: String,
    binary_sha256: String,
    corpus_roster_sha256: String,
    exclusion_list_sha256: String,
    packets_sha256: String,
    rubric_sha256: String,
    judgments_template_sha256: String,
    private_ledger_sha256: String,
    candidate_seed_pool_sha256: String,
    candidate_intake_sha256: String,
    acquisition_receipt_sha256: String,
    reviewer_receives_sampling_metadata: bool,
    labels_assigned: bool,
    feature_fit_authorized: bool,
    memory_or_retrieval_authorized: bool,
}
