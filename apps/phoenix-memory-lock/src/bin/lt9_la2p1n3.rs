//! P1N3 blind natural-context pair acquisition.
//! Text-only deterministic sampling; this program assigns no semantic labels.

use anyhow::{ensure, Context, Result};
use hashbrown::{HashMap, HashSet};
use memchr::memchr_iter;
use memmap2::MmapOptions;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::collections::BinaryHeap;
use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

#[path = "lt9_la2p1n3_features.rs"]
mod features;
#[path = "lt9_la2p1n3_seal.rs"]
mod seal;
use features::*;
use seal::*;

const DATE: &str = "2026-09-23";
const MAX_DISTANCE: usize = 24;
const WINDOW: usize = 8;
const DISPLAY_WINDOW: usize = 12;
const RESERVOIR_PER_CORPUS: usize = 64;
const TARGET_PER_BAND: usize = 16;
const MAX_PAIRS_PER_CORPUS_BAND: usize = 4;
const SALT: &[u8] = b"lt9-la2-p1n3-natural-pairwise-20260923-v1";
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
    features: ContextFeatures,
}

#[derive(Clone, Copy)]
struct PairChoice {
    left: usize,
    right: usize,
    jaccard: f32,
    order_key: u64,
}

#[derive(Serialize)]
struct ReviewPacket {
    packet_id: String,
    lexical_pair: [String; 2],
    contexts: [String; 2],
    judgment: Option<String>,
}

#[derive(Serialize)]
struct PrivateLedgerRow {
    packet_id: String,
    candidate_id: String,
    corpus_id: String,
    left_document_sha256: String,
    right_document_sha256: String,
    left_template_sha256: String,
    right_template_sha256: String,
    lexical_overlap_jaccard: f32,
    overlap_band: String,
    split: String,
    left: ContextFeatures,
    right: ContextFeatures,
    pair_features: PairFeatures,
}

#[derive(Serialize)]
struct SourceCount {
    corpus_id: String,
    docs_scanned: u64,
    relation_occurrences_seen: [u64; 3],
    reservoir_contexts: [usize; 3],
    candidate_pairs: [u64; 3],
}

#[derive(Serialize)]
struct Receipt {
    schema: &'static str,
    date: &'static str,
    status: &'static str,
    source_roster_sha256: String,
    source_hashes_verified: bool,
    corpus_counts: Vec<SourceCount>,
    target_pairs_per_relation_band: usize,
    max_pairs_per_corpus_band: usize,
    selected_pairs_per_relation_band: [[usize; 2]; 3],
    review_packet_count: usize,
    unique_document_count: usize,
    fit_packets: usize,
    holdout_packets: usize,
    split_template_overlap: usize,
    candidate_tokens_in_any_feature: usize,
    judgment_values_assigned: usize,
    scope: Scope,
}

#[derive(Serialize)]
struct Scope {
    qrels_or_queries_read: bool,
    previous_outcomes_read: bool,
    expected_family_labels_used: bool,
    retrieval_or_ranking_run: bool,
    authority_updated: bool,
    human_judgments_assigned_by_sampler: bool,
    reviewer_receives_provenance_or_strata: bool,
}

#[derive(Serialize)]
struct PreReviewRoot {
    schema: &'static str,
    date: &'static str,
    branch: String,
    protocol_sha256: String,
    roster_sha256: String,
    source_sha256: BTreeMap<String, String>,
    binary_sha256: String,
    packets_sha256: String,
    rubric_sha256: String,
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
    ensure!(
        args.len() == 3,
        "usage: lt9_la2p1n3 <repo-root> <output-dir>"
    );
    run(Path::new(&args[1]), Path::new(&args[2]))
}

fn run(repo: &Path, output: &Path) -> Result<()> {
    let protocol = repo.join("docs/LT9_LA2_P1N3_NATURAL_PAIRWISE_COMPATIBILITY_20260923.md");
    let roster_path = repo.join("experiments/lt9-la2-p1n3/corpus-roster-20260923.json");
    let roster_bytes = fs::read(&roster_path).context("read frozen text-only corpus roster")?;
    let roster: Roster = serde_json::from_slice(&roster_bytes).context("decode P1N3 roster")?;
    ensure!(
        roster.schema == "phoenix.lexical.lt9-la2-p1n3-corpus-roster/v1",
        "unexpected roster schema"
    );
    ensure!(roster.corpora.len() == 12, "P1N3 corpus cohort changed");
    fs::create_dir_all(output)?;
    let review_dir = output.join("blind-review");
    fs::create_dir_all(&review_dir)?;

    let mut reservoirs: Vec<Vec<Reservoir>> = (0..roster.corpora.len())
        .map(|_| (0..RELATIONS.len()).map(|_| Reservoir::new()).collect())
        .collect();
    let mut source_counts = Vec::with_capacity(roster.corpora.len());

    for (corpus_index, spec) in roster.corpora.iter().enumerate() {
        let file = File::open(&spec.path)
            .with_context(|| format!("open text corpus {}", spec.corpus_id))?;
        let mmap = unsafe { MmapOptions::new().map(&file) }
            .with_context(|| format!("mmap {}", spec.corpus_id))?;
        let corpus_hash = format!("{:x}", Sha256::digest(mmap.as_ref()));
        ensure!(
            corpus_hash == spec.sha256,
            "frozen corpus hash mismatch: {}",
            spec.corpus_id
        );
        let mut docs = 0u64;
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
            let title_tokens = tokenize(title);
            let body_tokens = tokenize(body);
            for (relation_index, relation) in RELATIONS.iter().enumerate() {
                let in_title = nearest_pair(&title_tokens, relation.a, relation.b);
                let in_body = nearest_pair(&body_tokens, relation.a, relation.b);
                let best = match (in_title, in_body) {
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
                    best.filter(|(_, p)| p.2 <= MAX_DISTANCE)
                {
                    let tokens = if is_title {
                        &title_tokens
                    } else {
                        &body_tokens
                    };
                    let field = if is_title { title } else { body };
                    let doc_hash = document_hash(title, body);
                    let (excerpt, template_hash, features) =
                        extract_context(field, tokens, left, right, distance, *relation, is_title);
                    let occurrence = Occurrence {
                        corpus_index,
                        doc_hash: doc_hash.clone(),
                        template_hash,
                        excerpt,
                        features,
                    };
                    let priority = stable_hash(
                        &[
                            spec.corpus_id.as_bytes(),
                            doc_hash.as_bytes(),
                            relation.id.as_bytes(),
                        ]
                        .concat(),
                    );
                    reservoirs[corpus_index][relation_index].insert(priority, occurrence);
                    seen[relation_index] += 1;
                }
            }
            docs += 1;
        }
        let stored = std::array::from_fn(|r| reservoirs[corpus_index][r].slots.len());
        source_counts.push(SourceCount {
            corpus_id: spec.corpus_id.clone(),
            docs_scanned: docs,
            relation_occurrences_seen: seen,
            reservoir_contexts: stored,
            candidate_pairs: [0; 3],
        });
    }

    let mut all_occurrences: Vec<Vec<Occurrence>> =
        (0..RELATIONS.len()).map(|_| Vec::new()).collect();
    for corpus_index in 0..roster.corpora.len() {
        for relation_index in 0..RELATIONS.len() {
            let reservoir = std::mem::replace(
                &mut reservoirs[corpus_index][relation_index],
                Reservoir::new(),
            );
            all_occurrences[relation_index].extend(reservoir.into_items());
        }
    }

    let mut pools: Vec<Vec<PairChoice>> = Vec::new();
    let mut per_corpus_pairs = vec![[0u64; 3]; roster.corpora.len()];
    for relation_index in 0..RELATIONS.len() {
        let mut candidates = Vec::new();
        for left in 0..all_occurrences[relation_index].len() {
            for right in (left + 1)..all_occurrences[relation_index].len() {
                let a = &all_occurrences[relation_index][left];
                let b = &all_occurrences[relation_index][right];
                if a.corpus_index != b.corpus_index || a.doc_hash == b.doc_hash {
                    continue;
                }
                let overlap = token_overlap(&a.features.tokens, &b.features.tokens);
                let key = stable_hash(
                    format!(
                        "{}|{}|{}",
                        RELATIONS[relation_index].id, a.doc_hash, b.doc_hash
                    )
                    .as_bytes(),
                );
                candidates.push((left, right, overlap, key, a.corpus_index));
            }
        }
        for (_, _, _, _, corpus_index) in &candidates {
            per_corpus_pairs[*corpus_index][relation_index] += 1;
        }
        let mut by_corpus: Vec<Vec<(usize, usize, f32, u64)>> =
            vec![Vec::new(); roster.corpora.len()];
        for (left, right, overlap, key, corpus_index) in candidates {
            by_corpus[corpus_index].push((left, right, overlap, key));
        }
        let mut low_pool = Vec::new();
        let mut high_pool = Vec::new();
        for corpus_pairs in &mut by_corpus {
            corpus_pairs.sort_by(|a, b| a.2.total_cmp(&b.2).then_with(|| a.3.cmp(&b.3)));
            let n = corpus_pairs.len();
            if n < 4 {
                continue;
            }
            let low_end = (n / 4).max(1);
            let high_start = (n * 3 / 4).min(n - 1);
            low_pool.extend(corpus_pairs[..low_end].iter().map(|p| PairChoice {
                left: p.0,
                right: p.1,
                jaccard: p.2,
                order_key: p.3,
            }));
            high_pool.extend(corpus_pairs[high_start..].iter().map(|p| PairChoice {
                left: p.0,
                right: p.1,
                jaccard: p.2,
                order_key: p.3,
            }));
        }
        low_pool.sort_by_key(|p| p.order_key);
        high_pool.sort_by_key(|p| p.order_key);
        pools.push(low_pool);
        pools.push(high_pool);
    }
    for corpus_index in 0..roster.corpora.len() {
        source_counts[corpus_index].candidate_pairs = per_corpus_pairs[corpus_index];
    }

    let selected = select_pairs(&pools, &all_occurrences);
    ensure!(
        !selected.is_empty(),
        "no natural context pairs were eligible"
    );
    let mut packets = Vec::with_capacity(selected.len());
    let mut ledger = Vec::with_capacity(selected.len());
    let mut used_docs = HashSet::new();
    let mut token_leaks = 0usize;
    for item in selected {
        let relation = RELATIONS[item.relation_index];
        let left = &all_occurrences[item.relation_index][item.choice.left];
        let right = &all_occurrences[item.relation_index][item.choice.right];
        ensure!(
            left.doc_hash != right.doc_hash,
            "same-document context pair"
        );
        ensure!(
            used_docs.insert(left.doc_hash.clone()),
            "document reused across packets"
        );
        ensure!(
            used_docs.insert(right.doc_hash.clone()),
            "document reused across packets"
        );
        let packet_id = packet_id(relation.id, &left.doc_hash, &right.doc_hash);
        let pair_features = pair_features(&left.features, &right.features);
        token_leaks +=
            feature_token_leaks(&left.features, &right.features, &pair_features, relation);
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
            left_document_sha256: left.doc_hash.clone(),
            right_document_sha256: right.doc_hash.clone(),
            left_template_sha256: left.template_hash.clone(),
            right_template_sha256: right.template_hash.clone(),
            lexical_overlap_jaccard: item.choice.jaccard,
            overlap_band: item.band.to_string(),
            split: String::new(),
            left: left.features.clone(),
            right: right.features.clone(),
            pair_features,
        });
    }

    assign_template_splits(&mut ledger);
    ensure!(
        token_leaks == 0,
        "candidate token appeared in compatibility feature vectors"
    );
    validate_packets(&packets, &ledger)?;

    let packet_path = review_dir.join("packets.json");
    write_json(&packet_path, &packets)?;
    let rubric_path = review_dir.join("rubric.md");
    fs::write(&rubric_path, RUBRIC.as_bytes())?;
    let ledger_path = output.join("private-ledger.json");
    write_json(&ledger_path, &ledger)?;
    let receipt = make_receipt(
        &roster,
        &roster_bytes,
        source_counts,
        &ledger,
        used_docs.len(),
        token_leaks,
    )?;
    let receipt_path = output.join("p1n3-pre-review-receipt.json");
    write_json(&receipt_path, &receipt)?;
    let root = make_root(
        repo,
        &protocol,
        &roster_path,
        &packet_path,
        &rubric_path,
        &ledger_path,
        &receipt_path,
    )?;
    write_json(&output.join("pre-review-root.json"), &root)?;
    println!(
        "packets={} fit={} holdout={} docs={} output={}",
        receipt.review_packet_count,
        receipt.fit_packets,
        receipt.holdout_packets,
        used_docs.len(),
        output.display()
    );
    Ok(())
}

const RUBRIC: &str = "# Context-pair review\n\nFor each item, read the displayed word pair and both natural excerpts. Assign exactly one label:\n\n- `SAME`: both contexts express a compatible contextual use of the displayed lexical relation.\n- `DIFFERENT`: the contexts clearly express incompatible contextual uses or senses relevant to that relation.\n- `UNKNOWN`: local text does not establish compatibility or incompatibility, or is genuinely ambiguous.\n\nJudge the two contexts on their own merits. Shared words alone do not establish `SAME`. Do not infer missing meaning from outside information. Modify only the `judgment` field to one of `SAME`, `DIFFERENT`, or `UNKNOWN`; preserve each packet ID, word pair, and context text exactly.\n";

#[derive(Clone)]
struct Selected {
    relation_index: usize,
    band: &'static str,
    choice: PairChoice,
}

fn select_pairs(pools: &[Vec<PairChoice>], occurrences: &[Vec<Occurrence>]) -> Vec<Selected> {
    let mut cursors = vec![0usize; pools.len()];
    let mut selected = Vec::new();
    let mut used_docs: HashSet<String> = HashSet::new();
    let mut selected_by_corpus: HashMap<(usize, usize, usize), usize> = HashMap::new();
    let mut selected_by_pool = vec![0usize; pools.len()];
    let max_rounds = pools.iter().map(Vec::len).sum::<usize>().saturating_add(1);
    for _ in 0..max_rounds {
        let mut progress = false;
        for pool_index in 0..pools.len() {
            if selected_by_pool[pool_index] >= TARGET_PER_BAND {
                continue;
            }
            while cursors[pool_index] < pools[pool_index].len() {
                let choice = pools[pool_index][cursors[pool_index]];
                cursors[pool_index] += 1;
                let relation_index = pool_index / 2;
                let a = &occurrences[relation_index][choice.left];
                let b = &occurrences[relation_index][choice.right];
                if used_docs.contains(&a.doc_hash) || used_docs.contains(&b.doc_hash) {
                    continue;
                }
                let corpus = a.corpus_index;
                let band_index = pool_index % 2;
                let cap_key = (relation_index, band_index, corpus);
                if *selected_by_corpus.get(&cap_key).unwrap_or(&0) >= MAX_PAIRS_PER_CORPUS_BAND {
                    continue;
                }
                used_docs.insert(a.doc_hash.clone());
                used_docs.insert(b.doc_hash.clone());
                *selected_by_corpus.entry(cap_key).or_insert(0) += 1;
                selected_by_pool[pool_index] += 1;
                selected.push(Selected {
                    relation_index,
                    band: if pool_index % 2 == 0 { "low" } else { "high" },
                    choice,
                });
                progress = true;
                break;
            }
        }
        if !progress {
            break;
        }
    }
    selected
}

fn document_hash(title: &str, text: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(title.as_bytes());
    hash.update([0]);
    hash.update(text.as_bytes());
    format!("{:x}", hash.finalize())
}
fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn hash_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("read for hashing {}", path.display()))?;
    Ok(sha256(&bytes))
}
fn packet_id(candidate: &str, left: &str, right: &str) -> String {
    let (a, b) = if left <= right {
        (left, right)
    } else {
        (right, left)
    };
    format!(
        "n3-{}",
        &sha256(&[SALT, candidate.as_bytes(), a.as_bytes(), b.as_bytes()].concat())[..16]
    )
}
fn stable_hash(bytes: &[u8]) -> u64 {
    let digest = Sha256::digest([SALT, bytes].concat());
    u64::from_le_bytes(digest[..8].try_into().expect("digest prefix"))
}
fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let mut file = File::create(path)?;
    serde_json::to_writer_pretty(&mut file, value)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizer_keeps_utf8_boundaries_and_ascii_words() {
        let text = "A river—bank, near water!";
        let words = tokenize(text);
        assert_eq!(
            words
                .iter()
                .map(|t| t.text.to_ascii_lowercase())
                .collect::<Vec<_>>(),
            ["a", "river", "bank", "near", "water"]
        );
        assert_eq!(&text[words[2].start..words[2].end], "bank");
    }

    #[test]
    fn nearest_pair_uses_minimum_distance() {
        let words = tokenize("bank near water then water beside the distant bank");
        let pair = nearest_pair(&words, "bank", "water").expect("pair");
        assert_eq!(pair.2, 2);
        assert_eq!(words[pair.0].text, "bank");
        assert_eq!(words[pair.1].text, "water");
    }

    #[test]
    fn candidate_tokens_are_visible_only_in_the_excerpt() {
        let text = "The bank leaned beside the water after rain.";
        let words = tokenize(text);
        let (left, right, distance) = nearest_pair(&words, "bank", "water").unwrap();
        let (excerpt, _, features) =
            extract_context(text, &words, left, right, distance, RELATIONS[0], false);
        assert!(excerpt.contains("bank") && excerpt.contains("water"));
        let pair = pair_features(&features, &features);
        assert_eq!(
            feature_token_leaks(&features, &features, &pair, RELATIONS[0]),
            0
        );
        assert!(!features.tokens.iter().any(|t| t == "bank" || t == "water"));
    }

    #[test]
    fn exact_overlap_jaccard_uses_unique_non_candidate_tokens() {
        let a = vec!["river".to_string(), "green".to_string()];
        let b = vec!["river".to_string(), "shore".to_string()];
        assert_eq!(token_overlap(&a, &b), 1.0 / 3.0);
    }

    #[test]
    fn template_components_do_not_cross_frozen_split() {
        let mut rows = vec![
            PrivateLedgerRow {
                packet_id: "a".to_string(),
                candidate_id: "bank_to_water".to_string(),
                corpus_id: "hidden".to_string(),
                left_document_sha256: "d1".to_string(),
                right_document_sha256: "d2".to_string(),
                left_template_sha256: "same".to_string(),
                right_template_sha256: "other-a".to_string(),
                lexical_overlap_jaccard: 0.0,
                overlap_band: "low".to_string(),
                split: String::new(),
                left: empty_features(),
                right: empty_features(),
                pair_features: empty_pair_features(),
            },
            PrivateLedgerRow {
                packet_id: "b".to_string(),
                candidate_id: "bank_to_water".to_string(),
                corpus_id: "hidden".to_string(),
                left_document_sha256: "d3".to_string(),
                right_document_sha256: "d4".to_string(),
                left_template_sha256: "same".to_string(),
                right_template_sha256: "other-b".to_string(),
                lexical_overlap_jaccard: 0.8,
                overlap_band: "high".to_string(),
                split: String::new(),
                left: empty_features(),
                right: empty_features(),
                pair_features: empty_pair_features(),
            },
        ];
        assign_template_splits(&mut rows);
        assert_eq!(rows[0].split, rows[1].split);
    }

    fn empty_features() -> ContextFeatures {
        ContextFeatures {
            tokens: Vec::new(),
            role_tokens: Vec::new(),
            bigrams: Vec::new(),
            trigrams: Vec::new(),
            role_counts: [0; 3],
            local_token_count: 0,
            pair_distance: 0,
            distance_bin: 0,
            support_cue: false,
            contradiction_cue: false,
            is_title_field: false,
        }
    }

    fn empty_pair_features() -> PairFeatures {
        PairFeatures {
            shared_tokens: Vec::new(),
            shared_role_tokens: Vec::new(),
            shared_bigrams: Vec::new(),
            shared_trigrams: Vec::new(),
            token_jaccard: 0.0,
            bigram_jaccard: 0.0,
            trigram_jaccard: 0.0,
            role_count_abs_delta: [0; 3],
            token_count_abs_delta: 0,
            distance_bin_abs_delta: 0,
            support_cue_equal: false,
            contradiction_cue_equal: false,
            same_field_kind: false,
        }
    }
}
