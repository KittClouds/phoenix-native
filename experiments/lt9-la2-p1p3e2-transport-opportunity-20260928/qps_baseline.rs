//! Frozen BM25F/QPS baseline rank pass for P1P3E2 counterpart candidates.
//!
//! Compile this source in the isolated QPS validation harness described in
//! `BASELINE_BUILD.md`; the root workspace currently has an unrelated missing
//! TTS member. This binary only ranks the label-blind qrels candidate rows.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::env;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use hashbrown::HashMap;
use memchr::memchr_iter;
use memmap2::MmapOptions;
use phoenix_lexical_qps::{
    DocumentInput, FieldConfig, MAXIMUM_QUERY_GROUPS, QpsBuilder, QpsConfig, QpsError,
    SearchScratch, QpsIndex,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const TOP_K: usize = 100;

#[derive(Debug, Deserialize)]
struct Cohort {
    datasets: Vec<DatasetSpec>,
}

#[derive(Clone, Debug, Deserialize)]
struct DatasetSpec {
    name: String,
    root: PathBuf,
}

#[derive(Debug, Deserialize)]
struct ScreenReceipt {
    input_hashes: Vec<InputHash>,
}

#[derive(Debug, Deserialize)]
struct InputHash {
    path: String,
    sha256: String,
    bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CandidateRow {
    dataset: String,
    qrels_splits: Vec<String>,
    query_id: String,
    document_id: String,
    document_ordinal: u64,
    candidate_id: String,
    lexical_pair: [String; 2],
    direction: String,
    transport_status: String,
    qrels_grade: i32,
    partition: String,
    query_text: String,
    document_title: String,
    document_excerpt: String,
}

#[derive(Debug, Deserialize)]
struct CorpusRow<'a> {
    #[serde(default, borrow)]
    title: Option<Cow<'a, str>>,
    #[serde(default, borrow)]
    text: Option<Cow<'a, str>>,
}

#[derive(Clone, Debug, Serialize)]
struct RankedCandidate {
    candidate: CandidateRow,
    baseline_rank: Option<usize>,
    baseline_status: &'static str,
}

#[derive(Debug, Serialize)]
struct DatasetRankSummary {
    name: String,
    corpus_documents: u64,
    query_count: usize,
    candidate_rows: usize,
    missed_top100: usize,
    underranked_11_100: usize,
    already_top10: usize,
    unsupported_queries_too_large: usize,
    unsupported_candidate_rows: usize,
    corpus_sha256: String,
    corpus_bytes: u64,
}

#[derive(Debug, Serialize)]
struct RelationRankSummary {
    candidate_id: String,
    candidate_rows: usize,
    missed_top100: usize,
    underranked_11_100: usize,
    already_top10: usize,
    unsupported_candidate_rows: usize,
}

#[derive(Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    date: &'static str,
    status: &'static str,
    interpretation: &'static str,
    qrels_candidate_file_sha256: String,
    qrels_screen_receipt_sha256: String,
    qps_config: &'static str,
    top_k: usize,
    datasets: Vec<DatasetRankSummary>,
    relations: Vec<RelationRankSummary>,
    candidate_rows: usize,
    missed_top100: usize,
    underranked_11_100: usize,
    already_top10: usize,
    unsupported_queries_too_large: usize,
    unsupported_candidate_rows: usize,
    qps_dependency_note: &'static str,
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn input_hash_for(receipt: &ScreenReceipt, path: &Path) -> Result<(String, u64)> {
    let path = path.display().to_string();
    let row = receipt
        .input_hashes
        .iter()
        .find(|row| row.path == path)
        .with_context(|| format!("screen receipt does not bind corpus {path}"))?;
    Ok((row.sha256.clone(), row.bytes))
}

fn build_index(path: &Path, expected_hash: &str) -> Result<(QpsIndex, u64, String, u64)> {
    let file = File::open(path).with_context(|| format!("open corpus {}", path.display()))?;
    // SAFETY: all mapped corpus inputs are frozen, read-only benchmark artifacts.
    let map = unsafe { MmapOptions::new().map(&file) }
        .with_context(|| format!("map corpus {}", path.display()))?;
    let fields = [
        FieldConfig::new("title", 2.5, 0.35, 0.0),
        FieldConfig::new("body", 1.0, 0.75, 0.0),
    ];
    let config = QpsConfig {
        maximum_query_groups: MAXIMUM_QUERY_GROUPS,
        maximum_candidate_pool: 256,
        proximity_weight: 0.0,
        order_weight: 0.0,
        phrase_weight: 0.0,
        segment_weight: 0.0,
        ..QpsConfig::default()
    };
    let mut builder = QpsBuilder::new(Vec::from(fields).into_boxed_slice(), config)?;
    let mut hasher = Sha256::new();
    let mut documents = 0_u64;
    let mut row_start = 0_usize;
    for newline in memchr_iter(b'\n', &map).chain(std::iter::once(map.len())) {
        if newline == row_start {
            if newline < map.len() {
                hasher.update(&map[newline..newline + 1]);
            }
            row_start = newline.saturating_add(1);
            continue;
        }
        let hashed_end = if newline < map.len() { newline + 1 } else { newline };
        hasher.update(&map[row_start..hashed_end]);
        let line = map
            .get(row_start..newline)
            .context("invalid corpus mmap line range")?;
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let row: CorpusRow<'_> = serde_json::from_slice(line)
            .with_context(|| format!("decode corpus line {}", documents + 1))?;
        let title = row.title.as_deref().unwrap_or_default();
        let body = row.text.as_deref().unwrap_or_default();
        builder.insert(DocumentInput {
            external_id: documents,
            fields: &[title, body],
        })?;
        documents += 1;
        row_start = newline.saturating_add(1);
        if newline == map.len() {
            break;
        }
    }
    let observed_hash = format!("{:x}", hasher.finalize());
    if observed_hash != expected_hash {
        bail!("corpus hash changed since candidate screen: {}", path.display());
    }
    let bytes = map.len() as u64;
    let index = builder.build()?;
    Ok((index, documents, observed_hash, bytes))
}

fn status(rank: Option<usize>) -> &'static str {
    match rank {
        None => "MISSED_TOP100",
        Some(1..=10) => "ALREADY_TOP10",
        Some(11..=100) => "UNDERRANKED_11_100",
        Some(_) => "MISSED_TOP100",
    }
}

enum QueryRank {
    Ranked(HashMap<u64, usize>),
    UnsupportedTooLarge,
}

fn main() -> Result<()> {
    let args: Vec<_> = env::args_os().collect();
    if args.len() != 5 {
        bail!("usage: p1p3e2-qps-baseline <cohort.json> <screen-receipt.json> <candidates.jsonl> <new-output-dir>");
    }
    let cohort_path = PathBuf::from(&args[1]);
    let screen_receipt_path = PathBuf::from(&args[2]);
    let candidates_path = PathBuf::from(&args[3]);
    let output_dir = PathBuf::from(&args[4]);
    if output_dir.exists() {
        bail!("refusing existing output directory {}", output_dir.display());
    }
    let cohort: Cohort = serde_json::from_slice(&fs::read(&cohort_path)?)?;
    let screen_bytes = fs::read(&screen_receipt_path)?;
    let screen_hash = sha256(&screen_bytes);
    let screen: ScreenReceipt = serde_json::from_slice(&screen_bytes)?;
    let candidate_bytes = fs::read(&candidates_path)?;
    let candidate_hash = sha256(&candidate_bytes);
    let mut candidates = Vec::new();
    for (line_number, line) in candidate_bytes.split(|byte| *byte == b'\n').enumerate() {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        candidates.push(
            serde_json::from_slice::<CandidateRow>(line)
                .with_context(|| format!("decode candidate row {}", line_number + 1))?,
        );
    }
    if candidates.is_empty() {
        bail!("candidate bank is empty; do not build a baseline index");
    }
    fs::create_dir_all(&output_dir)?;
    let ranked_path = output_dir.join("ranked-opportunity-candidates.jsonl");
    let mut ranked_out = BufWriter::new(File::create(&ranked_path)?);
    let mut dataset_summaries = Vec::new();
    let mut relation_counts: BTreeMap<String, (usize, usize, usize, usize, usize)> =
        BTreeMap::new();
    let mut total_missed = 0;
    let mut total_underranked = 0;
    let mut total_top10 = 0;
    let mut total_unsupported = 0;
    let mut total_unsupported_queries = 0;
    for spec in &cohort.datasets {
        let mut rows: Vec<_> = candidates
            .iter()
            .filter(|row| row.dataset == spec.name)
            .cloned()
            .collect();
        if rows.is_empty() {
            continue;
        }
        let dataset_candidate_rows = rows.len();
        eprintln!("P1P3E2 QPS index start: {} candidate_rows={}", spec.name, rows.len());
        let corpus_path = spec.root.join("corpus.jsonl");
        let (expected_hash, _) = input_hash_for(&screen, &corpus_path)?;
        let (index, corpus_documents, corpus_hash, corpus_bytes) =
            build_index(&corpus_path, &expected_hash)?;
        eprintln!("P1P3E2 QPS index ready: {} docs={}", spec.name, corpus_documents);
        let mut query_texts = BTreeMap::<String, String>::new();
        for row in &rows {
            query_texts
                .entry(row.query_id.clone())
                .or_insert_with(|| row.query_text.clone());
        }
        let mut ranked_queries = HashMap::<String, QueryRank>::with_capacity(query_texts.len());
        let mut scratch = SearchScratch::default();
        let mut hits = Vec::with_capacity(TOP_K);
        for (query_id, query_text) in &query_texts {
            hits.clear();
            match index.search_evidence_into(query_text, TOP_K, &mut scratch, &mut hits) {
                Ok(_) => {
                    let ranks = hits
                        .iter()
                        .enumerate()
                        .map(|(rank, hit)| (hit.external_id, rank + 1))
                        .collect();
                    ranked_queries.insert(query_id.clone(), QueryRank::Ranked(ranks));
                }
                Err(QpsError::QueryTooLarge) => {
                    ranked_queries.insert(query_id.clone(), QueryRank::UnsupportedTooLarge);
                }
                Err(error) => return Err(error.into()),
            }
        }
        let unsupported_queries = ranked_queries
            .values()
            .filter(|row| matches!(row, QueryRank::UnsupportedTooLarge))
            .count();
        let mut missed = 0;
        let mut underranked = 0;
        let mut top10 = 0;
        let mut unsupported = 0;
        rows.sort_by(|left, right| {
            (&left.query_id, &left.document_ordinal, &left.candidate_id)
                .cmp(&(&right.query_id, &right.document_ordinal, &right.candidate_id))
        });
        for row in rows {
            let query_rank = ranked_queries
                .get(&row.query_id)
                .with_context(|| format!("missing ranked query {}", row.query_id))?;
            let (rank, row_status) = match query_rank {
                QueryRank::Ranked(ranks) => {
                    let rank = ranks.get(&row.document_ordinal).copied();
                    (rank, status(rank))
                }
                QueryRank::UnsupportedTooLarge => (None, "QUERY_UNSUPPORTED_TOO_LARGE"),
            };
            match row_status {
                "MISSED_TOP100" => missed += 1,
                "UNDERRANKED_11_100" => underranked += 1,
                "ALREADY_TOP10" => top10 += 1,
                "QUERY_UNSUPPORTED_TOO_LARGE" => unsupported += 1,
                _ => unreachable!("unknown baseline status"),
            }
            let counts = relation_counts.entry(row.candidate_id.clone()).or_default();
            counts.0 += 1;
            match row_status {
                "MISSED_TOP100" => counts.1 += 1,
                "UNDERRANKED_11_100" => counts.2 += 1,
                "ALREADY_TOP10" => counts.3 += 1,
                "QUERY_UNSUPPORTED_TOO_LARGE" => counts.4 += 1,
                _ => unreachable!("unknown baseline status"),
            }
            serde_json::to_writer(
                &mut ranked_out,
                &RankedCandidate {
                    candidate: row,
                    baseline_rank: rank,
                    baseline_status: row_status,
                },
            )?;
            ranked_out.write_all(b"\n")?;
        }
        total_missed += missed;
        total_underranked += underranked;
        total_top10 += top10;
        total_unsupported += unsupported;
        total_unsupported_queries += unsupported_queries;
        dataset_summaries.push(DatasetRankSummary {
            name: spec.name.clone(),
            corpus_documents,
            query_count: query_texts.len(),
            candidate_rows: dataset_candidate_rows,
            missed_top100: missed,
            underranked_11_100: underranked,
            already_top10: top10,
            unsupported_queries_too_large: unsupported_queries,
            unsupported_candidate_rows: unsupported,
            corpus_sha256: corpus_hash,
            corpus_bytes,
        });
        drop(index);
    }
    ranked_out.flush()?;
    let relations = relation_counts
        .into_iter()
        .map(|(candidate_id, counts)| RelationRankSummary {
            candidate_id,
            candidate_rows: counts.0,
            missed_top100: counts.1,
            underranked_11_100: counts.2,
            already_top10: counts.3,
            unsupported_candidate_rows: counts.4,
        })
        .collect();
    let receipt = Receipt {
        schema: "phoenix.lexical.lt9-la2-p1p3e2-qps-baseline/v1",
        date: "2026-09-28",
        status: "QPS_BASELINE_RANKED_CANDIDATE_OPPORTUNITIES_NOT_TRANSPORT_AUTHORITY",
        interpretation: "Ranks positive-qrels counterpart candidates using the frozen BM25F/QPS baseline. Only missed top-100 or ranks 11-100 are bank opportunities; qrels relevance does not certify lexical compatibility.",
        qrels_candidate_file_sha256: candidate_hash,
        qrels_screen_receipt_sha256: screen_hash,
        qps_config: "title weight=2.5 b=0.35; body weight=1.0 b=0.75; proximity/order/phrase/segment=0",
        top_k: TOP_K,
        datasets: dataset_summaries,
        relations,
        candidate_rows: candidates.len(),
        missed_top100: total_missed,
        underranked_11_100: total_underranked,
        already_top10: total_top10,
        unsupported_queries_too_large: total_unsupported_queries,
        unsupported_candidate_rows: total_unsupported,
        qps_dependency_note: "Built against the isolated copy of the frozen phoenix-lexical-qps crate; hash that source copy in the execution manifest.",
    };
    serde_json::to_writer_pretty(
        BufWriter::new(File::create(output_dir.join("qps-baseline-receipt.json"))?),
        &receipt,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::status;

    #[test]
    fn rank_buckets_match_frozen_serving_boundary() {
        assert_eq!(status(None), "MISSED_TOP100");
        assert_eq!(status(Some(1)), "ALREADY_TOP10");
        assert_eq!(status(Some(10)), "ALREADY_TOP10");
        assert_eq!(status(Some(11)), "UNDERRANKED_11_100");
        assert_eq!(status(Some(100)), "UNDERRANKED_11_100");
        assert_eq!(status(Some(101)), "MISSED_TOP100");
    }
}
