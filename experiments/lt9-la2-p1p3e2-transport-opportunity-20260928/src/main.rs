use std::borrow::Cow;
use std::collections::BTreeMap;
use std::env;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use hashbrown::{HashMap, HashSet};
use memchr::memchr_iter;
use memmap2::MmapOptions;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const SPLIT_SALT: &str = "P1P3E2-20260928";

#[derive(Debug, Deserialize)]
struct Cohort {
    schema: String,
    date: String,
    datasets: Vec<DatasetSpec>,
}

#[derive(Clone, Debug, Deserialize)]
struct DatasetSpec {
    name: String,
    root: PathBuf,
    qrels_splits: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct CandidateInventory {
    schema: String,
    date: String,
    source: String,
    authority_status: String,
    relations: Vec<RelationSpec>,
    negative_support_rule: String,
}

#[derive(Clone, Debug, Deserialize)]
struct RelationSpec {
    pair_id: String,
    source: String,
    target: String,
    also_screen_reverse: bool,
    transport_status: String,
}

#[derive(Clone, Debug, Serialize)]
struct Direction {
    id: String,
    pair_id: String,
    source: String,
    target: String,
    transport_status: String,
}

#[derive(Debug, Deserialize)]
struct QueryRow {
    #[serde(rename = "_id")]
    id: String,
    #[serde(default)]
    text: String,
}

#[derive(Debug, Deserialize)]
struct CorpusRow<'a> {
    #[serde(rename = "_id")]
    id: Cow<'a, str>,
    #[serde(default, borrow)]
    title: Option<Cow<'a, str>>,
    #[serde(default, borrow)]
    text: Option<Cow<'a, str>>,
}

#[derive(Clone, Debug)]
struct Seed {
    qid: String,
    did: String,
    direction: Direction,
    query_text: String,
    relevance: i32,
    qrels_splits: HashSet<String>,
}

#[derive(Debug, Serialize)]
struct OpportunityCandidate {
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
    partition: &'static str,
    query_text: String,
    document_title: String,
    document_excerpt: String,
}

#[derive(Debug, Serialize)]
struct InputHash {
    path: String,
    sha256: String,
    bytes: u64,
}

#[derive(Debug, Serialize)]
struct DatasetSummary {
    name: String,
    corpus_documents: u64,
    positive_qrels_rows: u64,
    source_query_rows: u64,
    source_query_relevant_doc_pairs: u64,
    exact_counterpart_doc_pairs: u64,
    fit_candidates: u64,
    holdout_candidates: u64,
    cross_partition_candidates_discarded: u64,
}

#[derive(Debug, Serialize)]
struct RelationSummary {
    candidate_id: String,
    direction: String,
    transport_status: String,
    candidate_rows: u64,
    fit_rows: u64,
    holdout_rows: u64,
}

#[derive(Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    date: &'static str,
    status: &'static str,
    interpretation: &'static str,
    protocol_sha256: String,
    cohort_sha256: String,
    candidate_inventory_sha256: String,
    runner_source_sha256: String,
    runner_binary_sha256: String,
    candidate_inventory_source: String,
    negative_support_rule: String,
    candidate_rows_sha256: String,
    candidate_rows_bytes: u64,
    split_salt: &'static str,
    candidate_directions: Vec<Direction>,
    input_hashes: Vec<InputHash>,
    datasets: Vec<DatasetSummary>,
    relations: Vec<RelationSummary>,
    candidate_rows_total: u64,
    fit_rows_total: u64,
    holdout_rows_total: u64,
    baseline_rank_status: &'static str,
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn sha256_path(path: &Path) -> Result<(String, u64)> {
    let mut reader =
        BufReader::new(File::open(path).with_context(|| format!("open {}", path.display()))?);
    let mut digest = Sha256::new();
    let mut bytes = 0_u64;
    // Keep the chunk on the heap: Windows main-thread stacks are much smaller
    // than a megabyte once the caller's frame is included.
    let mut buffer = vec![0_u8; 256 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
        bytes += count as u64;
    }
    Ok((format!("{:x}", digest.finalize()), bytes))
}

fn exact_token_contains(text: &str, needle: &str) -> bool {
    let mut start = None;
    for (index, character) in text.char_indices() {
        if character.is_alphanumeric() || character == '_' {
            start.get_or_insert(index);
        } else if let Some(begin) = start.take() {
            if text[begin..index].eq_ignore_ascii_case(needle) {
                return true;
            }
        }
    }
    start.is_some_and(|begin| text[begin..].eq_ignore_ascii_case(needle))
}

fn partition(namespace: char, dataset: &str, id: &str) -> &'static str {
    let identity = format!("{SPLIT_SALT}|{namespace}|{dataset}|{id}");
    let digest = Sha256::digest(identity.as_bytes());
    if digest[0] % 10 < 8 {
        "FIT"
    } else {
        "HOLDOUT"
    }
}

fn directions(inventory: &CandidateInventory) -> Vec<Direction> {
    let mut result = Vec::with_capacity(inventory.relations.len() * 2);
    for relation in &inventory.relations {
        result.push(Direction {
            id: format!(
                "{}:{}->{}",
                relation.pair_id, relation.source, relation.target
            ),
            pair_id: relation.pair_id.clone(),
            source: relation.source.clone(),
            target: relation.target.clone(),
            transport_status: relation.transport_status.clone(),
        });
        if relation.also_screen_reverse {
            result.push(Direction {
                id: format!(
                    "{}:{}->{}",
                    relation.pair_id, relation.target, relation.source
                ),
                pair_id: relation.pair_id.clone(),
                source: relation.target.clone(),
                target: relation.source.clone(),
                transport_status: relation.transport_status.clone(),
            });
        }
    }
    result
}

fn excerpt(text: &str, target: &str) -> String {
    let lower = text.to_lowercase();
    let target_lower = target.to_lowercase();
    let Some(byte_start) = lower.find(&target_lower) else {
        return text.chars().take(240).collect();
    };
    let char_start = text[..byte_start].chars().count().saturating_sub(120);
    let char_end = text[byte_start..]
        .chars()
        .count()
        .saturating_add(120)
        .min(text.chars().count());
    text.chars()
        .skip(char_start)
        .take(char_end - char_start)
        .collect()
}

fn read_queries(path: &Path) -> Result<(HashMap<String, String>, InputHash)> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let hash = InputHash {
        path: path.display().to_string(),
        sha256: sha256(&bytes),
        bytes: bytes.len() as u64,
    };
    let mut result = HashMap::new();
    for (line_index, line) in bytes.split(|value| *value == b'\n').enumerate() {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        let row: QueryRow = serde_json::from_slice(line)
            .with_context(|| format!("decode query {} line {}", path.display(), line_index + 1))?;
        result.insert(row.id, row.text);
    }
    Ok((result, hash))
}

fn qrels_seeds(
    path: &Path,
    queries: &HashMap<String, String>,
    directions: &[Direction],
    by_doc: &mut HashMap<String, Vec<Seed>>,
) -> Result<(u64, u64, u64, InputHash)> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let hash = InputHash {
        path: path.display().to_string(),
        sha256: sha256(&bytes),
        bytes: bytes.len() as u64,
    };
    let mut positive_rows = 0_u64;
    let mut source_query_rows = 0_u64;
    let mut source_doc_pairs = 0_u64;
    for (line_index, line) in bytes.split(|value| *value == b'\n').enumerate() {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() || line.starts_with(b"query-id") {
            continue;
        }
        let text = std::str::from_utf8(line).context("qrels line is not UTF-8")?;
        let mut columns = text.split('\t');
        let qid = columns.next().context("qrels query id missing")?;
        let did = columns.next().context("qrels document id missing")?;
        let score = columns
            .next()
            .context("qrels score missing")?
            .parse::<i32>()
            .with_context(|| {
                format!(
                    "invalid qrels score at {}:{}",
                    path.display(),
                    line_index + 1
                )
            })?;
        if score <= 0 {
            continue;
        }
        positive_rows += 1;
        let Some(query) = queries.get(qid) else {
            continue;
        };
        let query_dirs = directions
            .iter()
            .filter(|direction| exact_token_contains(query, &direction.source));
        let matching: Vec<&Direction> = query_dirs.collect();
        if matching.is_empty() {
            continue;
        }
        source_query_rows += 1;
        for direction in matching {
            source_doc_pairs += 1;
            let existing = by_doc.get(did).and_then(|seeds| {
                seeds
                    .iter()
                    .position(|seed| seed.qid == qid && seed.direction.id == direction.id)
            });
            if let Some(index) = existing {
                let seed = &mut by_doc.get_mut(did).expect("entry observed above")[index];
                seed.relevance = seed.relevance.max(score);
                seed.qrels_splits.insert(
                    path.file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                );
            } else {
                let mut qrels_splits = HashSet::new();
                qrels_splits.insert(
                    path.file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                );
                by_doc.entry(did.to_owned()).or_default().push(Seed {
                    qid: qid.to_owned(),
                    did: did.to_owned(),
                    direction: direction.clone(),
                    query_text: query.clone(),
                    relevance: score,
                    qrels_splits,
                });
            }
        }
    }
    Ok((positive_rows, source_query_rows, source_doc_pairs, hash))
}

fn scan_dataset(
    spec: &DatasetSpec,
    directions: &[Direction],
    candidates_out: &mut BufWriter<File>,
    input_hashes: &mut Vec<InputHash>,
    relation_counts: &mut BTreeMap<String, (u64, u64, u64, String, String)>,
) -> Result<DatasetSummary> {
    let queries_path = spec.root.join("queries.jsonl");
    let corpus_path = spec.root.join("corpus.jsonl");
    let (queries, query_hash) = read_queries(&queries_path)?;
    input_hashes.push(query_hash);
    let mut by_doc: HashMap<String, Vec<Seed>> = HashMap::new();
    let mut positive_qrels_rows = 0;
    let mut source_query_rows = 0;
    let mut source_doc_pairs = 0;
    for split in &spec.qrels_splits {
        let qrels_path = spec.root.join("qrels").join(format!("{split}.tsv"));
        if !qrels_path.is_file() {
            bail!("missing frozen qrels input {}", qrels_path.display());
        }
        let (positive, source_rows, pairs, hash) =
            qrels_seeds(&qrels_path, &queries, directions, &mut by_doc)?;
        positive_qrels_rows += positive;
        source_query_rows += source_rows;
        source_doc_pairs += pairs;
        input_hashes.push(hash);
    }

    let corpus_file =
        File::open(&corpus_path).with_context(|| format!("open {}", corpus_path.display()))?;
    // SAFETY: this is a read-only mapping; the corpus files are frozen inputs for this run.
    let corpus_map = unsafe { MmapOptions::new().map(&corpus_file) }
        .with_context(|| format!("map {}", corpus_path.display()))?;
    let mut corpus_hasher = Sha256::new();
    let mut corpus_documents = 0_u64;
    let mut exact_counterpart_doc_pairs = 0_u64;
    let mut fit_candidates = 0_u64;
    let mut holdout_candidates = 0_u64;
    let mut cross_partition_candidates_discarded = 0_u64;
    let mut row_start = 0_usize;
    for newline in memchr_iter(b'\n', &corpus_map).chain(std::iter::once(corpus_map.len())) {
        if newline == row_start {
            if newline < corpus_map.len() {
                corpus_hasher.update(&corpus_map[newline..newline + 1]);
            }
            row_start = newline.saturating_add(1);
            continue;
        }
        let hashed_end = if newline < corpus_map.len() {
            newline + 1
        } else {
            newline
        };
        corpus_hasher.update(&corpus_map[row_start..hashed_end]);
        let line = corpus_map
            .get(row_start..newline)
            .context("invalid mmap line range")?;
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let row: CorpusRow<'_> = serde_json::from_slice(line).with_context(|| {
            format!(
                "decode {} document {}",
                corpus_path.display(),
                corpus_documents
            )
        })?;
        let ordinal = corpus_documents;
        corpus_documents += 1;
        if let Some(seeds) = by_doc.get(row.id.as_ref()) {
            let title = row.title.as_deref().unwrap_or_default();
            let body = row.text.as_deref().unwrap_or_default();
            for seed in seeds {
                let has_source = exact_token_contains(title, &seed.direction.source)
                    || exact_token_contains(body, &seed.direction.source);
                let has_target = exact_token_contains(title, &seed.direction.target)
                    || exact_token_contains(body, &seed.direction.target);
                if !has_target || has_source {
                    continue;
                }
                exact_counterpart_doc_pairs += 1;
                let query_partition = partition('Q', &spec.name, &seed.qid);
                let document_partition = partition('D', &spec.name, &seed.did);
                if query_partition != document_partition {
                    cross_partition_candidates_discarded += 1;
                    continue;
                }
                let split = query_partition;
                if split == "FIT" {
                    fit_candidates += 1;
                } else {
                    holdout_candidates += 1;
                }
                let key = format!(
                    "{}:{}->{}",
                    seed.direction.pair_id, seed.direction.source, seed.direction.target
                );
                let counts = relation_counts.entry(key.clone()).or_insert((
                    0,
                    0,
                    0,
                    seed.direction.transport_status.clone(),
                    format!("{}->{}", seed.direction.source, seed.direction.target),
                ));
                counts.0 += 1;
                if split == "FIT" {
                    counts.1 += 1;
                } else {
                    counts.2 += 1;
                }
                let document_excerpt = if exact_token_contains(title, &seed.direction.target) {
                    excerpt(title, &seed.direction.target)
                } else {
                    excerpt(body, &seed.direction.target)
                };
                let pair = if seed.direction.source <= seed.direction.target {
                    [seed.direction.source.clone(), seed.direction.target.clone()]
                } else {
                    [seed.direction.target.clone(), seed.direction.source.clone()]
                };
                let mut qrels_splits: Vec<_> = seed.qrels_splits.iter().cloned().collect();
                qrels_splits.sort();
                serde_json::to_writer(
                    &mut *candidates_out,
                    &OpportunityCandidate {
                        dataset: spec.name.clone(),
                        qrels_splits,
                        query_id: seed.qid.clone(),
                        document_id: seed.did.clone(),
                        document_ordinal: ordinal,
                        candidate_id: key,
                        lexical_pair: pair,
                        direction: format!("{}->{}", seed.direction.source, seed.direction.target),
                        transport_status: seed.direction.transport_status.clone(),
                        qrels_grade: seed.relevance,
                        partition: split,
                        query_text: seed.query_text.clone(),
                        document_title: title.to_owned(),
                        document_excerpt,
                    },
                )?;
                candidates_out.write_all(b"\n")?;
            }
        }
        row_start = newline.saturating_add(1);
        if newline == corpus_map.len() {
            break;
        }
    }
    input_hashes.push(InputHash {
        path: corpus_path.display().to_string(),
        sha256: format!("{:x}", corpus_hasher.finalize()),
        bytes: corpus_map.len() as u64,
    });
    Ok(DatasetSummary {
        name: spec.name.clone(),
        corpus_documents,
        positive_qrels_rows,
        source_query_rows,
        source_query_relevant_doc_pairs: source_doc_pairs,
        exact_counterpart_doc_pairs,
        fit_candidates,
        holdout_candidates,
        cross_partition_candidates_discarded,
    })
}

fn main() -> Result<()> {
    let args: Vec<_> = env::args_os().collect();
    if args.len() != 5 {
        bail!("usage: p1p3e2-opportunity-screen <protocol.md> <cohort.json> <candidate-relations.json> <new-output-dir>");
    }
    let protocol_path = PathBuf::from(&args[1]);
    let cohort_path = PathBuf::from(&args[2]);
    let relations_path = PathBuf::from(&args[3]);
    let output_dir = PathBuf::from(&args[4]);
    if output_dir.exists() {
        bail!(
            "refusing existing output directory {}",
            output_dir.display()
        );
    }
    let protocol_bytes = fs::read(&protocol_path)?;
    let cohort_bytes = fs::read(&cohort_path)?;
    let relations_bytes = fs::read(&relations_path)?;
    let cohort: Cohort = serde_json::from_slice(&cohort_bytes)?;
    let inventory: CandidateInventory = serde_json::from_slice(&relations_bytes)?;
    if cohort.schema != "phoenix.lexical.lt9-la2-p1p3e2-corpus-cohort/v1"
        || inventory.schema != "phoenix.lexical.lt9-la2-p1p3e2-candidate-inventory/v1"
        || cohort.date != "2026-09-28"
        || inventory.date != "2026-09-28"
        || inventory.authority_status != "CANDIDATE_ONLY_NOT_PROMOTED"
    {
        bail!("unexpected or unfrozen cohort/candidate inventory");
    }
    let candidate_directions = directions(&inventory);
    fs::create_dir_all(&output_dir)?;
    let candidates_path = output_dir.join("qrels-counterpart-candidates.jsonl");
    let candidate_file = File::create(&candidates_path)?;
    let mut candidates_out = BufWriter::new(candidate_file);
    let mut input_hashes = vec![
        InputHash {
            path: protocol_path.display().to_string(),
            sha256: sha256(&protocol_bytes),
            bytes: protocol_bytes.len() as u64,
        },
        InputHash {
            path: cohort_path.display().to_string(),
            sha256: sha256(&cohort_bytes),
            bytes: cohort_bytes.len() as u64,
        },
        InputHash {
            path: relations_path.display().to_string(),
            sha256: sha256(&relations_bytes),
            bytes: relations_bytes.len() as u64,
        },
    ];
    let mut datasets = Vec::with_capacity(cohort.datasets.len());
    let mut relation_counts = BTreeMap::new();
    for spec in &cohort.datasets {
        eprintln!("P1P3E2 scan start: {}", spec.name);
        datasets.push(
            scan_dataset(
                spec,
                &candidate_directions,
                &mut candidates_out,
                &mut input_hashes,
                &mut relation_counts,
            )
            .with_context(|| format!("scan dataset {}", spec.name))?,
        );
        let summary = datasets.last().expect("just pushed dataset summary");
        eprintln!(
            "P1P3E2 scan done: {} docs={} candidates={} fit={} holdout={} cross-split-dropped={}",
            summary.name,
            summary.corpus_documents,
            summary.exact_counterpart_doc_pairs,
            summary.fit_candidates,
            summary.holdout_candidates,
            summary.cross_partition_candidates_discarded
        );
    }
    candidates_out.flush()?;
    drop(candidates_out);
    let (candidate_rows_sha256, candidate_rows_bytes) = sha256_path(&candidates_path)?;
    eprintln!("P1P3E2 candidates sealed: bytes={candidate_rows_bytes}");
    let candidate_rows_total = relation_counts.values().map(|row| row.0).sum();
    let fit_rows_total = relation_counts.values().map(|row| row.1).sum();
    let holdout_rows_total = relation_counts.values().map(|row| row.2).sum();
    let relations = relation_counts
        .into_iter()
        .map(|(candidate_id, counts)| RelationSummary {
            candidate_id,
            direction: counts.4,
            transport_status: counts.3,
            candidate_rows: counts.0,
            fit_rows: counts.1,
            holdout_rows: counts.2,
        })
        .collect();
    eprintln!("P1P3E2 summaries ready");
    let receipt = Receipt {
        schema: "phoenix.lexical.lt9-la2-p1p3e2-opportunity-screen/v1",
        date: "2026-09-28",
        status: "QRELS_COUNTERPART_CANDIDATES_NOT_BASELINE_RANKED",
        interpretation: "Label-blind lexical candidate screen only. Positive qrels do not establish compatibility or authorize transport.",
        protocol_sha256: sha256(&protocol_bytes), cohort_sha256: sha256(&cohort_bytes),
        candidate_inventory_sha256: sha256(&relations_bytes),
        runner_source_sha256: sha256(&fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/main.rs"))?),
        runner_binary_sha256: sha256(&fs::read(env::current_exe()?)?),
        candidate_rows_sha256,
        candidate_rows_bytes,
        split_salt: SPLIT_SALT,
        candidate_inventory_source: inventory.source,
        negative_support_rule: inventory.negative_support_rule,
        candidate_directions, input_hashes, datasets, relations,
        candidate_rows_total, fit_rows_total, holdout_rows_total,
        baseline_rank_status: "NOT_RUN_IN_COUNTERPART_SCREEN",
    };
    eprintln!("P1P3E2 receipt assembled");
    let receipt_path = output_dir.join("opportunity-screen-receipt.json");
    let mut receipt_out = BufWriter::new(File::create(receipt_path)?);
    eprintln!("P1P3E2 writing receipt");
    serde_json::to_writer(&mut receipt_out, &receipt)?;
    receipt_out.flush()?;
    eprintln!("P1P3E2 receipt written");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{exact_token_contains, partition, Direction, Receipt};
    use std::collections::BTreeMap;

    #[test]
    fn exact_token_matching_respects_punctuation_case_and_boundaries() {
        assert!(exact_token_contains("A vicious-circle", "vicious"));
        assert!(exact_token_contains("WORSENING, not worse", "worsening"));
        assert!(!exact_token_contains("pulling and plucked", "pull"));
        assert!(!exact_token_contains("vehicle2", "vehicle"));
    }

    #[test]
    fn document_and_query_assignments_are_stable_and_namespaced() {
        assert_eq!(partition('Q', "nq", "q-1"), partition('Q', "nq", "q-1"));
        assert_ne!(partition('Q', "nq", "q-1"), partition('D', "nq", "q-1"));
    }

    #[test]
    fn compact_receipt_serialization_handles_candidate_directions() {
        let receipt = Receipt {
            schema: "schema",
            date: "date",
            status: "status",
            interpretation: "interpretation",
            protocol_sha256: "0".to_owned(),
            cohort_sha256: "0".to_owned(),
            candidate_inventory_sha256: "0".to_owned(),
            runner_source_sha256: "0".to_owned(),
            runner_binary_sha256: "0".to_owned(),
            candidate_inventory_source: "source".to_owned(),
            negative_support_rule: "rule".to_owned(),
            candidate_rows_sha256: "0".to_owned(),
            candidate_rows_bytes: 0,
            split_salt: "salt",
            candidate_directions: vec![Direction {
                id: "pair:a->b".to_owned(),
                pair_id: "pair".to_owned(),
                source: "a".to_owned(),
                target: "b".to_owned(),
                transport_status: "candidate".to_owned(),
            }],
            input_hashes: Vec::new(),
            datasets: Vec::new(),
            relations: Vec::new(),
            candidate_rows_total: 0,
            fit_rows_total: 0,
            holdout_rows_total: 0,
            baseline_rank_status: "not-run",
        };
        let encoded = serde_json::to_vec_pretty(&receipt).unwrap();
        assert!(std::str::from_utf8(&encoded)
            .unwrap()
            .contains("candidate_directions"));
        let _: BTreeMap<String, serde_json::Value> = serde_json::from_slice(&encoded).unwrap();
    }
}
