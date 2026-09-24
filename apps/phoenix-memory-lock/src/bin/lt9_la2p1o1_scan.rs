use super::*;
use anyhow::{Context, Result, ensure};
use memmap2::Mmap;
use serde::Deserialize;
use std::borrow::Cow;
use std::fs::File;
use std::path::Path;

#[derive(Deserialize)]
struct CorpusDoc<'a> {
    #[serde(rename = "_id")]
    id: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    title: Cow<'a, str>,
    #[serde(borrow, default)]
    text: Cow<'a, str>,
}

#[derive(Clone, Copy)]
struct TokenSpan {
    start: usize,
    end: usize,
}
fn token_hash(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for byte in bytes {
        h ^= byte.to_ascii_lowercase() as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn token_spans(bytes: &[u8]) -> Vec<TokenSpan> {
    let mut spans = Vec::new();
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
            spans.push(TokenSpan { start, end: cursor });
        }
    }
    spans
}

pub(super) fn document_hash(title: &str, text: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(title.as_bytes());
    digest.update([0]);
    digest.update(text.as_bytes());
    hex(&digest.finalize())
}

fn make_occurrence(
    corpus: &CorpusSpec,
    corpus_index: usize,
    term: &str,
    doc_id: &str,
    doc_hash: &str,
    field: &str,
    text: &str,
    spans: &[TokenSpan],
    focal: usize,
) -> Occurrence {
    let left = focal.saturating_sub(12);
    let right = (focal + 13).min(spans.len());
    let focal_span = spans[focal];
    let mut excerpt = String::new();
    if left < focal {
        excerpt.push_str(&text[spans[left].start..focal_span.start]);
    }
    excerpt.push('[');
    excerpt.push_str(&text[focal_span.start..focal_span.end]);
    excerpt.push(']');
    if focal + 1 < right {
        excerpt.push_str(&text[focal_span.end..spans[right - 1].end]);
    }
    let selection_key = salted_hash(&[
        term.as_bytes(),
        doc_hash.as_bytes(),
        field.as_bytes(),
        (focal as u64).to_le_bytes().as_slice(),
    ]);
    let node_id = selection_key[..16].to_owned();
    Occurrence {
        node_id,
        term: term.to_owned(),
        corpus_index,
        corpus_id: corpus.corpus_id.clone(),
        document_id: doc_id.to_owned(),
        document_sha256: doc_hash.to_owned(),
        field: field.to_owned(),
        excerpt,
        selection_key,
    }
}

pub fn scan_corpus(
    corpus: &CorpusSpec,
    corpus_index: usize,
    term_map: &HashMap<String, usize>,
    excluded_docs: &HashSet<String>,
    contexts: &mut [Vec<Vec<Occurrence>>],
) -> Result<CorpusScanReceipt> {
    let path = Path::new(&corpus.path);
    let file = File::open(path).with_context(|| format!("open corpus {}", path.display()))?;
    // SAFETY: read-only mapping remains valid while `file` is alive; this function never mutates it.
    let mapped =
        unsafe { Mmap::map(&file) }.with_context(|| format!("mmap corpus {}", path.display()))?;
    let actual_hash = sha256(&mapped);
    ensure!(
        actual_hash.eq_ignore_ascii_case(&corpus.sha256),
        "corpus hash mismatch for {}: expected {}, got {}",
        corpus.corpus_id,
        corpus.sha256,
        actual_hash
    );

    let mut by_token_hash: HashMap<u64, Vec<(usize, String)>> =
        HashMap::with_capacity(term_map.len());
    for (term, index) in term_map {
        by_token_hash
            .entry(token_hash(term.as_bytes()))
            .or_default()
            .push((*index, term.clone()));
    }
    let mut docs_scanned = 0u64;
    let mut prior_skipped = 0u64;
    let mut matched_doc_terms = 0u64;
    let mut retained = 0u64;
    for raw_line in mapped.split(|b| *b == b'\n') {
        let line = raw_line.strip_suffix(b"\r").unwrap_or(raw_line);
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let doc: CorpusDoc<'_> = serde_json::from_slice(line).with_context(|| {
            format!(
                "parse {} document line {}",
                corpus.corpus_id,
                docs_scanned + prior_skipped + 1
            )
        })?;
        docs_scanned += 1;
        let title = doc.title.as_ref();
        let text = doc.text.as_ref();
        let doc_hash = document_hash(title, text);
        if excluded_docs.contains(&doc_hash) {
            prior_skipped += 1;
            continue;
        }
        let doc_id = doc.id.as_deref().unwrap_or("");
        let mut doc_matches: HashMap<usize, Occurrence> = HashMap::new();
        for (field, body) in [("title", title), ("text", text)] {
            let spans = token_spans(body.as_bytes());
            for (position, span) in spans.iter().enumerate() {
                let word = &body.as_bytes()[span.start..span.end];
                let Some(candidates) = by_token_hash.get(&token_hash(word)) else {
                    continue;
                };
                for (term_index, term) in candidates {
                    if !word.eq_ignore_ascii_case(term.as_bytes()) {
                        continue;
                    }
                    let occurrence = make_occurrence(
                        corpus,
                        corpus_index,
                        term,
                        doc_id,
                        &doc_hash,
                        field,
                        body,
                        &spans,
                        position,
                    );
                    match doc_matches.get(term_index) {
                        Some(previous) if previous.selection_key <= occurrence.selection_key => {}
                        _ => {
                            doc_matches.insert(*term_index, occurrence);
                        }
                    }
                }
            }
        }
        matched_doc_terms += doc_matches.len() as u64;
        for (term_index, occurrence) in doc_matches {
            let reservoir = &mut contexts[term_index][corpus_index];
            if reservoir.len() < MAX_OCCURRENCES_PER_TERM_CORPUS {
                reservoir.push(occurrence);
                retained += 1;
            } else if let Some((largest, _)) = reservoir
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.selection_key.cmp(&b.1.selection_key))
            {
                if occurrence.selection_key < reservoir[largest].selection_key {
                    reservoir[largest] = occurrence;
                }
            }
        }
    }
    Ok(CorpusScanReceipt {
        corpus_id: corpus.corpus_id.clone(),
        corpus_sha256_expected: corpus.sha256.clone(),
        corpus_sha256_actual: actual_hash.clone(),
        hash_verified: actual_hash.eq_ignore_ascii_case(&corpus.sha256),
        documents_scanned: docs_scanned,
        prior_review_documents_skipped: prior_skipped,
        matched_document_terms: matched_doc_terms,
        retained_occurrences: retained,
    })
}
