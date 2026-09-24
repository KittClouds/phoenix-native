//! P1O2: fresh natural occurrence-context graphs for P1O1 mixed relations.
//! This intake assigns no labels, fits no observer, and grants no authority.

use anyhow::{ensure, Context, Result};
use hashbrown::{HashMap, HashSet};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[path = "lt9_la2p1o2_features.rs"]
mod features;
#[path = "lt9_la2p1o1_core.rs"]
#[allow(dead_code)]
mod p1o1;

use features::{context_features, jaccard, pair_features, ContextFeatures, PairFeatures};

const DATE: &str = "2026-09-23";
const CONTEXTS_PER_GRAPH: usize = 6;
const EDGES_PER_GRAPH: usize = 15;
const SALT: &[u8] = b"phoenix-lt9-la2-p1o2-natural-compatibility-20260923-v1";

#[derive(Deserialize)]
struct Cohort {
    candidates: Vec<Candidate>,
}

#[derive(Clone, Deserialize, Serialize)]
struct Candidate {
    candidate_id: String,
    lemma_a: String,
    lemma_b: String,
}

#[derive(Serialize)]
struct PublicPacket {
    packet_id: String,
    lexical_pair: [String; 2],
    left_context: String,
    right_context: String,
    judgment: Option<String>,
}

#[derive(Serialize)]
struct JudgmentRow {
    packet_id: String,
    judgment: Option<String>,
}

#[derive(Serialize)]
struct PrivateEdge {
    packet_id: String,
    candidate_id: String,
    lexical_pair: [String; 2],
    split: String,
    overlap_band: String,
    context_jaccard: f64,
    left_occurrence: p1o1::Occurrence,
    right_occurrence: p1o1::Occurrence,
    left_features: ContextFeatures,
    right_features: ContextFeatures,
    pair_features: PairFeatures,
}

#[derive(Serialize)]
struct CandidateReceipt {
    candidate_id: String,
    lexical_pair: [String; 2],
    status: String,
    selected_contexts: usize,
    fit_contexts: usize,
    holdout_contexts: usize,
    fit_corpus_count: usize,
    holdout_corpus_count: usize,
    fit_token_jaccard_iqr: Option<f64>,
    holdout_token_jaccard_iqr: Option<f64>,
    reason: Option<String>,
}

#[derive(Serialize)]
struct AcquisitionReceipt {
    schema: &'static str,
    date: &'static str,
    status: &'static str,
    protocol_sha256: String,
    rubric_sha256: String,
    source_main_sha256: String,
    source_p1o1_core_sha256: String,
    source_p1o1_scan_sha256: String,
    source_features_sha256: String,
    harness_manifest_sha256: String,
    harness_lockfile_sha256: String,
    binary_sha256: String,
    roster_sha256: String,
    cohort_sha256: String,
    p1n3_ledger_sha256: String,
    p1n4_ledger_sha256: String,
    p1o1_ledger_sha256: String,
    excluded_doc_hash_count: usize,
    excluded_doc_hashes_sha256: String,
    corpus_hashes_verified: bool,
    corpus_scans: Vec<p1o1::CorpusScanReceipt>,
    candidates: Vec<CandidateReceipt>,
    packet_count: usize,
    packets_sha256: String,
    judgments_template_sha256: String,
    private_ledger_sha256: String,
    queries_or_qrels_read: bool,
    prior_judgments_read: bool,
    feature_fit: bool,
    authority_updated: bool,
    retrieval_run: bool,
}

#[derive(Serialize)]
struct PreReviewRoot {
    schema: &'static str,
    status: &'static str,
    acquisition_receipt_sha256: String,
    protocol_sha256: String,
    rubric_sha256: String,
    source_main_sha256: String,
    source_p1o1_core_sha256: String,
    source_p1o1_scan_sha256: String,
    source_features_sha256: String,
    harness_manifest_sha256: String,
    harness_lockfile_sha256: String,
    binary_sha256: String,
    roster_sha256: String,
    cohort_sha256: String,
    p1n3_ledger_sha256: String,
    p1n4_ledger_sha256: String,
    p1o1_ledger_sha256: String,
    excluded_doc_hashes_sha256: String,
    packets_sha256: String,
    judgments_template_sha256: String,
    private_ledger_sha256: String,
    reviewer_receives_split_or_sampling_metadata: bool,
    judgments_completed: bool,
    labels_assigned: bool,
    feature_fit_authorized: bool,
    authority_or_retrieval_authorized: bool,
}

#[derive(Deserialize)]
struct CorpusRoster {
    corpora: Vec<p1o1::CorpusSpec>,
}

#[derive(Clone)]
struct SelectedGraph {
    contexts: Vec<p1o1::Occurrence>,
    features: Vec<ContextFeatures>,
    corpus_count: usize,
    iqr: f64,
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    ensure!(
        args.len() == 10,
        "usage: lt9_la2p1o2 <roster.json> <candidate-cohort.json> <p1n3-ledger.json> <p1n4-ledger.json> <p1o1-ledger.json> <output-dir> <rubric.md> <protocol.md> <harness-dir>"
    );
    let roster_path = PathBuf::from(&args[1]);
    let cohort_path = PathBuf::from(&args[2]);
    let p1n3_path = PathBuf::from(&args[3]);
    let p1n4_path = PathBuf::from(&args[4]);
    let p1o1_path = PathBuf::from(&args[5]);
    let output = PathBuf::from(&args[6]);
    let rubric_path = PathBuf::from(&args[7]);
    let protocol_path = PathBuf::from(&args[8]);
    let harness_dir = PathBuf::from(&args[9]);

    ensure!(
        !output.exists(),
        "refusing to overwrite output directory {}",
        output.display()
    );
    fs::create_dir_all(output.join("blind-review"))?;
    let roster_bytes = fs::read(&roster_path)?;
    let roster: CorpusRoster = serde_json::from_slice(&roster_bytes)?;
    ensure!(
        roster.corpora.len() == 13,
        "expected frozen 13-corpus P1O1 roster"
    );
    ensure!(
        !roster
            .corpora
            .iter()
            .any(|c| ["webis-touche2020", "nq", "hotpotqa"].contains(&c.corpus_id.as_str())),
        "reserved qualification corpus appears in roster"
    );
    let cohort_bytes = fs::read(&cohort_path)?;
    let cohort: Cohort = serde_json::from_slice(&cohort_bytes)?;
    ensure!(
        cohort.candidates.len() == 5,
        "frozen P1O2 cohort must contain exactly five P1O1 MIXED candidates"
    );
    let mut candidate_ids = HashSet::with_capacity(cohort.candidates.len());
    let mut terms = Vec::<String>::with_capacity(cohort.candidates.len() * 2);
    for candidate in &cohort.candidates {
        ensure!(
            candidate_ids.insert(candidate.candidate_id.clone()),
            "duplicate candidate ID"
        );
        ensure!(
            candidate.lemma_a != candidate.lemma_b,
            "candidate lemmas must differ"
        );
        for term in [&candidate.lemma_a, &candidate.lemma_b] {
            if !terms.contains(term) {
                terms.push(term.clone());
            }
        }
    }
    let mut term_map = HashMap::with_capacity(terms.len());
    for (index, term) in terms.iter().enumerate() {
        term_map.insert(term.clone(), index);
    }

    let mut excluded = HashSet::new();
    for path in [&p1n3_path, &p1n4_path, &p1o1_path] {
        collect_document_hashes(&serde_json::from_slice(&fs::read(path)?)?, &mut excluded);
    }
    let mut contexts =
        vec![vec![Vec::<p1o1::Occurrence>::new(); roster.corpora.len()]; terms.len()];
    let mut scans = Vec::with_capacity(roster.corpora.len());
    for (index, corpus) in roster.corpora.iter().enumerate() {
        scans.push(
            p1o1::scan_corpus(corpus, index, &term_map, &excluded, &mut contexts)
                .with_context(|| format!("scan corpus {}", corpus.corpus_id))?,
        );
    }

    let mut public = Vec::<PublicPacket>::new();
    let mut private = Vec::<PrivateEdge>::new();
    let mut candidate_receipts = Vec::with_capacity(cohort.candidates.len());
    for candidate in &cohort.candidates {
        let base_candidate = p1o1::CandidatePair {
            candidate_id: candidate.candidate_id.clone(),
            lemma_a: candidate.lemma_a.clone(),
            lemma_b: candidate.lemma_b.clone(),
            priority_sha256: salted_hash(&[
                candidate.candidate_id.as_bytes(),
                candidate.lemma_a.as_bytes(),
                candidate.lemma_b.as_bytes(),
            ]),
        };
        let first = p1o1::select_candidate_graphs(
            std::slice::from_ref(&base_candidate),
            &term_map,
            &contexts,
        )
        .chosen
        .into_iter()
        .next();
        let Some(first) = first else {
            candidate_receipts.push(underpowered(
                candidate,
                "no fresh context graph met the frozen diversity requirements",
            ));
            continue;
        };
        let used_docs: HashSet<String> = first
            .contexts
            .iter()
            .map(|occurrence| occurrence.document_sha256.clone())
            .collect();
        let mut remaining_contexts = contexts.clone();
        for per_corpus in remaining_contexts.iter_mut().flatten() {
            per_corpus.retain(|occurrence| !used_docs.contains(&occurrence.document_sha256));
        }
        let second = p1o1::select_candidate_graphs(
            std::slice::from_ref(&base_candidate),
            &term_map,
            &remaining_contexts,
        )
        .chosen
        .into_iter()
        .next();
        let Some(second) = second else {
            candidate_receipts.push(underpowered(
                candidate,
                "no document-disjoint second graph met the frozen diversity requirements",
            ));
            continue;
        };
        let graphs = [
            selected_graph(first, candidate),
            selected_graph(second, candidate),
        ];
        for (graph_index, graph) in graphs.iter().cloned().enumerate() {
            let split = if graph_index == 0 { "fit" } else { "holdout" };
            materialize_graph(candidate, split, graph, &mut public, &mut private);
        }
        candidate_receipts.push(CandidateReceipt {
            candidate_id: candidate.candidate_id.clone(),
            lexical_pair: [candidate.lemma_a.clone(), candidate.lemma_b.clone()],
            status: "PACKETS_READY".to_owned(),
            selected_contexts: 12,
            fit_contexts: 6,
            holdout_contexts: 6,
            fit_corpus_count: graphs[0].corpus_count,
            holdout_corpus_count: graphs[1].corpus_count,
            fit_token_jaccard_iqr: Some(graphs[0].iqr),
            holdout_token_jaccard_iqr: Some(graphs[1].iqr),
            reason: None,
        });
    }

    let blind_dir = output.join("blind-review");
    let packet_path = blind_dir.join("packets.json");
    let template_path = blind_dir.join("judgments-template.json");
    let rubric_out = blind_dir.join("rubric.md");
    p1o1::write_json(&packet_path, &public)?;
    let template: Vec<JudgmentRow> = public
        .iter()
        .map(|p| JudgmentRow {
            packet_id: p.packet_id.clone(),
            judgment: None,
        })
        .collect();
    p1o1::write_json(&template_path, &template)?;
    fs::copy(&rubric_path, &rubric_out)?;
    p1o1::write_json(&output.join("private-ledger.json"), &private)?;
    let mut excluded_sorted = excluded.iter().cloned().collect::<Vec<_>>();
    excluded_sorted.sort();
    p1o1::write_json(
        &output.join("excluded-document-hashes.json"),
        &excluded_sorted,
    )?;

    let manifest_dir = harness_dir.as_path();
    let main_source = manifest_dir.join("../../apps/phoenix-memory-lock/src/bin/lt9_la2p1o2.rs");
    let core_source =
        manifest_dir.join("../../apps/phoenix-memory-lock/src/bin/lt9_la2p1o1_core.rs");
    let scan_source =
        manifest_dir.join("../../apps/phoenix-memory-lock/src/bin/lt9_la2p1o1_scan.rs");
    let feature_source =
        manifest_dir.join("../../apps/phoenix-memory-lock/src/bin/lt9_la2p1o2_features.rs");
    let executable = env::current_exe()?;
    let corpus_hashes_verified = scans.iter().all(|r| r.hash_verified);
    ensure!(corpus_hashes_verified, "a corpus hash failed verification");
    let ready_candidates = candidate_receipts
        .iter()
        .filter(|r| r.status == "PACKETS_READY")
        .count();
    let receipt = AcquisitionReceipt {
        schema: "phoenix.lexical.lt9-la2-p1o2-acquisition/v1",
        date: DATE,
        status: if ready_candidates == candidate_receipts.len() {
            "BLIND_PACKETS_READY"
        } else {
            "BLIND_PACKETS_READY_WITH_CANDIDATE_SHORTFALL"
        },
        protocol_sha256: hash_file(&protocol_path)?,
        rubric_sha256: hash_file(&rubric_path)?,
        source_main_sha256: hash_file(&main_source)?,
        source_p1o1_core_sha256: hash_file(&core_source)?,
        source_p1o1_scan_sha256: hash_file(&scan_source)?,
        source_features_sha256: hash_file(&feature_source)?,
        harness_manifest_sha256: hash_file(&manifest_dir.join("Cargo.toml"))?,
        harness_lockfile_sha256: hash_file(&manifest_dir.join("Cargo.lock"))?,
        binary_sha256: hash_file(&executable)?,
        roster_sha256: sha256(&roster_bytes),
        cohort_sha256: sha256(&cohort_bytes),
        p1n3_ledger_sha256: hash_file(&p1n3_path)?,
        p1n4_ledger_sha256: hash_file(&p1n4_path)?,
        p1o1_ledger_sha256: hash_file(&p1o1_path)?,
        excluded_doc_hash_count: excluded.len(),
        excluded_doc_hashes_sha256: hash_file(&output.join("excluded-document-hashes.json"))?,
        corpus_hashes_verified,
        corpus_scans: scans,
        candidates: candidate_receipts,
        packet_count: public.len(),
        packets_sha256: hash_file(&packet_path)?,
        judgments_template_sha256: hash_file(&template_path)?,
        private_ledger_sha256: hash_file(&output.join("private-ledger.json"))?,
        queries_or_qrels_read: false,
        prior_judgments_read: false,
        feature_fit: false,
        authority_updated: false,
        retrieval_run: false,
    };
    p1o1::write_json(&output.join("acquisition-receipt.json"), &receipt)?;
    let root = PreReviewRoot {
        schema: "phoenix.lexical.lt9-la2-p1o2-pre-review-root/v1",
        status: receipt.status,
        acquisition_receipt_sha256: hash_file(&output.join("acquisition-receipt.json"))?,
        protocol_sha256: receipt.protocol_sha256.clone(),
        rubric_sha256: receipt.rubric_sha256.clone(),
        source_main_sha256: receipt.source_main_sha256.clone(),
        source_p1o1_core_sha256: receipt.source_p1o1_core_sha256.clone(),
        source_p1o1_scan_sha256: receipt.source_p1o1_scan_sha256.clone(),
        source_features_sha256: receipt.source_features_sha256.clone(),
        harness_manifest_sha256: receipt.harness_manifest_sha256.clone(),
        harness_lockfile_sha256: receipt.harness_lockfile_sha256.clone(),
        binary_sha256: receipt.binary_sha256.clone(),
        roster_sha256: receipt.roster_sha256.clone(),
        cohort_sha256: receipt.cohort_sha256.clone(),
        p1n3_ledger_sha256: receipt.p1n3_ledger_sha256.clone(),
        p1n4_ledger_sha256: receipt.p1n4_ledger_sha256.clone(),
        p1o1_ledger_sha256: receipt.p1o1_ledger_sha256.clone(),
        excluded_doc_hashes_sha256: receipt.excluded_doc_hashes_sha256.clone(),
        packets_sha256: receipt.packets_sha256.clone(),
        judgments_template_sha256: receipt.judgments_template_sha256.clone(),
        private_ledger_sha256: receipt.private_ledger_sha256.clone(),
        reviewer_receives_split_or_sampling_metadata: false,
        judgments_completed: false,
        labels_assigned: false,
        feature_fit_authorized: false,
        authority_or_retrieval_authorized: false,
    };
    p1o1::write_json(&output.join("pre-review-root.json"), &root)?;
    println!(
        "{} candidate relations, {} blind pairs -> {}",
        ready_candidates,
        public.len(),
        output.display()
    );
    Ok(())
}

fn underpowered(candidate: &Candidate, reason: &str) -> CandidateReceipt {
    CandidateReceipt {
        candidate_id: candidate.candidate_id.clone(),
        lexical_pair: [candidate.lemma_a.clone(), candidate.lemma_b.clone()],
        status: "UNDERPOWERED".to_owned(),
        selected_contexts: 0,
        fit_contexts: 0,
        holdout_contexts: 0,
        fit_corpus_count: 0,
        holdout_corpus_count: 0,
        fit_token_jaccard_iqr: None,
        holdout_token_jaccard_iqr: None,
        reason: Some(reason.to_owned()),
    }
}

fn collect_document_hashes(value: &Value, out: &mut HashSet<String>) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                if matches!(
                    key.as_str(),
                    "left_document_sha256" | "right_document_sha256" | "document_sha256"
                ) {
                    if let Some(hash) = child.as_str() {
                        out.insert(hash.to_owned());
                    }
                }
                collect_document_hashes(child, out);
            }
        }
        Value::Array(items) => items
            .iter()
            .for_each(|item| collect_document_hashes(item, out)),
        _ => {}
    }
}

fn selected_graph(chosen: p1o1::ChosenCandidate, candidate: &Candidate) -> SelectedGraph {
    let corpus_count = chosen
        .contexts
        .iter()
        .map(|x| x.corpus_id.as_str())
        .collect::<HashSet<_>>()
        .len();
    let features = chosen
        .contexts
        .iter()
        .map(|x| context_features(x, &candidate.lemma_a, &candidate.lemma_b))
        .collect::<Vec<_>>();
    SelectedGraph {
        contexts: chosen.contexts,
        features,
        corpus_count,
        iqr: chosen.jaccard_q3 - chosen.jaccard_q1,
    }
}

fn materialize_graph(
    candidate: &Candidate,
    split: &str,
    graph: SelectedGraph,
    public: &mut Vec<PublicPacket>,
    private: &mut Vec<PrivateEdge>,
) {
    let pair = [candidate.lemma_a.clone(), candidate.lemma_b.clone()];
    let mut overlaps = Vec::with_capacity(EDGES_PER_GRAPH);
    let mut edges = Vec::with_capacity(EDGES_PER_GRAPH);
    for left in 0..CONTEXTS_PER_GRAPH {
        for right in left + 1..CONTEXTS_PER_GRAPH {
            let jac = jaccard(&graph.features[left].tokens, &graph.features[right].tokens);
            overlaps.push(jac);
            edges.push((left, right, jac));
        }
    }
    let mut sorted = overlaps;
    sorted.sort_by(f64::total_cmp);
    let q1 = sorted[3];
    let q3 = sorted[11];
    for (edge_index, (left, right, jac)) in edges.into_iter().enumerate() {
        let id = salted_hash(&[
            candidate.candidate_id.as_bytes(),
            split.as_bytes(),
            graph.contexts[left].node_id.as_bytes(),
            graph.contexts[right].node_id.as_bytes(),
            &(edge_index as u64).to_le_bytes(),
        ]);
        let packet_id = format!("o2-{}", &id[..16]);
        let reverse = id.as_bytes()[0] & 1 == 1;
        let (public_left, public_right) = if reverse {
            (right, left)
        } else {
            (left, right)
        };
        public.push(PublicPacket {
            packet_id: packet_id.clone(),
            lexical_pair: pair.clone(),
            left_context: graph.contexts[public_left].excerpt.clone(),
            right_context: graph.contexts[public_right].excerpt.clone(),
            judgment: None,
        });
        let band = if jac <= q1 {
            "low"
        } else if jac >= q3 {
            "high"
        } else {
            "middle"
        };
        private.push(PrivateEdge {
            packet_id,
            candidate_id: candidate.candidate_id.clone(),
            lexical_pair: pair.clone(),
            split: split.to_owned(),
            overlap_band: band.to_owned(),
            context_jaccard: jac,
            left_occurrence: graph.contexts[left].clone(),
            right_occurrence: graph.contexts[right].clone(),
            left_features: graph.features[left].clone(),
            right_features: graph.features[right].clone(),
            pair_features: pair_features(&graph.features[left], &graph.features[right]),
        });
    }
}

fn salted_hash(parts: &[&[u8]]) -> String {
    let mut hash = Sha256::new();
    hash.update(SALT);
    for part in parts {
        hash.update((part.len() as u64).to_le_bytes());
        hash.update(part);
    }
    hex(&hash.finalize())
}

fn sha256(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}
fn hex(bytes: &[u8]) -> String {
    const H: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(H[(byte >> 4) as usize] as char);
        out.push(H[(byte & 15) as usize] as char);
    }
    out
}
fn hash_file(path: &Path) -> Result<String> {
    Ok(sha256(
        &fs::read(path).with_context(|| format!("hash {}", path.display()))?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate() -> Candidate {
        Candidate {
            candidate_id: "a-b".into(),
            lemma_a: "alpha".into(),
            lemma_b: "beta".into(),
        }
    }

    #[test]
    fn candidate_tokens_are_not_context_features() {
        let occ = p1o1::Occurrence {
            node_id: "n".into(),
            term: "alpha".into(),
            corpus_index: 0,
            corpus_id: "c".into(),
            document_id: "d".into(),
            document_sha256: "h".into(),
            field: "text".into(),
            excerpt: "alpha [alpha] useful beta context".into(),
            selection_key: "k".into(),
        };
        let c = candidate();
        let features = context_features(&occ, &c.lemma_a, &c.lemma_b);
        assert!(!features.tokens.contains(&"alpha".to_owned()));
        assert!(!features.tokens.contains(&"beta".to_owned()));
        assert!(features.tokens.contains(&"useful".to_owned()));
        assert!(features.masked_template.contains("<FOCAL>"));
    }

    #[test]
    fn pair_features_are_symmetric_and_candidate_excluded() {
        let make = |id: &str, term: &str, text: &str| p1o1::Occurrence {
            node_id: id.into(),
            term: term.into(),
            corpus_index: 0,
            corpus_id: "c".into(),
            document_id: id.into(),
            document_sha256: id.into(),
            field: "text".into(),
            excerpt: text.into(),
            selection_key: id.into(),
        };
        let c = candidate();
        let a = context_features(
            &make("1", "alpha", "useful [alpha] context"),
            &c.lemma_a,
            &c.lemma_b,
        );
        let b = context_features(
            &make("2", "beta", "useful [beta] context"),
            &c.lemma_a,
            &c.lemma_b,
        );
        let ab = pair_features(&a, &b);
        let ba = pair_features(&b, &a);
        assert_eq!(ab.shared_tokens, ba.shared_tokens);
        assert_eq!(ab.token_jaccard, ba.token_jaccard);
        assert!(ab.shared_tokens.contains(&"useful".to_owned()));
        assert!(!ab.shared_tokens.contains(&"alpha".to_owned()));
    }

    #[test]
    fn document_extractor_collects_only_hash_fields() {
        let value: Value = serde_json::json!({"left_document_sha256":"a", "feature":"secret", "nested":{"document_sha256":"b"}});
        let mut hashes = HashSet::new();
        collect_document_hashes(&value, &mut hashes);
        assert_eq!(hashes.len(), 2);
        assert!(hashes.contains("a") && hashes.contains("b"));
    }
}
