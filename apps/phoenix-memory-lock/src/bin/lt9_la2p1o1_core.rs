use anyhow::{Context, Result, ensure};
use hashbrown::{HashMap, HashSet};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

#[path = "lt9_la2p1o1_scan.rs"]
mod scan;
pub use scan::scan_corpus;

pub const DATE: &str = "2026-09-23";
pub const TARGET_CANDIDATES: usize = 8;
pub const CONTEXTS_PER_CANDIDATE: usize = 6;
const MAX_OCCURRENCES_PER_TERM_CORPUS: usize = 2;
const MAX_TRIPLETS_PER_TERM: usize = 48;
const SALT: &[u8] = b"phoenix-lt9-la2-p1o1-20260923-v1";

#[derive(Clone, Deserialize)]
pub struct CorpusRoster {
    pub corpora: Vec<CorpusSpec>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct CorpusSpec {
    pub corpus_id: String,
    pub path: String,
    pub sha256: String,
}

#[derive(Deserialize)]
pub struct CandidateExclusions {
    pub excluded_lemmas: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CandidatePair {
    pub candidate_id: String,
    pub lemma_a: String,
    pub lemma_b: String,
    pub priority_sha256: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Occurrence {
    pub node_id: String,
    pub term: String,
    pub corpus_index: usize,
    pub corpus_id: String,
    pub document_id: String,
    pub document_sha256: String,
    pub field: String,
    pub excerpt: String,
    pub selection_key: String,
}

#[derive(Clone, Serialize)]
pub struct CorpusScanReceipt {
    pub corpus_id: String,
    pub corpus_sha256_expected: String,
    pub corpus_sha256_actual: String,
    pub hash_verified: bool,
    pub documents_scanned: u64,
    pub prior_review_documents_skipped: u64,
    pub matched_document_terms: u64,
    pub retained_occurrences: u64,
}

#[derive(Clone)]
pub struct ChosenCandidate {
    pub candidate: CandidatePair,
    pub contexts: Vec<Occurrence>,
    pub structural_signatures: usize,
    pub unique_context_tokens: usize,
    pub jaccard_q1: f64,
    pub jaccard_median: f64,
    pub jaccard_q3: f64,
}

pub struct CandidateSelection {
    pub chosen: Vec<ChosenCandidate>,
    pub eligible_candidates: usize,
    pub candidates_examined: usize,
}

#[derive(Serialize)]
pub struct PublicPacket {
    pub packet_id: String,
    pub lexical_pair: [String; 2],
    pub left_context: String,
    pub right_context: String,
    pub judgment: Option<String>,
}

#[derive(Serialize)]
pub struct PrivateEdgeRow {
    pub packet_id: String,
    pub candidate_id: String,
    pub edge_index: usize,
    pub lexical_pair: [String; 2],
    pub left: Occurrence,
    pub right: Occurrence,
    pub context_jaccard: f64,
    pub left_structural_signature: String,
    pub right_structural_signature: String,
}

#[derive(Serialize)]
pub struct CandidateIntakeReport {
    pub schema: &'static str,
    pub date: &'static str,
    pub proposal_pool_size: usize,
    pub eligible_candidates_encountered: usize,
    pub candidates_examined_in_hash_order: usize,
    pub selected_candidates: Vec<CandidateIntakeRow>,
    pub packet_count: usize,
    pub edge_count_per_candidate: usize,
    pub selection_uses_human_labels: bool,
}

#[derive(Serialize)]
pub struct CandidateIntakeRow {
    pub candidate_id: String,
    pub lexical_pair: [String; 2],
    pub contexts: usize,
    pub corpus_count: usize,
    pub structural_signatures: usize,
    pub unique_non_candidate_context_tokens: usize,
    pub jaccard_q1: f64,
    pub jaccard_median: f64,
    pub jaccard_q3: f64,
}

#[derive(Deserialize)]
struct PriorDocumentRefs {
    left_document_sha256: Option<String>,
    right_document_sha256: Option<String>,
}

#[derive(Clone)]
struct Triplet([usize; 3]);

pub fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    hex(&digest)
}

pub fn hash_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("read for hashing: {}", path.display()))?;
    Ok(sha256(&bytes))
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 15) as usize] as char);
    }
    out
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

pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let mut file = File::create(path).with_context(|| format!("create {}", path.display()))?;
    serde_json::to_writer_pretty(&mut file, value)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

pub fn load_reviewed_document_hashes(paths: &[std::path::PathBuf]) -> Result<HashSet<String>> {
    let mut excluded = HashSet::new();
    for path in paths {
        let bytes = fs::read(path)
            .with_context(|| format!("read document-exclusion ledger {}", path.display()))?;
        let rows: Vec<PriorDocumentRefs> = serde_json::from_slice(&bytes)
            .with_context(|| format!("parse document-exclusion ledger {}", path.display()))?;
        for row in rows {
            excluded.extend(row.left_document_sha256);
            excluded.extend(row.right_document_sha256);
        }
    }
    Ok(excluded)
}

pub fn wordnet_candidate_pool(
    dict: &Path,
    excluded: &HashSet<String>,
) -> Result<Vec<CandidatePair>> {
    let stop: HashSet<&'static str> = [
        "a", "an", "and", "are", "as", "at", "be", "by", "for", "from", "in", "is", "it", "of",
        "on", "or", "that", "the", "this", "to", "was", "were", "with",
    ]
    .into_iter()
    .collect();
    let mut unique = HashSet::<(String, String)>::new();
    for pos in ["noun", "verb", "adj", "adv"] {
        let path = dict.join(format!("data.{pos}"));
        let text = fs::read_to_string(&path)
            .with_context(|| format!("read WordNet data {}", path.display()))?;
        for line in text.lines() {
            if line.starts_with(' ') || line.is_empty() {
                continue;
            }
            let Some((synset, _)) = line.split_once('|') else {
                continue;
            };
            let fields: Vec<&str> = synset.split_ascii_whitespace().collect();
            if fields.len() < 5 {
                continue;
            }
            let Ok(count) = usize::from_str_radix(fields[3], 16) else {
                continue;
            };
            if count < 2 || fields.len() < 4 + count * 2 {
                continue;
            }
            let mut lemmas = Vec::<String>::with_capacity(count);
            for i in 0..count {
                let raw = fields[4 + i * 2];
                if raw.contains('_')
                    || raw.len() < 3
                    || raw.len() > 32
                    || !raw.bytes().all(|b| b.is_ascii_alphabetic())
                {
                    continue;
                }
                let lemma = raw.to_ascii_lowercase();
                if stop.contains(lemma.as_str()) || excluded.contains(&lemma) {
                    continue;
                }
                lemmas.push(lemma);
            }
            lemmas.sort_unstable();
            lemmas.dedup();
            for a in 0..lemmas.len() {
                for b in (a + 1)..lemmas.len() {
                    if lemmas[a] != lemmas[b] {
                        unique.insert((lemmas[a].clone(), lemmas[b].clone()));
                    }
                }
            }
        }
    }
    let mut rows: Vec<CandidatePair> = unique
        .into_iter()
        .map(|(a, b)| {
            let priority = salted_hash(&[a.as_bytes(), b.as_bytes()]);
            let candidate_id = priority[..16].to_owned();
            CandidatePair {
                candidate_id,
                lemma_a: a,
                lemma_b: b,
                priority_sha256: priority,
            }
        })
        .collect();
    rows.sort_unstable_by(|a, b| a.priority_sha256.cmp(&b.priority_sha256));
    rows.truncate(8192);
    ensure!(
        !rows.is_empty(),
        "WordNet yielded no candidate pairs after frozen exclusions"
    );
    Ok(rows)
}

fn make_triplets(occurrences: &[Occurrence], lemma: &str) -> Vec<Triplet> {
    let mut ranked = Vec::<(String, Triplet)>::new();
    for a in 0..occurrences.len() {
        for b in (a + 1)..occurrences.len() {
            for c in (b + 1)..occurrences.len() {
                let ids = [a, b, c];
                let corpus_set: HashSet<usize> =
                    ids.iter().map(|i| occurrences[*i].corpus_index).collect();
                if corpus_set.len() < 2 {
                    continue;
                }
                let doc_a = &occurrences[a].document_sha256;
                let doc_b = &occurrences[b].document_sha256;
                let doc_c = &occurrences[c].document_sha256;
                if doc_a == doc_b || doc_a == doc_c || doc_b == doc_c {
                    continue;
                }
                let rank = salted_hash(&[
                    lemma.as_bytes(),
                    occurrences[a].node_id.as_bytes(),
                    occurrences[b].node_id.as_bytes(),
                    occurrences[c].node_id.as_bytes(),
                ]);
                ranked.push((rank, Triplet(ids)));
            }
        }
    }
    ranked.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    ranked.truncate(MAX_TRIPLETS_PER_TERM);
    ranked.into_iter().map(|(_, triple)| triple).collect()
}

fn flatten_term(term_index: usize, contexts: &[Vec<Vec<Occurrence>>]) -> Vec<Occurrence> {
    let mut rows = Vec::new();
    for corpus_rows in &contexts[term_index] {
        rows.extend(corpus_rows.iter().cloned());
    }
    rows.sort_unstable_by(|a, b| a.selection_key.cmp(&b.selection_key));
    rows
}

fn excerpt_tokens(occurrence: &Occurrence) -> Vec<(String, bool)> {
    let bytes = occurrence.excerpt.as_bytes();
    let focal_start = occurrence
        .excerpt
        .find('[')
        .map(|index| index + 1)
        .unwrap_or(0);
    let focal_end = occurrence.excerpt[focal_start..]
        .find(']')
        .map(|index| focal_start + index)
        .unwrap_or(focal_start);
    let mut tokens = Vec::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        while cursor < bytes.len() && !bytes[cursor].is_ascii_alphanumeric() {
            cursor += 1;
        }
        let start = cursor;
        while cursor < bytes.len() && bytes[cursor].is_ascii_alphanumeric() {
            cursor += 1;
        }
        if cursor > start {
            let is_focal = start >= focal_start && cursor <= focal_end;
            tokens.push((
                occurrence.excerpt[start..cursor].to_ascii_lowercase(),
                is_focal,
            ));
        }
    }
    tokens
}

fn context_set(occurrence: &Occurrence, lemma_a: &str, lemma_b: &str) -> HashSet<String> {
    excerpt_tokens(occurrence)
        .into_iter()
        .filter(|(token, focal)| !*focal && token != lemma_a && token != lemma_b)
        .map(|(token, _)| token)
        .collect()
}

fn structural_signature(occurrence: &Occurrence, lemma_a: &str, lemma_b: &str) -> String {
    let tokens = excerpt_tokens(occurrence);
    let focal = tokens
        .iter()
        .position(|(_, is_focal)| *is_focal)
        .unwrap_or(0);
    let mut bins = [0u8; 6];
    for (index, (token, is_focal)) in tokens.iter().enumerate() {
        if *is_focal || token == lemma_a || token == lemma_b {
            continue;
        }
        if index < focal {
            let distance = focal - index;
            let bucket = if distance <= 2 {
                0
            } else if distance <= 6 {
                1
            } else {
                2
            };
            bins[bucket] = bins[bucket].saturating_add(1).min(3);
        } else {
            let distance = index - focal;
            let bucket = if distance <= 2 {
                3
            } else if distance <= 6 {
                4
            } else {
                5
            };
            bins[bucket] = bins[bucket].saturating_add(1).min(3);
        }
    }
    format!(
        "{}:{}:{}:{}:{}:{}:{}",
        occurrence.field, bins[0], bins[1], bins[2], bins[3], bins[4], bins[5]
    )
}

fn jaccard(left: &HashSet<String>, right: &HashSet<String>) -> f64 {
    if left.is_empty() && right.is_empty() {
        return 1.0;
    }
    let intersection = left.iter().filter(|token| right.contains(*token)).count();
    let union = left.len() + right.len() - intersection;
    if union == 0 {
        1.0
    } else {
        intersection as f64 / union as f64
    }
}

fn graph_metrics(
    contexts: &[Occurrence],
    lemma_a: &str,
    lemma_b: &str,
) -> Option<(usize, usize, f64, f64, f64)> {
    if contexts.len() != CONTEXTS_PER_CANDIDATE {
        return None;
    }
    let corpus_ids: HashSet<&str> = contexts.iter().map(|row| row.corpus_id.as_str()).collect();
    if corpus_ids.len() < 3 {
        return None;
    }
    let docs: HashSet<&str> = contexts
        .iter()
        .map(|row| row.document_sha256.as_str())
        .collect();
    if docs.len() != CONTEXTS_PER_CANDIDATE {
        return None;
    }
    let signatures: HashSet<String> = contexts
        .iter()
        .map(|row| structural_signature(row, lemma_a, lemma_b))
        .collect();
    let mut local_tokens = HashSet::<String>::new();
    let sets: Vec<HashSet<String>> = contexts
        .iter()
        .map(|row| context_set(row, lemma_a, lemma_b))
        .collect();
    for set in &sets {
        local_tokens.extend(set.iter().cloned());
    }
    if signatures.len() < 4 || local_tokens.len() < 24 {
        return None;
    }
    let mut similarities = Vec::<f64>::with_capacity(15);
    for left in 0..6 {
        for right in (left + 1)..6 {
            similarities.push(jaccard(&sets[left], &sets[right]));
        }
    }
    similarities.sort_by(f64::total_cmp);
    let q1 = similarities[3];
    let median = similarities[7];
    let q3 = similarities[11];
    if q3 - q1 < 0.10 {
        return None;
    }
    Some((signatures.len(), local_tokens.len(), q1, median, q3))
}

fn graph_for_candidate(
    candidate: &CandidatePair,
    term_map: &HashMap<String, usize>,
    contexts: &[Vec<Vec<Occurrence>>],
) -> Option<ChosenCandidate> {
    let index_a = *term_map.get(&candidate.lemma_a)?;
    let index_b = *term_map.get(&candidate.lemma_b)?;
    let occ_a = flatten_term(index_a, contexts);
    let occ_b = flatten_term(index_b, contexts);
    let triples_a = make_triplets(&occ_a, &candidate.lemma_a);
    let triples_b = make_triplets(&occ_b, &candidate.lemma_b);
    for a in &triples_a {
        for b in &triples_b {
            let mut selected = Vec::with_capacity(6);
            selected.extend(a.0.iter().map(|index| occ_a[*index].clone()));
            selected.extend(b.0.iter().map(|index| occ_b[*index].clone()));
            if let Some((signature_count, unique_tokens, q1, median, q3)) =
                graph_metrics(&selected, &candidate.lemma_a, &candidate.lemma_b)
            {
                return Some(ChosenCandidate {
                    candidate: candidate.clone(),
                    contexts: selected,
                    structural_signatures: signature_count,
                    unique_context_tokens: unique_tokens,
                    jaccard_q1: q1,
                    jaccard_median: median,
                    jaccard_q3: q3,
                });
            }
        }
    }
    None
}

pub fn select_candidate_graphs(
    pool: &[CandidatePair],
    term_map: &HashMap<String, usize>,
    contexts: &[Vec<Vec<Occurrence>>],
) -> CandidateSelection {
    let mut chosen = Vec::with_capacity(TARGET_CANDIDATES);
    let mut eligible_candidates = 0;
    let mut candidates_examined = 0;
    for candidate in pool {
        candidates_examined += 1;
        if let Some(graph) = graph_for_candidate(candidate, term_map, contexts) {
            eligible_candidates += 1;
            chosen.push(graph);
            if chosen.len() == TARGET_CANDIDATES {
                break;
            }
        }
    }
    CandidateSelection {
        chosen,
        eligible_candidates,
        candidates_examined,
    }
}

pub fn make_intake_report(
    pool: &[CandidatePair],
    selection: &CandidateSelection,
    _term_map: &HashMap<String, usize>,
    _contexts: &[Vec<Vec<Occurrence>>],
) -> CandidateIntakeReport {
    let selected_candidates = selection
        .chosen
        .iter()
        .map(|chosen| {
            let corpus_count = chosen
                .contexts
                .iter()
                .map(|row| row.corpus_id.as_str())
                .collect::<HashSet<_>>()
                .len();
            CandidateIntakeRow {
                candidate_id: chosen.candidate.candidate_id.clone(),
                lexical_pair: [
                    chosen.candidate.lemma_a.clone(),
                    chosen.candidate.lemma_b.clone(),
                ],
                contexts: chosen.contexts.len(),
                corpus_count,
                structural_signatures: chosen.structural_signatures,
                unique_non_candidate_context_tokens: chosen.unique_context_tokens,
                jaccard_q1: chosen.jaccard_q1,
                jaccard_median: chosen.jaccard_median,
                jaccard_q3: chosen.jaccard_q3,
            }
        })
        .collect();
    CandidateIntakeReport {
        schema: "phoenix.lexical.lt9-la2-p1o1-candidate-intake/v1",
        date: DATE,
        proposal_pool_size: pool.len(),
        eligible_candidates_encountered: selection.eligible_candidates,
        candidates_examined_in_hash_order: selection.candidates_examined,
        selected_candidates,
        packet_count: selection.chosen.len() * 15,
        edge_count_per_candidate: 15,
        selection_uses_human_labels: false,
    }
}

pub fn materialize_packets(chosen: &[ChosenCandidate]) -> Vec<PublicPacket> {
    let mut packets = Vec::with_capacity(chosen.len() * 15);
    for graph in chosen {
        let pair = [
            graph.candidate.lemma_a.clone(),
            graph.candidate.lemma_b.clone(),
        ];
        let mut edge_index = 0;
        for left in 0..6 {
            for right in (left + 1)..6 {
                let id = salted_hash(&[
                    graph.candidate.candidate_id.as_bytes(),
                    graph.contexts[left].node_id.as_bytes(),
                    graph.contexts[right].node_id.as_bytes(),
                    &(edge_index as u64).to_le_bytes(),
                ]);
                let reverse = id.as_bytes()[0] & 1 == 1;
                let (left_context, right_context) = if reverse {
                    (right, left)
                } else {
                    (left, right)
                };
                packets.push(PublicPacket {
                    packet_id: format!("o1-{}", &id[..16]),
                    lexical_pair: pair.clone(),
                    left_context: graph.contexts[left_context].excerpt.clone(),
                    right_context: graph.contexts[right_context].excerpt.clone(),
                    judgment: None,
                });
                edge_index += 1;
            }
        }
    }
    packets
}

pub fn materialize_private_rows(chosen: &[ChosenCandidate]) -> Vec<PrivateEdgeRow> {
    let mut rows = Vec::with_capacity(chosen.len() * 15);
    for graph in chosen {
        let pair = [
            graph.candidate.lemma_a.clone(),
            graph.candidate.lemma_b.clone(),
        ];
        let sets: Vec<HashSet<String>> = graph
            .contexts
            .iter()
            .map(|row| context_set(row, &pair[0], &pair[1]))
            .collect();
        let signatures: Vec<String> = graph
            .contexts
            .iter()
            .map(|row| structural_signature(row, &pair[0], &pair[1]))
            .collect();
        let mut edge_index = 0;
        for left in 0..6 {
            for right in (left + 1)..6 {
                let id = salted_hash(&[
                    graph.candidate.candidate_id.as_bytes(),
                    graph.contexts[left].node_id.as_bytes(),
                    graph.contexts[right].node_id.as_bytes(),
                    &(edge_index as u64).to_le_bytes(),
                ]);
                let reverse = id.as_bytes()[0] & 1 == 1;
                let (left_context, right_context) = if reverse {
                    (right, left)
                } else {
                    (left, right)
                };
                rows.push(PrivateEdgeRow {
                    packet_id: format!("o1-{}", &id[..16]),
                    candidate_id: graph.candidate.candidate_id.clone(),
                    edge_index,
                    lexical_pair: pair.clone(),
                    left: graph.contexts[left_context].clone(),
                    right: graph.contexts[right_context].clone(),
                    context_jaccard: jaccard(&sets[left_context], &sets[right_context]),
                    left_structural_signature: signatures[left_context].clone(),
                    right_structural_signature: signatures[right_context].clone(),
                });
                edge_index += 1;
            }
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::scan::document_hash;
    use super::*;

    fn occurrence(index: usize, term: &str, excerpt: &str, corpus_index: usize) -> Occurrence {
        Occurrence {
            node_id: format!("node-{index}"),
            term: term.to_owned(),
            corpus_index,
            corpus_id: format!("corpus-{corpus_index}"),
            document_id: format!("doc-{index}"),
            document_sha256: format!("hash-{index}"),
            field: "text".to_owned(),
            excerpt: excerpt.to_owned(),
            selection_key: format!("key-{index}"),
        }
    }

    #[test]
    fn context_features_exclude_focal_and_candidate_terms() {
        let row = occurrence(0, "car", "A [car] and vehicle travel on roads", 0);
        let tokens = context_set(&row, "car", "vehicle");
        assert_eq!(
            tokens,
            ["a", "and", "travel", "on", "roads"]
                .into_iter()
                .map(str::to_owned)
                .collect()
        );
    }

    #[test]
    fn public_packets_and_private_edges_have_identical_randomized_endpoints() {
        let candidate = CandidatePair {
            candidate_id: "candidate-1".to_owned(),
            lemma_a: "car".to_owned(),
            lemma_b: "automobile".to_owned(),
            priority_sha256: "0".repeat(64),
        };
        let contexts: Vec<Occurrence> = (0..6)
            .map(|index| {
                let term = if index < 3 { "car" } else { "automobile" };
                occurrence(
                    index,
                    term,
                    &format!("left{index} [{term}] context{index}"),
                    index % 3,
                )
            })
            .collect();
        let chosen = vec![ChosenCandidate {
            candidate,
            contexts,
            structural_signatures: 6,
            unique_context_tokens: 36,
            jaccard_q1: 0.0,
            jaccard_median: 0.0,
            jaccard_q3: 0.0,
        }];
        let public = materialize_packets(&chosen);
        let private = materialize_private_rows(&chosen);
        assert_eq!(public.len(), 15);
        assert_eq!(private.len(), 15);
        let ids: HashSet<&str> = public.iter().map(|row| row.packet_id.as_str()).collect();
        assert_eq!(ids.len(), 15);
        for (packet, edge) in public.iter().zip(private.iter()) {
            assert_eq!(packet.packet_id, edge.packet_id);
            assert_eq!(packet.left_context, edge.left.excerpt);
            assert_eq!(packet.right_context, edge.right.excerpt);
        }
    }

    #[test]
    fn document_hash_uses_the_prior_review_identity_contract() {
        let mut canonical = Sha256::new();
        canonical.update(b"title");
        canonical.update([0]);
        canonical.update(b"body");
        assert_eq!(document_hash("title", "body"), hex(&canonical.finalize()));
    }
}
