//! P1P3 label-blind acquisition. It samples occurrence-context pairs only;
//! it does not read review labels, materialize model features, or fit a model.

use anyhow::{ensure, Context, Result};
use hashbrown::{HashMap, HashSet};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[path = "lt9_la2p1o1_core.rs"]
#[allow(dead_code)]
mod p1o1;

const DATE: &str = "2026-09-24";
const TARGET_CANDIDATES: usize = 4;
const CONTEXTS_PER_GRAPH: usize = 6;
const EDGES_PER_GRAPH: usize = 15;
const REVIEWERS: [&str; 3] = ["reviewer-1", "reviewer-2", "reviewer-3"];
const SALT: &[u8] = b"phoenix-lt9-la2-p1p3-consensus-observability-20260924-v1";
type TermMap = HashMap<String, usize>;
type ContextBank = Vec<Vec<Vec<p1o1::Occurrence>>>;

#[derive(Deserialize)]
struct Roster {
    corpora: Vec<p1o1::CorpusSpec>,
}

#[derive(Serialize)]
struct PublicPacket {
    packet_id: String,
    lexical_pair: [String; 2],
    left_context: String,
    right_context: String,
}

#[derive(Serialize)]
struct JudgmentTemplate {
    packet_id: String,
    judgment: Option<String>,
}

#[derive(Serialize)]
struct OccurrenceRef {
    node_id: String,
    corpus_id: String,
    document_sha256: String,
    field: String,
}

#[derive(Serialize)]
struct PrivateEdge {
    edge_key: String,
    candidate_id: String,
    lexical_pair: [String; 2],
    split: String,
    overlap_band: String,
    reviewer_packet_ids: [String; 3],
    left: OccurrenceRef,
    right: OccurrenceRef,
}

#[derive(Serialize)]
struct CandidateReceipt {
    candidate_id: String,
    lexical_pair: [String; 2],
    status: String,
    fit_contexts: usize,
    holdout_contexts: usize,
    fit_corpora: Vec<String>,
    holdout_corpora: Vec<String>,
    fit_overlap_iqr: Option<f64>,
    holdout_overlap_iqr: Option<f64>,
    reason: Option<String>,
}

#[derive(Serialize)]
struct ReviewerReceipt {
    reviewer_slot: String,
    packet_count: usize,
    packets_sha256: String,
    template_sha256: String,
    rubric_sha256: String,
}

#[derive(Serialize)]
struct InputReceipt {
    role: String,
    path: String,
    sha256: String,
}

#[derive(Serialize)]
struct AcquisitionReceipt {
    schema: &'static str,
    date: &'static str,
    status: &'static str,
    protocol_sha256: String,
    rubric_sha256: String,
    source_sha256: String,
    p1o1_core_sha256: String,
    p1o1_scan_sha256: String,
    p1o2_feature_definition_sha256: String,
    harness_manifest_sha256: String,
    harness_lockfile_sha256: String,
    executable_sha256: String,
    inputs: Vec<InputReceipt>,
    proposal_pool_size: usize,
    proposals_examined: usize,
    prior_candidate_lemma_exclusion_count: usize,
    selected_candidate_lemma_count: usize,
    excluded_prior_document_count: usize,
    excluded_prior_document_hashes_sha256: String,
    corpus_hashes_verified: bool,
    corpus_scans: Vec<p1o1::CorpusScanReceipt>,
    candidates: Vec<CandidateReceipt>,
    physical_pair_count: usize,
    private_ledger_sha256: String,
    reviewer_files: Vec<ReviewerReceipt>,
    prior_judgment_files_opened: bool,
    qrels_or_queries_read: bool,
    model_features_materialized: bool,
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
    source_sha256: String,
    executable_sha256: String,
    private_ledger_sha256: String,
    reviewer_packet_hashes: Vec<String>,
    reviewer_template_hashes: Vec<String>,
    reviewer_receives_sampling_metadata: bool,
    reviewer_reviews_complete: bool,
    labels_compared: bool,
    feature_fit_authorized: bool,
    memory_or_retrieval_authorized: bool,
}

#[derive(Clone)]
struct EdgeDraft {
    edge_key: String,
    candidate_id: String,
    pair: [String; 2],
    split: String,
    overlap_band: String,
    left: p1o1::Occurrence,
    right: p1o1::Occurrence,
}

#[derive(Deserialize)]
struct CandidateExclusions {
    excluded_lemmas: Vec<String>,
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    ensure!(
        args.len() == 16,
        "usage: lt9_la2p1p3_acquire <roster.json> <wordnet-dict> <wordnet-archive> <excluded-lemmas.json> <p1n3-packets.json> <p1n3-private.json> <p1n4-packets.json> <p1n4-private.json> <p1o1-packets.json> <p1o1-private.json> <p1o2-packets.json> <p1o2-private.json> <rubric.md> <protocol.md> <new-output-dir>"
    );
    let roster_path = PathBuf::from(&args[1]);
    let wordnet_dict = PathBuf::from(&args[2]);
    let wordnet_archive = PathBuf::from(&args[3]);
    let excluded_lemmas_path = PathBuf::from(&args[4]);
    let prior_packet_paths = [
        PathBuf::from(&args[5]),
        PathBuf::from(&args[7]),
        PathBuf::from(&args[9]),
        PathBuf::from(&args[11]),
    ];
    let prior_ledger_paths = [
        PathBuf::from(&args[6]),
        PathBuf::from(&args[8]),
        PathBuf::from(&args[10]),
        PathBuf::from(&args[12]),
    ];
    let rubric_path = PathBuf::from(&args[13]);
    let protocol_path = PathBuf::from(&args[14]);
    let output_final = PathBuf::from(&args[15]);
    let output = output_final.with_extension(format!("p1p3-staging-{}", std::process::id()));
    ensure!(
        !output_final.exists(),
        "refusing to overwrite {}",
        output_final.display()
    );
    ensure!(
        !output.exists(),
        "refusing to overwrite staging path {}",
        output.display()
    );

    let roster_bytes = fs::read(&roster_path)?;
    let roster: Roster = serde_json::from_slice(&roster_bytes)?;
    ensure!(
        roster.corpora.len() == 13,
        "P1P3 requires the frozen 13-corpus roster"
    );
    ensure!(
        !roster.corpora.iter().any(|corpus| {
            ["webis-touche2020", "nq", "hotpotqa"].contains(&corpus.corpus_id.as_str())
        }),
        "a sealed qualification corpus appears in the discovery roster"
    );

    let mut excluded_lemmas: HashSet<String> =
        serde_json::from_slice::<CandidateExclusions>(&fs::read(&excluded_lemmas_path)?)?
            .excluded_lemmas
            .into_iter()
            .map(|lemma| lemma.to_ascii_lowercase())
            .collect();
    for path in &prior_packet_paths {
        let added = collect_packet_lemmas(path, &mut excluded_lemmas)?;
        ensure!(
            added > 0,
            "prior packet file has no lexical-pair entries: {}",
            path.display()
        );
    }
    let prior_candidate_lemma_exclusion_count = excluded_lemmas.len();
    let mut excluded_documents = HashSet::<String>::new();
    for path in &prior_ledger_paths {
        let bytes = fs::read(path)
            .with_context(|| format!("read prior private ledger {}", path.display()))?;
        let value: Value = serde_json::from_slice(&bytes)?;
        collect_document_hashes(&value, &mut excluded_documents);
    }

    let mut proposal_pool = p1o1::wordnet_candidate_pool(&wordnet_dict, &excluded_lemmas)?;
    for candidate in &mut proposal_pool {
        let priority = salted_hash(&[candidate.lemma_a.as_bytes(), candidate.lemma_b.as_bytes()]);
        candidate.candidate_id = priority[..16].to_owned();
        candidate.priority_sha256 = priority;
    }
    proposal_pool.sort_unstable_by(|left, right| left.priority_sha256.cmp(&right.priority_sha256));

    let mut term_map = TermMap::with_capacity(proposal_pool.len());
    for candidate in &proposal_pool {
        let next = term_map.len();
        term_map.entry(candidate.lemma_a.clone()).or_insert(next);
        let next = term_map.len();
        term_map.entry(candidate.lemma_b.clone()).or_insert(next);
    }
    let mut contexts: ContextBank =
        vec![vec![Vec::<p1o1::Occurrence>::new(); roster.corpora.len()]; term_map.len()];
    let mut corpus_scans = Vec::with_capacity(roster.corpora.len());
    for (index, corpus) in roster.corpora.iter().enumerate() {
        corpus_scans.push(
            p1o1::scan_corpus(corpus, index, &term_map, &excluded_documents, &mut contexts)
                .with_context(|| format!("scan corpus {}", corpus.corpus_id))?,
        );
    }
    ensure!(
        corpus_scans.iter().all(|scan| scan.hash_verified),
        "corpus hash verification failed"
    );

    let mut drafts = Vec::<EdgeDraft>::with_capacity(TARGET_CANDIDATES * 2 * EDGES_PER_GRAPH);
    let mut candidate_receipts = Vec::<CandidateReceipt>::new();
    let mut chosen_lemmas = excluded_lemmas;
    let mut chosen_documents = HashSet::<String>::new();
    let mut proposals_examined = 0usize;
    for candidate in &proposal_pool {
        proposals_examined += 1;
        if candidate_receipts.len() == TARGET_CANDIDATES {
            break;
        }
        if chosen_lemmas.contains(&candidate.lemma_a) || chosen_lemmas.contains(&candidate.lemma_b)
        {
            continue;
        }
        let local = candidate_contexts(candidate, &term_map, &contexts, &chosen_documents)?;
        let Some(first) = select_graph(candidate, &local.0, &local.1) else {
            continue;
        };
        let first_docs = graph_documents(&first);
        let mut second_source = local.1.clone();
        remove_documents(&mut second_source, &first_docs);
        let Some(second) = select_graph(candidate, &local.0, &second_source) else {
            continue;
        };
        let second_docs = graph_documents(&second);
        ensure!(
            first_docs.is_disjoint(&second_docs),
            "within-candidate graph documents overlap"
        );
        ensure!(
            first_docs.is_disjoint(&chosen_documents),
            "fit graph reuses a prior candidate document"
        );
        ensure!(
            second_docs.is_disjoint(&chosen_documents),
            "holdout graph reuses a prior candidate document"
        );

        let first_is_fit =
            salted_hash(&[candidate.candidate_id.as_bytes(), b"graph-role"]).as_bytes()[0] & 1 == 0;
        let (fit_graph, holdout_graph) = if first_is_fit {
            (&first, &second)
        } else {
            (&second, &first)
        };
        let fit_iqr = append_graph_edges(candidate, fit_graph, "fit", &mut drafts);
        let holdout_iqr = append_graph_edges(candidate, holdout_graph, "holdout", &mut drafts);
        chosen_documents.extend(first_docs);
        chosen_documents.extend(second_docs);
        chosen_lemmas.insert(candidate.lemma_a.clone());
        chosen_lemmas.insert(candidate.lemma_b.clone());
        candidate_receipts.push(CandidateReceipt {
            candidate_id: candidate.candidate_id.clone(),
            lexical_pair: [candidate.lemma_a.clone(), candidate.lemma_b.clone()],
            status: "PACKETS_READY".to_owned(),
            fit_contexts: CONTEXTS_PER_GRAPH,
            holdout_contexts: CONTEXTS_PER_GRAPH,
            fit_corpora: sorted_corpora(fit_graph),
            holdout_corpora: sorted_corpora(holdout_graph),
            fit_overlap_iqr: Some(fit_iqr),
            holdout_overlap_iqr: Some(holdout_iqr),
            reason: None,
        });
    }

    fs::create_dir_all(&output)?;
    let reviewer_packets = materialize_reviewer_packets(&drafts)?;
    let private_rows = materialize_private_rows(&drafts)?;
    p1o1::write_json(&output.join("private-ledger.json"), &private_rows)?;
    let mut excluded_sorted = excluded_documents.iter().cloned().collect::<Vec<_>>();
    excluded_sorted.sort_unstable();
    p1o1::write_json(
        &output.join("excluded-document-hashes.json"),
        &excluded_sorted,
    )?;

    let exe = env::current_exe()?;
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let source_path =
        manifest_dir.join("../../apps/phoenix-memory-lock/src/bin/lt9_la2p1p3_acquire.rs");
    let core_path = manifest_dir.join("../../apps/phoenix-memory-lock/src/bin/lt9_la2p1o1_core.rs");
    let scan_path = manifest_dir.join("../../apps/phoenix-memory-lock/src/bin/lt9_la2p1o1_scan.rs");
    let p1o2_feature_path =
        manifest_dir.join("../../apps/phoenix-memory-lock/src/bin/lt9_la2p1o2_features.rs");
    let lock_path = manifest_dir.join("Cargo.lock");
    let reviewer_receipts = write_reviewer_files(&output, &reviewer_packets, &rubric_path)?;
    let reviewer_packet_hashes = reviewer_receipts
        .iter()
        .map(|row| row.packets_sha256.clone())
        .collect::<Vec<_>>();
    let reviewer_template_hashes = reviewer_receipts
        .iter()
        .map(|row| row.template_sha256.clone())
        .collect::<Vec<_>>();
    let selected_candidate_count = candidate_receipts.len();
    let mut inputs = vec![
        input_receipt("corpus roster", &roster_path)?,
        input_receipt("WordNet source archive", &wordnet_archive)?,
        input_receipt("prior candidate exclusion list", &excluded_lemmas_path)?,
        input_receipt("P1P3 rubric", &rubric_path)?,
        input_receipt("P1P3 protocol", &protocol_path)?,
    ];
    for name in ["data.noun", "data.verb", "data.adj", "data.adv"] {
        inputs.push(input_receipt(
            &format!("WordNet {name}"),
            &wordnet_dict.join(name),
        )?);
    }
    for (index, path) in prior_packet_paths.iter().enumerate() {
        inputs.push(input_receipt(
            [
                "P1N3 public packets",
                "P1N4 public packets",
                "P1O1 public packets",
                "P1O2 public packets",
            ][index],
            path,
        )?);
    }
    for (index, path) in prior_ledger_paths.iter().enumerate() {
        inputs.push(input_receipt(
            [
                "P1N3 private ledger",
                "P1N4 private ledger",
                "P1O1 private ledger",
                "P1O2 private ledger",
            ][index],
            path,
        )?);
    }
    let status = if candidate_receipts.len() == TARGET_CANDIDATES {
        "BLIND_PACKETS_READY"
    } else {
        "LABEL_BLIND_ACQUISITION_SHORTFALL"
    };
    let receipt = AcquisitionReceipt {
        schema: "phoenix.lexical.lt9-la2-p1p3-acquisition/v1",
        date: DATE,
        status,
        protocol_sha256: p1o1::hash_file(&protocol_path)?,
        rubric_sha256: p1o1::hash_file(&rubric_path)?,
        source_sha256: p1o1::hash_file(&source_path)?,
        p1o1_core_sha256: p1o1::hash_file(&core_path)?,
        p1o1_scan_sha256: p1o1::hash_file(&scan_path)?,
        p1o2_feature_definition_sha256: p1o1::hash_file(&p1o2_feature_path)?,
        harness_manifest_sha256: p1o1::hash_file(&manifest_dir.join("Cargo.toml"))?,
        harness_lockfile_sha256: p1o1::hash_file(&lock_path)?,
        executable_sha256: p1o1::hash_file(&exe)?,
        inputs,
        proposal_pool_size: proposal_pool.len(),
        proposals_examined,
        prior_candidate_lemma_exclusion_count,
        selected_candidate_lemma_count: candidate_receipts.len() * 2,
        excluded_prior_document_count: excluded_documents.len(),
        excluded_prior_document_hashes_sha256: p1o1::hash_file(
            &output.join("excluded-document-hashes.json"),
        )?,
        corpus_hashes_verified: true,
        corpus_scans,
        candidates: candidate_receipts,
        physical_pair_count: drafts.len(),
        private_ledger_sha256: p1o1::hash_file(&output.join("private-ledger.json"))?,
        reviewer_files: reviewer_receipts,
        prior_judgment_files_opened: false,
        qrels_or_queries_read: false,
        model_features_materialized: false,
        feature_fit: false,
        authority_updated: false,
        retrieval_run: false,
    };
    p1o1::write_json(&output.join("acquisition-receipt.json"), &receipt)?;
    let root = PreReviewRoot {
        schema: "phoenix.lexical.lt9-la2-p1p3-pre-review-root/v1",
        status,
        acquisition_receipt_sha256: p1o1::hash_file(&output.join("acquisition-receipt.json"))?,
        protocol_sha256: receipt.protocol_sha256.clone(),
        rubric_sha256: receipt.rubric_sha256.clone(),
        source_sha256: receipt.source_sha256.clone(),
        executable_sha256: receipt.executable_sha256.clone(),
        private_ledger_sha256: receipt.private_ledger_sha256.clone(),
        reviewer_packet_hashes,
        reviewer_template_hashes,
        reviewer_receives_sampling_metadata: false,
        reviewer_reviews_complete: false,
        labels_compared: false,
        feature_fit_authorized: false,
        memory_or_retrieval_authorized: false,
    };
    p1o1::write_json(&output.join("pre-review-root.json"), &root)?;
    fs::rename(&output, &output_final).with_context(|| {
        format!(
            "publish completed blind acquisition to {}",
            output_final.display()
        )
    })?;
    println!(
        "{status}: {} candidates, {} physical pairs, three independent review sets -> {}",
        selected_candidate_count,
        drafts.len(),
        output_final.display()
    );
    Ok(())
}

fn collect_packet_lemmas(path: &Path, out: &mut HashSet<String>) -> Result<usize> {
    let bytes = fs::read(path)
        .with_context(|| format!("read prior public packet file {}", path.display()))?;
    let rows: Value = serde_json::from_slice(&bytes)?;
    let packets = rows
        .as_array()
        .context("prior public packets must be a JSON array")?;
    let mut entries = 0usize;
    for packet in packets {
        let pair = packet
            .get("lexical_pair")
            .and_then(Value::as_array)
            .context("prior packet lacks lexical_pair")?;
        ensure!(
            pair.len() == 2,
            "prior packet lexical_pair must have two lemmas"
        );
        ensure!(
            pair.iter().all(Value::is_string),
            "prior packet lexical_pair contains a non-string lemma"
        );
        for lemma in pair.iter().filter_map(Value::as_str) {
            out.insert(lemma.to_ascii_lowercase());
        }
        entries += 1;
    }
    Ok(entries)
}

fn collect_document_hashes(value: &Value, out: &mut HashSet<String>) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                if matches!(
                    key.as_str(),
                    "document_sha256" | "left_document_sha256" | "right_document_sha256"
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

fn candidate_contexts(
    candidate: &p1o1::CandidatePair,
    term_map: &TermMap,
    contexts: &ContextBank,
    used_documents: &HashSet<String>,
) -> Result<(TermMap, ContextBank)> {
    let source_index = *term_map
        .get(&candidate.lemma_a)
        .context("candidate source absent from term map")?;
    let target_index = *term_map
        .get(&candidate.lemma_b)
        .context("candidate target absent from term map")?;
    let local_map = TermMap::from([
        (candidate.lemma_a.clone(), 0usize),
        (candidate.lemma_b.clone(), 1usize),
    ]);
    let mut local = vec![
        contexts[source_index].clone(),
        contexts[target_index].clone(),
    ];
    remove_documents(&mut local, used_documents);
    Ok((local_map, local))
}

fn select_graph(
    candidate: &p1o1::CandidatePair,
    term_map: &TermMap,
    contexts: &ContextBank,
) -> Option<p1o1::ChosenCandidate> {
    p1o1::select_candidate_graphs(std::slice::from_ref(candidate), term_map, contexts)
        .chosen
        .into_iter()
        .next()
}

fn graph_documents(graph: &p1o1::ChosenCandidate) -> HashSet<String> {
    graph
        .contexts
        .iter()
        .map(|row| row.document_sha256.clone())
        .collect()
}

fn remove_documents(contexts: &mut [Vec<Vec<p1o1::Occurrence>>], excluded: &HashSet<String>) {
    for per_corpus in contexts.iter_mut().flatten() {
        per_corpus.retain(|row| !excluded.contains(&row.document_sha256));
    }
}

fn token_set(occurrence: &p1o1::Occurrence, lemma_a: &str, lemma_b: &str) -> HashSet<String> {
    let bytes = occurrence.excerpt.as_bytes();
    let focal_start = occurrence.excerpt.find('[').map(|i| i + 1).unwrap_or(0);
    let focal_end = occurrence.excerpt[focal_start..]
        .find(']')
        .map(|i| focal_start + i)
        .unwrap_or(focal_start);
    let mut result = HashSet::new();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        while cursor < bytes.len() && !bytes[cursor].is_ascii_alphanumeric() {
            cursor += 1;
        }
        let start = cursor;
        while cursor < bytes.len() && bytes[cursor].is_ascii_alphanumeric() {
            cursor += 1;
        }
        if cursor == start {
            continue;
        }
        let focal = start >= focal_start && cursor <= focal_end;
        if focal {
            continue;
        }
        let token = occurrence.excerpt[start..cursor].to_ascii_lowercase();
        if token != lemma_a && token != lemma_b {
            result.insert(token);
        }
    }
    result
}

fn jaccard(left: &HashSet<String>, right: &HashSet<String>) -> f64 {
    let intersection = left.iter().filter(|token| right.contains(*token)).count();
    let union = left.len() + right.len() - intersection;
    if union == 0 {
        1.0
    } else {
        intersection as f64 / union as f64
    }
}

fn overlap_band(scores: &[f64], index: usize) -> &'static str {
    let mut sorted = scores.to_vec();
    sorted.sort_by(f64::total_cmp);
    let q1 = sorted[3];
    let q3 = sorted[11];
    if scores[index] <= q1 {
        "low"
    } else if scores[index] >= q3 {
        "high"
    } else {
        "middle"
    }
}

fn append_graph_edges(
    candidate: &p1o1::CandidatePair,
    graph: &p1o1::ChosenCandidate,
    split: &str,
    out: &mut Vec<EdgeDraft>,
) -> f64 {
    let pair = [candidate.lemma_a.clone(), candidate.lemma_b.clone()];
    let token_sets = graph
        .contexts
        .iter()
        .map(|row| token_set(row, &pair[0], &pair[1]))
        .collect::<Vec<_>>();
    let mut edge_scores = Vec::with_capacity(EDGES_PER_GRAPH);
    for left in 0..CONTEXTS_PER_GRAPH {
        for right in (left + 1)..CONTEXTS_PER_GRAPH {
            edge_scores.push((left, right, jaccard(&token_sets[left], &token_sets[right])));
        }
    }
    let mut sorted = edge_scores.iter().map(|edge| edge.2).collect::<Vec<_>>();
    sorted.sort_by(f64::total_cmp);
    let iqr = sorted[11] - sorted[3];
    for (edge_index, (left_index, right_index, score)) in edge_scores.into_iter().enumerate() {
        let left = graph.contexts[left_index].clone();
        let right = graph.contexts[right_index].clone();
        let edge_key = salted_hash(&[
            candidate.candidate_id.as_bytes(),
            split.as_bytes(),
            left.node_id.as_bytes(),
            right.node_id.as_bytes(),
            &(edge_index as u64).to_le_bytes(),
        ]);
        let reverse = edge_key.as_bytes()[0] & 1 == 1;
        let (left, right) = if reverse {
            (right, left)
        } else {
            (left, right)
        };
        out.push(EdgeDraft {
            edge_key,
            candidate_id: candidate.candidate_id.clone(),
            pair: pair.clone(),
            split: split.to_owned(),
            overlap_band: overlap_band(&sorted, edge_index).to_owned(),
            left,
            right,
        });
        let _ = score;
    }
    iqr
}

fn materialize_reviewer_packets(
    drafts: &[EdgeDraft],
) -> Result<HashMap<String, Vec<PublicPacket>>> {
    let mut result = HashMap::with_capacity(REVIEWERS.len());
    for reviewer in REVIEWERS {
        let mut rows = drafts
            .iter()
            .map(|edge| {
                let id = salted_hash(&[reviewer.as_bytes(), edge.edge_key.as_bytes()]);
                let reverse = id.as_bytes()[0] & 1 == 1;
                let (left, right) = if reverse {
                    (&edge.right, &edge.left)
                } else {
                    (&edge.left, &edge.right)
                };
                (
                    salted_hash(&[reviewer.as_bytes(), b"order", id.as_bytes()]),
                    PublicPacket {
                        packet_id: format!("p3-{}", &id[..16]),
                        lexical_pair: edge.pair.clone(),
                        left_context: left.excerpt.clone(),
                        right_context: right.excerpt.clone(),
                    },
                )
            })
            .collect::<Vec<_>>();
        rows.sort_unstable_by(|left, right| left.0.cmp(&right.0));
        let packets = rows.into_iter().map(|row| row.1).collect::<Vec<_>>();
        ensure!(
            packets.len() == drafts.len(),
            "reviewer packet count mismatch"
        );
        let unique_ids = packets
            .iter()
            .map(|row| row.packet_id.clone())
            .collect::<HashSet<_>>();
        ensure!(
            unique_ids.len() == packets.len(),
            "reviewer packet ID collision"
        );
        result.insert(reviewer.to_owned(), packets);
    }
    Ok(result)
}

fn materialize_private_rows(drafts: &[EdgeDraft]) -> Result<Vec<PrivateEdge>> {
    let mut seen = HashSet::with_capacity(drafts.len());
    let mut rows = Vec::with_capacity(drafts.len());
    for edge in drafts {
        ensure!(
            seen.insert(edge.edge_key.clone()),
            "physical edge key collision"
        );
        let ids = REVIEWERS.map(|reviewer| {
            let id = salted_hash(&[reviewer.as_bytes(), edge.edge_key.as_bytes()]);
            format!("p3-{}", &id[..16])
        });
        rows.push(PrivateEdge {
            edge_key: edge.edge_key.clone(),
            candidate_id: edge.candidate_id.clone(),
            lexical_pair: edge.pair.clone(),
            split: edge.split.clone(),
            overlap_band: edge.overlap_band.clone(),
            reviewer_packet_ids: ids,
            left: occurrence_ref(&edge.left),
            right: occurrence_ref(&edge.right),
        });
    }
    Ok(rows)
}

fn occurrence_ref(occurrence: &p1o1::Occurrence) -> OccurrenceRef {
    OccurrenceRef {
        node_id: occurrence.node_id.clone(),
        corpus_id: occurrence.corpus_id.clone(),
        document_sha256: occurrence.document_sha256.clone(),
        field: occurrence.field.clone(),
    }
}

fn write_reviewer_files(
    output: &Path,
    packets: &HashMap<String, Vec<PublicPacket>>,
    rubric_path: &Path,
) -> Result<Vec<ReviewerReceipt>> {
    let rubric_bytes = fs::read(rubric_path)?;
    let mut receipts = Vec::with_capacity(REVIEWERS.len());
    for reviewer in REVIEWERS {
        let dir = output.join("blind-review").join(reviewer);
        fs::create_dir_all(&dir)?;
        let reviewer_packets = packets
            .get(reviewer)
            .context("reviewer packet set missing")?;
        let packet_path = dir.join("packets.json");
        let template_path = dir.join("judgments-template.json");
        p1o1::write_json(&packet_path, reviewer_packets)?;
        let template = reviewer_packets
            .iter()
            .map(|packet| JudgmentTemplate {
                packet_id: packet.packet_id.clone(),
                judgment: None,
            })
            .collect::<Vec<_>>();
        p1o1::write_json(&template_path, &template)?;
        fs::write(dir.join("rubric.md"), &rubric_bytes)?;
        receipts.push(ReviewerReceipt {
            reviewer_slot: reviewer.to_owned(),
            packet_count: reviewer_packets.len(),
            packets_sha256: p1o1::hash_file(&packet_path)?,
            template_sha256: p1o1::hash_file(&template_path)?,
            rubric_sha256: p1o1::hash_file(&dir.join("rubric.md"))?,
        });
    }
    Ok(receipts)
}

fn sorted_corpora(graph: &p1o1::ChosenCandidate) -> Vec<String> {
    let mut ids = graph
        .contexts
        .iter()
        .map(|row| row.corpus_id.clone())
        .collect::<Vec<_>>();
    ids.sort_unstable();
    ids.dedup();
    ids
}

fn input_receipt(role: &str, path: &Path) -> Result<InputReceipt> {
    Ok(InputReceipt {
        role: role.to_owned(),
        path: path.display().to_string(),
        sha256: p1o1::hash_file(path)?,
    })
}

fn salted_hash(parts: &[&[u8]]) -> String {
    let mut digest = Sha256::new();
    digest.update(SALT);
    for part in parts {
        digest.update((part.len() as u64).to_le_bytes());
        digest.update(part);
    }
    let bytes = digest.finalize();
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn occurrence(text: &str) -> p1o1::Occurrence {
        p1o1::Occurrence {
            node_id: "n".into(),
            term: "car".into(),
            corpus_index: 0,
            corpus_id: "c".into(),
            document_id: "d".into(),
            document_sha256: "h".into(),
            field: "text".into(),
            excerpt: text.into(),
            selection_key: "s".into(),
        }
    }

    #[test]
    fn sampling_tokens_exclude_focal_and_both_candidate_words() {
        let tokens = token_set(&occurrence("A [car] vehicle with roads"), "car", "vehicle");
        assert_eq!(
            tokens,
            HashSet::from(["a".to_owned(), "with".to_owned(), "roads".to_owned()])
        );
    }

    #[test]
    fn p1o2_quartile_rule_uses_sorted_positions_three_and_eleven() {
        let scores = (0..15).map(|value| value as f64).collect::<Vec<_>>();
        assert_eq!(overlap_band(&scores, 0), "low");
        assert_eq!(overlap_band(&scores, 3), "low");
        assert_eq!(overlap_band(&scores, 4), "middle");
        assert_eq!(overlap_band(&scores, 10), "middle");
        assert_eq!(overlap_band(&scores, 11), "high");
        assert_eq!(overlap_band(&scores, 14), "high");
    }

    #[test]
    fn candidate_and_reviewer_salts_make_separate_blind_ids() {
        let edge = b"physical-edge";
        assert_ne!(
            salted_hash(&[b"reviewer-1", edge]),
            salted_hash(&[b"reviewer-2", edge])
        );
        assert_eq!(salted_hash(&[edge]), salted_hash(&[edge]));
    }
}
