//! P1N4 label-blind, targeted natural-context acquisition.
//! Selection uses only frozen corpus text and structural features; it assigns no labels.

use anyhow::{ensure, Context, Result};
use hashbrown::{HashMap, HashSet};
use memchr::memchr_iter;
use memmap2::MmapOptions;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::collections::{BTreeMap, BinaryHeap};
use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

#[path = "lt9_la2p1n3_features.rs"]
mod features;
use features::*;

const DATE: &str = "2026-09-23";
const MAX_DISTANCE: usize = 24;
const WINDOW: usize = 8;
const DISPLAY_WINDOW: usize = 12;
const RESERVOIR_PER_CORPUS: usize = 96;
const TARGET_PER_CANDIDATE_STRATUM: usize = 8;
const MAX_PER_CORPUS_CANDIDATE_STRATUM: usize = 2;
const STRATA: [&str; 5] = [
    "ambiguity_low_evidence",
    "high_overlap_structural_divergence",
    "high_overlap_control",
    "low_overlap",
    "ordinary_middle",
];
const SALT: &[u8] = b"lt9-la2-p1n4-targeted-natural-compatibility-20260923-v1";
const SUPPORT: &[&str] = &[
    "also",
    "called",
    "known",
    "means",
    "aka",
    "similar",
    "equivalent",
    "same",
    "like",
];
const CONTRADICTION: &[&str] = &[
    "not",
    "never",
    "unlike",
    "different",
    "rather",
    "instead",
    "versus",
    "vs",
    "without",
];
const RELATIONS: [Relation; 3] = [
    Relation {
        id: "bank_to_water",
        a: "bank",
        b: "water",
    },
    Relation {
        id: "car_to_vehicle",
        a: "car",
        b: "vehicle",
    },
    Relation {
        id: "insurance_to_coverage",
        a: "insurance",
        b: "coverage",
    },
];

#[derive(Clone, Copy)]
struct Relation {
    id: &'static str,
    a: &'static str,
    b: &'static str,
}

#[derive(Deserialize)]
struct Roster {
    schema: String,
    corpora: Vec<CorpusSpec>,
}

#[derive(Clone, Deserialize, Serialize)]
struct CorpusSpec {
    corpus_id: String,
    path: PathBuf,
    sha256: String,
}

#[derive(Deserialize)]
struct CorpusDoc<'a> {
    #[serde(default, borrow)]
    title: Option<Cow<'a, str>>,
    #[serde(default, borrow)]
    text: Option<Cow<'a, str>>,
}

#[derive(Deserialize)]
struct P1n3Root {
    private_ledger_sha256: String,
    packets_sha256: String,
}

#[derive(Deserialize)]
struct PriorPacketDocHashes {
    candidate_id: String,
    left_document_sha256: String,
    right_document_sha256: String,
    left_template_sha256: String,
    right_template_sha256: String,
}

#[derive(Clone, Copy)]
struct Token<'a> {
    text: &'a str,
    start: usize,
    end: usize,
}

#[derive(Clone, Serialize)]
struct ContextFeatures {
    tokens: Vec<String>,
    role_tokens: Vec<String>,
    bigrams: Vec<String>,
    trigrams: Vec<String>,
    role_counts: [u16; 3],
    local_token_count: u16,
    pair_distance: u16,
    distance_bin: u8,
    support_cue: bool,
    contradiction_cue: bool,
    is_title_field: bool,
}

#[derive(Clone, Serialize)]
struct PairFeatures {
    shared_tokens: Vec<String>,
    shared_role_tokens: Vec<String>,
    shared_bigrams: Vec<String>,
    shared_trigrams: Vec<String>,
    token_jaccard: f32,
    bigram_jaccard: f32,
    trigram_jaccard: f32,
    role_count_abs_delta: [u16; 3],
    token_count_abs_delta: u16,
    distance_bin_abs_delta: u8,
    support_cue_equal: bool,
    contradiction_cue_equal: bool,
    same_field_kind: bool,
}

#[derive(Clone)]
struct Occurrence {
    corpus_index: usize,
    doc_hash: String,
    template_hash: String,
    excerpt: String,
    excerpt_words: u16,
    features: ContextFeatures,
}

#[derive(Clone, Copy)]
struct PairChoice {
    left: usize,
    right: usize,
    corpus_index: usize,
    overlap: f32,
    divergence: u32,
    stratum: usize,
    order_key: u64,
}

#[derive(Clone, Serialize)]
struct ReviewPacket {
    packet_id: String,
    lexical_pair: [String; 2],
    contexts: [String; 2],
    judgment: Option<String>,
}

#[derive(Clone, Serialize)]
struct PrivateLedgerRow {
    packet_id: String,
    candidate_id: String,
    corpus_id: String,
    sampling_stratum: String,
    left_document_sha256: String,
    right_document_sha256: String,
    left_template_sha256: String,
    right_template_sha256: String,
    lexical_overlap_jaccard: f32,
    structural_divergence: u32,
    split: String,
    left: ContextFeatures,
    right: ContextFeatures,
    pair_features: PairFeatures,
}

#[derive(Serialize)]
struct CorpusCount {
    corpus_id: String,
    documents_scanned: u64,
    p1n3_documents_excluded: u64,
    p1n3_templates_excluded: u64,
    relation_occurrences_seen: [u64; 3],
    reservoir_contexts: [usize; 3],
    eligible_context_pairs: [u64; 3],
}

#[derive(Serialize)]
struct Receipt {
    schema: &'static str,
    date: &'static str,
    status: &'static str,
    source_roster_sha256: String,
    p1n3_document_exclusion_ledger_sha256: String,
    p1n3_documents_excluded: usize,
    p1n3_context_templates_excluded: usize,
    source_hashes_verified: bool,
    target_per_candidate_per_stratum: usize,
    max_per_corpus_candidate_stratum: usize,
    stratum_names: [&'static str; 5],
    selected_by_candidate_stratum: [[usize; 5]; 3],
    shortfall_by_candidate_stratum: [[usize; 5]; 3],
    review_packet_count: usize,
    unique_documents_in_packets: usize,
    fit_packets: usize,
    holdout_packets: usize,
    template_split_conflicts: usize,
    candidate_token_feature_leaks: usize,
    semantic_labels_assigned: usize,
    corpora: Vec<CorpusCount>,
    scope: Scope,
}

#[derive(Serialize)]
struct Scope {
    p1n3_packets_or_judgments_opened_by_sampler: bool,
    p1n3_document_and_template_hashes_used_only_for_exclusion: bool,
    qrels_queries_or_expected_sense_labels_read: bool,
    retrieval_or_ranking_run: bool,
    authority_updated: bool,
    labels_assigned_by_sampler: bool,
    reviewer_receives_provenance_features_or_strata: bool,
}

#[derive(Serialize)]
struct PreReviewRoot {
    schema: &'static str,
    date: &'static str,
    branch: String,
    protocol_sha256: String,
    roster_sha256: String,
    prior_ledger_sha256: String,
    prior_root_sha256: String,
    source_sha256: BTreeMap<String, String>,
    binary_sha256: String,
    packets_sha256: String,
    rubric_sha256: String,
    judgment_template_sha256: String,
    private_ledger_sha256: String,
    receipt_sha256: String,
    judgments_completed: bool,
}

struct Reservoir {
    slots: Vec<(u64, Occurrence)>,
    heap: BinaryHeap<(u64, usize)>,
}

impl Reservoir {
    fn new() -> Self {
        Self {
            slots: Vec::with_capacity(RESERVOIR_PER_CORPUS),
            heap: BinaryHeap::with_capacity(RESERVOIR_PER_CORPUS),
        }
    }
    fn insert(&mut self, key: u64, item: Occurrence) {
        if self.slots.len() < RESERVOIR_PER_CORPUS {
            let index = self.slots.len();
            self.slots.push((key, item));
            self.heap.push((key, index));
        } else if self.heap.peek().is_some_and(|(largest, _)| key < *largest) {
            let (_, index) = self.heap.pop().expect("full reservoir has a heap entry");
            self.slots[index] = (key, item);
            self.heap.push((key, index));
        }
    }
    fn into_items(self) -> Vec<Occurrence> {
        self.slots.into_iter().map(|(_, item)| item).collect()
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    ensure!(args.len() == 5,
        "usage: lt9_la2p1n4 <repo-root> <p1n3-private-ledger.json> <p1n3-pre-review-root.json> <output-dir>");
    run(
        Path::new(&args[1]),
        Path::new(&args[2]),
        Path::new(&args[3]),
        Path::new(&args[4]),
    )
}

fn run(repo: &Path, prior_ledger_path: &Path, prior_root_path: &Path, output: &Path) -> Result<()> {
    let protocol = repo.join("docs/LT9_LA2_P1N4_TARGETED_COMPATIBILITY_20260923.md");
    let roster_path = repo.join("experiments/lt9-la2-p1n3/corpus-roster-20260923.json");
    let p1n3_root_bytes = fs::read(prior_root_path).context("read P1N3 pre-review root")?;
    let p1n3_root: P1n3Root = serde_json::from_slice(&p1n3_root_bytes)?;
    let prior_ledger_bytes =
        fs::read(prior_ledger_path).context("read P1N3 hash-only exclusion input")?;
    ensure!(
        sha256(&prior_ledger_bytes) == p1n3_root.private_ledger_sha256,
        "P1N3 private-ledger hash does not match its frozen root"
    );
    let _sealed_packet_hash = &p1n3_root.packets_sha256;
    let prior_rows: Vec<PriorPacketDocHashes> = serde_json::from_slice(&prior_ledger_bytes)?;
    ensure!(
        prior_rows.len() == 96,
        "expected exactly 96 sealed P1N3 ledger rows"
    );
    let mut excluded_docs = HashSet::with_capacity(prior_rows.len() * 2);
    let mut excluded_templates = HashSet::with_capacity(prior_rows.len() * 2);
    for row in prior_rows {
        excluded_docs.insert(row.left_document_sha256);
        excluded_docs.insert(row.right_document_sha256);
        excluded_templates.insert(prior_template_key(
            &row.candidate_id,
            &row.left_template_sha256,
        ));
        excluded_templates.insert(prior_template_key(
            &row.candidate_id,
            &row.right_template_sha256,
        ));
    }
    ensure!(
        excluded_docs.len() == 192,
        "P1N3 exclusion set must contain 192 unique documents"
    );

    let roster_bytes = fs::read(&roster_path).context("read frozen P1N3 text-only roster")?;
    let roster: Roster = serde_json::from_slice(&roster_bytes)?;
    ensure!(
        roster.schema == "phoenix.lexical.lt9-la2-p1n3-corpus-roster/v1"
            && roster.corpora.len() == 12,
        "P1N3 corpus cohort changed"
    );
    fs::create_dir_all(output)?;
    let review_dir = output.join("blind-review");
    fs::create_dir_all(&review_dir)?;

    let mut reservoirs: Vec<Vec<Reservoir>> = (0..roster.corpora.len())
        .map(|_| (0..RELATIONS.len()).map(|_| Reservoir::new()).collect())
        .collect();
    let mut corpus_counts = Vec::with_capacity(roster.corpora.len());

    for (corpus_index, spec) in roster.corpora.iter().enumerate() {
        let file =
            File::open(&spec.path).with_context(|| format!("open corpus {}", spec.corpus_id))?;
        let mmap = unsafe { MmapOptions::new().map(&file) }
            .with_context(|| format!("mmap {}", spec.corpus_id))?;
        ensure!(
            sha256(mmap.as_ref()) == spec.sha256,
            "frozen corpus hash mismatch: {}",
            spec.corpus_id
        );
        let mut docs = 0u64;
        let mut excluded = 0u64;
        let mut excluded_contexts = 0u64;
        let mut seen = [0u64; 3];
        let mut start = 0usize;
        for end in memchr_iter(b'\n', mmap.as_ref()).chain(std::iter::once(mmap.len())) {
            let line = &mmap[start..end];
            start = end.saturating_add(1);
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            let doc: CorpusDoc<'_> = serde_json::from_slice(line)
                .with_context(|| format!("decode JSONL in {}", spec.corpus_id))?;
            let title = doc.title.as_deref().unwrap_or("");
            let body = doc.text.as_deref().unwrap_or("");
            let doc_hash = document_hash(title, body);
            if excluded_docs.contains(&doc_hash) {
                excluded += 1;
                docs += 1;
                continue;
            }
            let title_tokens = tokenize(title);
            let body_tokens = tokenize(body);
            for (relation_index, relation) in RELATIONS.iter().enumerate() {
                let title_pair = nearest_pair(&title_tokens, relation.a, relation.b);
                let body_pair = nearest_pair(&body_tokens, relation.a, relation.b);
                let best = match (title_pair, body_pair) {
                    (Some(a), Some(b)) => {
                        if a.2 <= b.2 {
                            Some((true, a))
                        } else {
                            Some((false, b))
                        }
                    }
                    (Some(a), None) => Some((true, a)),
                    (None, Some(b)) => Some((false, b)),
                    (None, None) => None,
                };
                if let Some((is_title, (left, right, distance))) =
                    best.filter(|(_, pair)| pair.2 <= MAX_DISTANCE)
                {
                    let tokens = if is_title {
                        &title_tokens
                    } else {
                        &body_tokens
                    };
                    let field = if is_title { title } else { body };
                    let (excerpt, template_hash, features) =
                        extract_context(field, tokens, left, right, distance, *relation, is_title);
                    if is_prior_template_excluded(relation.id, &template_hash, &excluded_templates)
                    {
                        excluded_contexts += 1;
                        continue;
                    }
                    let occurrence = Occurrence {
                        corpus_index,
                        doc_hash: doc_hash.clone(),
                        template_hash,
                        excerpt_words: excerpt
                            .split_ascii_whitespace()
                            .count()
                            .min(u16::MAX as usize) as u16,
                        excerpt,
                        features,
                    };
                    let key = stable_hash(
                        &[
                            spec.corpus_id.as_bytes(),
                            doc_hash.as_bytes(),
                            relation.id.as_bytes(),
                        ]
                        .concat(),
                    );
                    reservoirs[corpus_index][relation_index].insert(key, occurrence);
                    seen[relation_index] += 1;
                }
            }
            docs += 1;
        }
        let stored = std::array::from_fn(|r| reservoirs[corpus_index][r].slots.len());
        corpus_counts.push(CorpusCount {
            corpus_id: spec.corpus_id.clone(),
            documents_scanned: docs,
            p1n3_documents_excluded: excluded,
            p1n3_templates_excluded: excluded_contexts,
            relation_occurrences_seen: seen,
            reservoir_contexts: stored,
            eligible_context_pairs: [0; 3],
        });
    }

    let mut occurrences: Vec<Vec<Occurrence>> = (0..RELATIONS.len()).map(|_| Vec::new()).collect();
    for corpus_index in 0..roster.corpora.len() {
        for relation_index in 0..RELATIONS.len() {
            occurrences[relation_index].extend(
                std::mem::replace(
                    &mut reservoirs[corpus_index][relation_index],
                    Reservoir::new(),
                )
                .into_items(),
            );
        }
    }
    let mut pools: Vec<Vec<PairChoice>> = (0..RELATIONS.len() * STRATA.len())
        .map(|_| Vec::new())
        .collect();
    let mut eligible_pairs = vec![[0u64; 3]; roster.corpora.len()];
    for relation_index in 0..RELATIONS.len() {
        build_relation_pools(
            relation_index,
            &occurrences[relation_index],
            &mut pools,
            &mut eligible_pairs,
        )?;
    }
    for corpus_index in 0..roster.corpora.len() {
        corpus_counts[corpus_index].eligible_context_pairs = eligible_pairs[corpus_index];
    }
    let selected = select_pairs(&pools, &occurrences);
    ensure!(
        !selected.is_empty(),
        "no targeted natural context pairs were eligible"
    );

    let mut packets = Vec::with_capacity(selected.len());
    let mut ledger = Vec::with_capacity(selected.len());
    let mut used_docs = HashSet::with_capacity(selected.len() * 2);
    let mut token_leaks = 0usize;
    for item in selected {
        let relation = RELATIONS[item.relation_index];
        let left = &occurrences[item.relation_index][item.choice.left];
        let right = &occurrences[item.relation_index][item.choice.right];
        ensure!(
            left.doc_hash != right.doc_hash,
            "same document used at both endpoints"
        );
        ensure!(
            used_docs.insert(left.doc_hash.clone()) && used_docs.insert(right.doc_hash.clone()),
            "document reused across P1N4 packets"
        );
        ensure!(
            !excluded_docs.contains(&left.doc_hash) && !excluded_docs.contains(&right.doc_hash),
            "P1N3 document leaked into P1N4"
        );
        let packet_id = packet_id(relation.id, &left.doc_hash, &right.doc_hash);
        let pair = pair_features(&left.features, &right.features);
        token_leaks += feature_token_leaks(&left.features, &right.features, &pair, relation);
        packets.push(ReviewPacket {
            packet_id: packet_id.clone(),
            lexical_pair: [relation.a.to_string(), relation.b.to_string()],
            contexts: [left.excerpt.clone(), right.excerpt.clone()],
            judgment: None,
        });
        ledger.push(PrivateLedgerRow {
            packet_id,
            candidate_id: relation.id.to_string(),
            corpus_id: roster.corpora[left.corpus_index].corpus_id.clone(),
            sampling_stratum: STRATA[item.choice.stratum].to_string(),
            left_document_sha256: left.doc_hash.clone(),
            right_document_sha256: right.doc_hash.clone(),
            left_template_sha256: left.template_hash.clone(),
            right_template_sha256: right.template_hash.clone(),
            lexical_overlap_jaccard: item.choice.overlap,
            structural_divergence: item.choice.divergence,
            split: String::new(),
            left: left.features.clone(),
            right: right.features.clone(),
            pair_features: pair,
        });
    }
    assign_template_splits(&mut ledger);
    ensure!(
        token_leaks == 0,
        "candidate token present in compatibility features"
    );
    validate_packets(&packets, &ledger, &excluded_docs)?;

    // Opaque deterministic presentation order prevents the packet sequence revealing strata.
    let mut order: Vec<usize> = (0..packets.len()).collect();
    order.sort_by_key(|&i| stable_hash(packets[i].packet_id.as_bytes()));
    let packets: Vec<ReviewPacket> = order.iter().map(|&i| packets[i].clone()).collect();
    let ledger: Vec<PrivateLedgerRow> = order.iter().map(|&i| ledger[i].clone()).collect();

    let packet_path = review_dir.join("packets.json");
    write_json(&packet_path, &packets)?;
    let rubric_path = review_dir.join("rubric.md");
    fs::write(&rubric_path, RUBRIC.as_bytes())?;
    let judgments_template_path = review_dir.join("judgments-template.json");
    let judgments_template: Vec<JudgmentTemplateRow> = packets
        .iter()
        .map(|packet| JudgmentTemplateRow {
            packet_id: packet.packet_id.clone(),
            judgment: None,
        })
        .collect();
    write_json(&judgments_template_path, &judgments_template)?;
    let private_ledger_path = output.join("private-ledger.json");
    write_json(&private_ledger_path, &ledger)?;
    let receipt = make_receipt(
        &roster,
        &roster_bytes,
        &prior_ledger_bytes,
        &excluded_docs,
        &excluded_templates,
        corpus_counts,
        &ledger,
        used_docs.len(),
        token_leaks,
    )?;
    let receipt_path = output.join("p1n4-pre-review-receipt.json");
    write_json(&receipt_path, &receipt)?;
    let root = make_root(
        repo,
        &protocol,
        &roster_path,
        prior_ledger_path,
        prior_root_path,
        &packet_path,
        &rubric_path,
        &judgments_template_path,
        &private_ledger_path,
        &receipt_path,
    )?;
    write_json(&output.join("pre-review-root.json"), &root)?;
    println!(
        "P1N4 packets={} unique_docs={} output={}",
        receipt.review_packet_count,
        used_docs.len(),
        output.display()
    );
    Ok(())
}

include!("lt9_la2p1n4_helpers.rs");
