//! R3-H sealed human-judgment packet generator.
//!
//! Samples only train/dev queries from the frozen literal universe. The packet
//! is blinded: it contains query and document text plus an opaque item id.
//! System scores, ranks, rarity, qrels, and document ids stay in a separate
//! ledger. Test qrels are never opened.

use std::collections::{HashMap, HashSet};
use std::env;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use blake3::Hasher;
use phoenix_lexical_qps::{
    DocumentInput, FieldConfig, QpsBuilder, QpsConfig, QpsIndex, SearchScratch,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MAXIMUM_QUERY_GROUPS: usize = 128;
const MAX_QUERIES_PER_SPLIT: usize = 12;
const PACKETS_PER_QUERY: usize = 4;
const EXHAUSTIVE_LIMIT: usize = 4096;

#[derive(Debug, Deserialize)]
struct CorpusRow {
    _id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    text: String,
}
#[derive(Debug, Deserialize)]
struct QueryRow {
    _id: String,
    text: String,
}
#[derive(Clone)]
struct Document {
    id: String,
    title: String,
    text: String,
}
#[derive(Clone)]
struct Query {
    id: String,
    text: String,
}
type Qrels = HashMap<String, HashMap<String, u32>>;

#[derive(Clone, Copy)]
struct HitMeta {
    document_index: usize,
    phoenix_rank: usize,
    bm25f: f32,
    rarity: f32,
}

#[derive(Debug, Serialize)]
struct PacketItem {
    packet_id: String,
    query: String,
    document_title: String,
    document_text: String,
    judgment: Option<u8>,
    note: String,
}

#[derive(Debug, Serialize)]
struct LedgerItem {
    packet_id: String,
    dataset: String,
    split: String,
    query_id: String,
    document_id: String,
    stratum: String,
    system_origin: String,
    phoenix_rank: usize,
    bm25f_rank: usize,
    bm25f_score: f32,
    rarest_matched_term: f32,
}

#[derive(Debug, Serialize)]
struct Receipt {
    contract: &'static str,
    dataset: String,
    sampling_seed: String,
    query_policy: &'static str,
    packet_policy: &'static str,
    corpus_sha256: String,
    query_sha256: String,
    train_qrels_sha256: String,
    dev_qrels_sha256: String,
    selected_queries: usize,
    packets: usize,
    packets_by_stratum: HashMap<String, usize>,
    packet_file: String,
    ledger_file: String,
}

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let root = PathBuf::from(args.next().context(
        "usage: qps_v3_r3_h_packet <dataset-root> <packet-json> <ledger-json> [receipt-json] [sampling-salt]",
    )?);
    let packet_path = PathBuf::from(args.next().context("missing packet output")?);
    let ledger_path = PathBuf::from(args.next().context("missing ledger output")?);
    let receipt_path = args.next().map(PathBuf::from);
    let sampling_salt = args.next().unwrap_or_else(|| "r3h-v1".to_owned());
    let dataset = root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("beir")
        .to_owned();
    let corpus_path = root.join("corpus.jsonl");
    let queries_path = root.join("queries.jsonl");
    let train_path = root.join("qrels").join("train.tsv");
    let dev_path = root.join("qrels").join("dev.tsv");
    let documents = read_corpus(&corpus_path)?;
    let queries = read_queries(&queries_path)?;
    let query_by_id = queries
        .into_iter()
        .map(|query| (query.id.clone(), query))
        .collect::<HashMap<_, _>>();
    let (train_qrels, dev_qrels, train_sha, dev_sha) =
        load_train_dev_qrels(&train_path, &dev_path)?;
    let index = build_qps(&documents)?;
    let mut scratch = SearchScratch::with_document_capacity(documents.len(), MAXIMUM_QUERY_GROUPS);
    let mut packet_candidates = Vec::<(String, String, String, HitMeta, Document)>::new();
    let mut strata = HashMap::<String, usize>::new();
    for (split, qrels) in [("train", train_qrels), ("dev", dev_qrels)] {
        let selected = select_queries(&qrels, &sampling_salt);
        for query_id in selected {
            let Some(query) = query_by_id.get(&query_id) else {
                continue;
            };
            let positives = qrels.get(&query_id).expect("selected query qrels");
            let chosen = choose_candidates(&index, &mut scratch, &documents, query, positives)?;
            for (stratum, origin, hit) in chosen {
                let document = documents[hit.document_index].clone();
                *strata.entry(stratum.clone()).or_default() += 1;
                packet_candidates.push((
                    split.to_owned(),
                    query_id.clone(),
                    stratum,
                    hit,
                    document,
                ));
                let _ = origin;
            }
        }
    }
    let mut packet_order = packet_candidates
        .iter()
        .enumerate()
        .map(|(index, item)| {
            (
                stable_key(
                    &dataset,
                    &sampling_salt,
                    &item.0,
                    &item.1,
                    item.4.id.as_str(),
                ),
                index,
            )
        })
        .collect::<Vec<_>>();
    packet_order.sort_unstable();
    let mut packets = Vec::with_capacity(packet_order.len());
    let mut ledger = Vec::with_capacity(packet_order.len());
    for (ordinal, (_, candidate_index)) in packet_order.into_iter().enumerate() {
        let (split, query_id, stratum, hit, document) = packet_candidates[candidate_index].clone();
        let packet_id = format!("r3h-{:04}", ordinal + 1);
        let query = query_by_id.get(&query_id).expect("query exists");
        packets.push(PacketItem {
            packet_id: packet_id.clone(),
            query: query.text.clone(),
            document_title: document.title.clone(),
            document_text: document.text.clone(),
            judgment: None,
            note: String::new(),
        });
        let bm25f_rank = bm25f_rank_for(
            &index,
            &mut scratch,
            query,
            hit.document_index,
            documents.len(),
        )?;
        ledger.push(LedgerItem {
            packet_id,
            dataset: dataset.clone(),
            split,
            query_id,
            document_id: document.id,
            stratum,
            system_origin: origin_label(hit, bm25f_rank),
            phoenix_rank: hit.phoenix_rank,
            bm25f_rank,
            bm25f_score: hit.bm25f,
            rarest_matched_term: hit.rarity,
        });
    }
    fs::write(&packet_path, serde_json::to_vec_pretty(&packets)?)?;
    fs::write(&ledger_path, serde_json::to_vec_pretty(&ledger)?)?;
    let receipt = Receipt {
        contract: "phoenix.qps.r3h-human-authority-packet/v1",
        dataset,
        sampling_seed: format!("blake3({sampling_salt}|dataset|split|query|document)"),
        query_policy: "train_and_dev_only;_test_qrels_are_never_opened",
        packet_policy: "four_blinded_strata_per_selected_query;_only_2_vs_0_becomes_authoritative",
        corpus_sha256: sha256_file(&corpus_path)?,
        query_sha256: sha256_file(&queries_path)?,
        train_qrels_sha256: train_sha,
        dev_qrels_sha256: dev_sha,
        selected_queries: packets
            .iter()
            .map(|packet| packet.packet_id.split('-').next().unwrap_or(""))
            .count()
            / PACKETS_PER_QUERY,
        packets: packets.len(),
        packets_by_stratum: strata,
        packet_file: packet_path.display().to_string(),
        ledger_file: ledger_path.display().to_string(),
    };
    let json = serde_json::to_vec_pretty(&receipt)?;
    if let Some(path) = receipt_path {
        fs::write(path, &json)?;
    }
    println!("{}", String::from_utf8_lossy(&json));
    Ok(())
}

fn choose_candidates(
    index: &QpsIndex,
    scratch: &mut SearchScratch,
    documents: &[Document],
    query: &Query,
    positives: &HashMap<String, u32>,
) -> Result<Vec<(String, String, HitMeta)>> {
    let mut hits = Vec::with_capacity(EXHAUSTIVE_LIMIT);
    index.search_exhaustive_into(&query.text, EXHAUSTIVE_LIMIT, scratch, &mut hits)?;
    let positive_ids = positives
        .iter()
        .filter_map(|(id, score)| (*score > 0).then_some(id.as_str()))
        .collect::<HashSet<_>>();
    let candidates = hits
        .iter()
        .enumerate()
        .filter_map(|(rank, hit)| {
            let index = usize::try_from(hit.external_id).ok()?;
            let document = documents.get(index)?;
            if positive_ids.contains(document.id.as_str()) {
                return None;
            }
            Some(HitMeta {
                document_index: index,
                phoenix_rank: rank + 1,
                bm25f: hit.rank_evidence_v3.values[0],
                rarity: hit.rank_evidence_v3.values[19],
            })
        })
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return Ok(Vec::new());
    }
    let mut bm25_sorted = candidates.clone();
    bm25_sorted.sort_unstable_by(|left, right| {
        right
            .bm25f
            .total_cmp(&left.bm25f)
            .then_with(|| left.document_index.cmp(&right.document_index))
    });
    let bm25_rank = bm25_sorted
        .iter()
        .enumerate()
        .map(|(rank, hit)| (hit.document_index, rank + 1))
        .collect::<HashMap<_, _>>();
    let mut chosen = Vec::with_capacity(PACKETS_PER_QUERY);
    let mut used = HashSet::new();
    let disagreement = candidates
        .iter()
        .max_by_key(|hit| {
            let other = *bm25_rank
                .get(&hit.document_index)
                .unwrap_or(&hit.phoenix_rank);
            hit.phoenix_rank.abs_diff(other)
        })
        .copied();
    if let Some(hit) = disagreement {
        used.insert(hit.document_index);
        chosen.push((
            "system_disagreement".to_owned(),
            if bm25_rank[&hit.document_index] < hit.phoenix_rank {
                "bm25f_high"
            } else {
                "phoenix_high"
            }
            .to_owned(),
            hit,
        ));
    }
    let high_rarity = candidates
        .iter()
        .filter(|hit| !used.contains(&hit.document_index))
        .max_by(|left, right| left.rarity.total_cmp(&right.rarity))
        .copied();
    if let Some(hit) = high_rarity {
        used.insert(hit.document_index);
        chosen.push(("high_rarity".to_owned(), "rarity_high".to_owned(), hit));
    }
    if let Some(reference) = high_rarity {
        let control = candidates
            .iter()
            .filter(|hit| !used.contains(&hit.document_index))
            .min_by_key(|hit| {
                (
                    hit.phoenix_rank.abs_diff(reference.phoenix_rank),
                    (hit.rarity * 1000.0) as u32,
                )
            })
            .copied();
        if let Some(hit) = control {
            used.insert(hit.document_index);
            chosen.push((
                "rarity_control".to_owned(),
                "ordinary_rarity_matched_rank".to_owned(),
                hit,
            ));
        }
    }
    let deep = candidates
        .iter()
        .filter(|hit| !used.contains(&hit.document_index) && hit.phoenix_rank > 100)
        .min_by_key(|hit| hit.phoenix_rank)
        .copied();
    if let Some(hit) = deep {
        chosen.push((
            "deep_reachable".to_owned(),
            "rank_depth_control".to_owned(),
            hit,
        ));
    }
    while chosen.len() < PACKETS_PER_QUERY {
        let Some(hit) = candidates
            .iter()
            .find(|hit| !used.contains(&hit.document_index))
            .copied()
        else {
            break;
        };
        used.insert(hit.document_index);
        chosen.push((
            "ordinary_control".to_owned(),
            "fallback_control".to_owned(),
            hit,
        ));
    }
    Ok(chosen)
}

fn origin_label(hit: HitMeta, bm25f_rank: usize) -> String {
    if bm25f_rank < hit.phoenix_rank {
        "bm25f_high".to_owned()
    } else if hit.phoenix_rank < bm25f_rank {
        "phoenix_high".to_owned()
    } else {
        "tie".to_owned()
    }
}

fn bm25f_rank_for(
    index: &QpsIndex,
    scratch: &mut SearchScratch,
    query: &Query,
    document_index: usize,
    limit: usize,
) -> Result<usize> {
    let mut hits = Vec::with_capacity(limit.min(EXHAUSTIVE_LIMIT));
    index.search_exhaustive_into(&query.text, limit.min(EXHAUSTIVE_LIMIT), scratch, &mut hits)?;
    let mut sorted = hits.iter().copied().collect::<Vec<_>>();
    sorted.sort_unstable_by(|left, right| {
        right.rank_evidence_v3.values[0]
            .total_cmp(&left.rank_evidence_v3.values[0])
            .then_with(|| left.external_id.cmp(&right.external_id))
    });
    Ok(sorted
        .iter()
        .position(|hit| hit.external_id as usize == document_index)
        .map(|rank| rank + 1)
        .unwrap_or(limit + 1))
}

fn select_queries(qrels: &Qrels, sampling_salt: &str) -> Vec<String> {
    let mut ids = qrels.keys().cloned().collect::<Vec<_>>();
    ids.sort_unstable_by_key(|id| {
        let mut hasher = Hasher::new();
        hasher.update(sampling_salt.as_bytes());
        hasher.update(b"|");
        hasher.update(id.as_bytes());
        *hasher.finalize().as_bytes()
    });
    ids.truncate(MAX_QUERIES_PER_SPLIT);
    ids
}

fn stable_key(
    dataset: &str,
    sampling_salt: &str,
    split: &str,
    query: &str,
    document: &str,
) -> [u8; 32] {
    let mut hasher = Hasher::new();
    hasher.update(sampling_salt.as_bytes());
    hasher.update(b"|");
    hasher.update(dataset.as_bytes());
    hasher.update(b"|");
    hasher.update(split.as_bytes());
    hasher.update(b"|");
    hasher.update(query.as_bytes());
    hasher.update(b"|");
    hasher.update(document.as_bytes());
    *hasher.finalize().as_bytes()
}

fn build_qps(documents: &[Document]) -> Result<QpsIndex> {
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
    for (index, document) in documents.iter().enumerate() {
        builder.insert(DocumentInput {
            external_id: index as u64,
            fields: &[document.title.as_str(), document.text.as_str()],
        })?;
    }
    builder.build().map_err(Into::into)
}

fn read_corpus(path: &Path) -> Result<Vec<Document>> {
    BufReader::new(File::open(path)?)
        .lines()
        .enumerate()
        .map(|(line, value)| {
            let row: CorpusRow =
                serde_json::from_str(&value.with_context(|| format!("read {}", line + 1))?)?;
            Ok(Document {
                id: row._id,
                title: row.title,
                text: row.text,
            })
        })
        .collect()
}

fn read_queries(path: &Path) -> Result<Vec<Query>> {
    BufReader::new(File::open(path)?)
        .lines()
        .enumerate()
        .map(|(line, value)| {
            let row: QueryRow =
                serde_json::from_str(&value.with_context(|| format!("read {}", line + 1))?)?;
            Ok(Query {
                id: row._id,
                text: row.text,
            })
        })
        .collect()
}

fn read_qrels(path: &Path) -> Result<Qrels> {
    let mut result = Qrels::new();
    if !path.exists() {
        return Ok(result);
    }
    for (line, value) in BufReader::new(File::open(path)?).lines().enumerate() {
        let value = value.with_context(|| format!("read qrels line {}", line + 1))?;
        if line == 0 && value.starts_with("query-id") {
            continue;
        }
        let mut columns = value.split('\t');
        let query = columns.next().context("qrels query missing")?;
        let document = columns.next().context("qrels document missing")?;
        let score = columns
            .next()
            .context("qrels score missing")?
            .parse::<u32>()?;
        result
            .entry(query.to_owned())
            .or_default()
            .insert(document.to_owned(), score);
    }
    Ok(result)
}

fn load_train_dev_qrels(train: &Path, dev: &Path) -> Result<(Qrels, Qrels, String, String)> {
    let all_train = read_qrels(train)?;
    if dev.exists() {
        return Ok((
            all_train,
            read_qrels(dev)?,
            sha256_file(train)?,
            sha256_file(dev)?,
        ));
    }
    let mut training = Qrels::new();
    let mut development = Qrels::new();
    for (query, judgments) in all_train {
        if blake3::hash(query.as_bytes()).as_bytes()[0] % 5 == 0 {
            development.insert(query, judgments);
        } else {
            training.insert(query, judgments);
        }
    }
    let train_sha = sha256_qrels(&training);
    let dev_sha = sha256_qrels(&development);
    Ok((training, development, train_sha, dev_sha))
}

fn sha256_qrels(qrels: &Qrels) -> String {
    let mut rows = qrels
        .iter()
        .flat_map(|(query, judgments)| {
            judgments
                .iter()
                .map(move |(document, score)| (query.as_str(), document.as_str(), *score))
        })
        .collect::<Vec<_>>();
    rows.sort_unstable();
    let mut hasher = Sha256::new();
    for (query, document, score) in rows {
        hasher.update(query.as_bytes());
        hasher.update(b"\t");
        hasher.update(document.as_bytes());
        hasher.update(b"\t");
        hasher.update(score.to_string().as_bytes());
        hasher.update(b"\n");
    }
    format!("{:x}", hasher.finalize())
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    hasher.update(fs::read(path)?);
    Ok(format!("{:x}", hasher.finalize()))
}
