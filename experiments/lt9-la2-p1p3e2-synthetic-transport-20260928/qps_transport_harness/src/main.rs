//! Full-corpus QPS search lanes for the 17 reviewed E2 retrieval gaps.
//! The output is consumed by score_qps_lanes.py, which applies the frozen gate.
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::env;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{bail, Context, Result};
use hashbrown::{HashMap, HashSet};
use memchr::{memchr2_iter, memchr_iter};
use memmap2::{Mmap, MmapOptions};
use phoenix_lexical_qps::{
    DocumentInput, Expansion, FieldConfig, QpsBuilder, QpsConfig, QpsError, QpsIndex, QueryGroup,
    SearchHit, SearchScratch, MAXIMUM_QUERY_GROUPS,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const TOP_K: usize = 100;
const ALPHA: f32 = 0.5;

#[derive(Deserialize)]
struct Cohort {
    datasets: Vec<DatasetSpec>,
}
#[derive(Deserialize)]
struct DatasetSpec {
    name: String,
    root: PathBuf,
}
#[derive(Clone, Deserialize, Serialize)]
struct Candidate {
    dataset: String,
    qrels_splits: Vec<String>,
    query_id: String,
    document_id: String,
    document_ordinal: u64,
    candidate_id: String,
    direction: String,
    qrels_grade: i32,
    query_text: String,
    document_title: String,
    document_excerpt: String,
}
#[derive(Deserialize)]
struct Opportunity {
    candidate: Candidate,
    baseline_rank: Option<usize>,
    baseline_status: String,
}
#[derive(Deserialize)]
struct CorpusRow<'a> {
    #[serde(rename = "_id", borrow)]
    id: Cow<'a, str>,
    #[serde(default, borrow)]
    title: Option<Cow<'a, str>>,
    #[serde(default, borrow)]
    text: Option<Cow<'a, str>>,
}
#[derive(Serialize)]
struct Hit {
    ordinal: u64,
    score: f32,
}
#[derive(Serialize)]
struct ContextHit {
    ordinal: u64,
    score: f32,
    masked_context: String,
}
#[derive(Serialize)]
struct LaneRow {
    candidate: Candidate,
    baseline_status: String,
    prior_baseline_rank: Option<usize>,
    query_masked_context: String,
    baseline_hits: Vec<Hit>,
    expanded_hits: Vec<Hit>,
    new_context_hits: Vec<ContextHit>,
    document_ids: BTreeMap<u64, String>,
    baseline_microseconds: u64,
    expanded_microseconds: u64,
    status: &'static str,
}

fn mapped(path: &Path) -> Result<Mmap> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    // SAFETY: benchmark inputs are immutable frozen JSONL files.
    unsafe { MmapOptions::new().map(&file) }.context("mmap corpus")
}

fn lines(map: &Mmap) -> impl Iterator<Item = (u64, &[u8])> {
    let mut ordinal = 0_u64;
    let mut start = 0_usize;
    memchr_iter(b'\n', map)
        .chain(std::iter::once(map.len()))
        .filter_map(move |end| {
            let row = map
                .get(start..end)?
                .strip_suffix(b"\r")
                .unwrap_or(&map[start..end]);
            start = end.saturating_add(1);
            let current = ordinal;
            ordinal += 1;
            (!row.is_empty()).then_some((current, row))
        })
}

fn build_index(map: &Mmap) -> Result<QpsIndex> {
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
    for (ordinal, line) in lines(map) {
        let row: CorpusRow<'_> = serde_json::from_slice(line)?;
        builder.insert(DocumentInput {
            external_id: ordinal,
            fields: &[
                row.title.as_deref().unwrap_or_default(),
                row.text.as_deref().unwrap_or_default(),
            ],
        })?;
    }
    Ok(builder.build()?)
}

fn words(text: &str) -> Vec<String> {
    text.split(|ch: char| !ch.is_alphanumeric() && ch != '_')
        .filter(|part| !part.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn source_target(direction: &str) -> Result<(&str, &str)> {
    direction.split_once("->").context("direction lacks arrow")
}

fn search(
    index: &QpsIndex,
    query: &str,
    source: &str,
    target: Option<&str>,
    scratch: &mut SearchScratch,
    hits: &mut Vec<SearchHit>,
) -> Result<bool> {
    let tokens = words(query);
    if tokens.is_empty()
        || tokens.len() > MAXIMUM_QUERY_GROUPS
        || !tokens.iter().any(|token| token == source)
    {
        return Ok(false);
    }
    let expansions: Vec<Vec<Expansion<'_>>> = tokens
        .iter()
        .map(|token| {
            if token == source {
                match target {
                    Some(target) => vec![
                        Expansion {
                            term: token,
                            quality: 1.0,
                        },
                        Expansion {
                            term: target,
                            quality: ALPHA,
                        },
                    ],
                    None => vec![Expansion {
                        term: token,
                        quality: 1.0,
                    }],
                }
            } else {
                vec![Expansion {
                    term: token,
                    quality: 1.0,
                }]
            }
        })
        .collect();
    let groups: Vec<QueryGroup<'_>> = expansions
        .iter()
        .map(|parts| QueryGroup { expansions: parts })
        .collect();
    hits.clear();
    match index.search_groups_into(&groups, TOP_K, scratch, hits) {
        Ok(_) => Ok(true),
        Err(QpsError::QueryTooLarge) => Ok(false),
        Err(err) => Err(err.into()),
    }
}

fn mask_focal(text: &str, focal: &str) -> Option<String> {
    let haystack = text.as_bytes();
    let needle = focal.as_bytes();
    if needle.is_empty() {
        return None;
    }
    for byte in memchr2_iter(
        needle[0].to_ascii_lowercase(),
        needle[0].to_ascii_uppercase(),
        haystack,
    ) {
        let end = byte + focal.len();
        if end > haystack.len() || !haystack[byte..end].eq_ignore_ascii_case(needle) {
            continue;
        }
        if byte > 0 && haystack[byte - 1].is_ascii_alphanumeric() {
            continue;
        }
        if end < haystack.len() && haystack[end].is_ascii_alphanumeric() {
            continue;
        }
        let left = byte.saturating_sub(140);
        let right = (end + 140).min(text.len());
        // Corpus text is UTF-8; clamp excerpt boundaries to character boundaries.
        let left = (left..=byte)
            .find(|&i| text.is_char_boundary(i))
            .unwrap_or(byte);
        let right = (end..=right)
            .rev()
            .find(|&i| text.is_char_boundary(i))
            .unwrap_or(end);
        return Some(format!("{}[FOCAL]{}", &text[left..byte], &text[end..right]));
    }
    None
}

fn compact_hits(hits: &[SearchHit]) -> Vec<Hit> {
    hits.iter()
        .map(|hit| Hit {
            ordinal: hit.external_id,
            score: hit.score,
        })
        .collect()
}

fn main() -> Result<()> {
    let args: Vec<_> = env::args_os().collect();
    if args.len() != 4 {
        bail!("usage: weighted-qps-transport COHORT OPPORTUNITY_JSONL NEW_OUTPUT_DIR");
    }
    let cohort: Cohort = serde_json::from_slice(&fs::read(&args[1])?)?;
    let input = fs::read(&args[2])?;
    let out = PathBuf::from(&args[3]);
    if out.exists() {
        bail!("output already exists: {}", out.display());
    }
    let mut opportunities = Vec::new();
    for line in input
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        opportunities.push(serde_json::from_slice::<Opportunity>(line)?);
    }
    fs::create_dir_all(&out)?;
    let mut writer = BufWriter::new(File::create(out.join("qps-transport-searches.jsonl"))?);
    let mut summary = BTreeMap::new();
    for spec in cohort.datasets {
        let rows: Vec<_> = opportunities
            .iter()
            .filter(|row| row.candidate.dataset == spec.name)
            .collect();
        if rows.is_empty() {
            continue;
        }
        eprintln!(
            "QPS building {}: {} transport probes",
            spec.name,
            rows.len()
        );
        let map = mapped(&spec.root.join("corpus.jsonl"))?;
        let index = build_index(&map)?;
        let mut scratch = SearchScratch::default();
        let (mut baseline, mut expanded) = (Vec::with_capacity(TOP_K), Vec::with_capacity(TOP_K));
        let mut rendered = Vec::<LaneRow>::with_capacity(rows.len());
        let mut wanted = HashSet::new();
        for row in rows {
            let candidate = &row.candidate;
            let (source, target) = source_target(&candidate.direction)?;
            let Some(query_context) = mask_focal(&candidate.query_text, source) else {
                rendered.push(LaneRow {
                    candidate: candidate.clone(),
                    baseline_status: row.baseline_status.clone(),
                    prior_baseline_rank: row.baseline_rank,
                    query_masked_context: String::new(),
                    baseline_hits: vec![],
                    expanded_hits: vec![],
                    new_context_hits: vec![],
                    document_ids: BTreeMap::new(),
                    baseline_microseconds: 0,
                    expanded_microseconds: 0,
                    status: "NO_SOURCE_IN_QUERY",
                });
                continue;
            };
            let start = Instant::now();
            let valid = search(
                &index,
                &candidate.query_text,
                source,
                None,
                &mut scratch,
                &mut baseline,
            )?;
            let base_micros = start.elapsed().as_micros() as u64;
            if !valid {
                baseline.clear();
                expanded.clear();
            }
            let start = Instant::now();
            if valid {
                search(
                    &index,
                    &candidate.query_text,
                    source,
                    Some(target),
                    &mut scratch,
                    &mut expanded,
                )?;
            }
            let expanded_micros = start.elapsed().as_micros() as u64;
            for hit in baseline.iter().chain(expanded.iter()) {
                wanted.insert(hit.external_id);
            }
            rendered.push(LaneRow {
                candidate: candidate.clone(),
                baseline_status: row.baseline_status.clone(),
                prior_baseline_rank: row.baseline_rank,
                query_masked_context: query_context,
                baseline_hits: compact_hits(&baseline),
                expanded_hits: compact_hits(&expanded),
                new_context_hits: Vec::new(),
                baseline_microseconds: base_micros,
                document_ids: BTreeMap::new(),
                expanded_microseconds: expanded_micros,
                status: if valid {
                    "SEARCHED"
                } else {
                    "QUERY_UNSUPPORTED"
                },
            });
        }
        let mut docs = HashMap::<u64, (String, String, String)>::with_capacity(wanted.len());
        for (ordinal, line) in lines(&map) {
            if !wanted.contains(&ordinal) {
                continue;
            }
            let row: CorpusRow<'_> = serde_json::from_slice(line)?;
            docs.insert(
                ordinal,
                (
                    row.id.into_owned(),
                    row.title.unwrap_or_default().into_owned(),
                    row.text.unwrap_or_default().into_owned(),
                ),
            );
            if docs.len() == wanted.len() {
                break;
            }
        }
        for row in &mut rendered {
            let (_, target) = source_target(&row.candidate.direction)?;
            let base_ids: HashSet<_> = row.baseline_hits.iter().map(|hit| hit.ordinal).collect();
            for hit in row.baseline_hits.iter().chain(row.expanded_hits.iter()) {
                if let Some((id, _, _)) = docs.get(&hit.ordinal) {
                    row.document_ids.insert(hit.ordinal, id.clone());
                }
            }
            for hit in &row.expanded_hits {
                if base_ids.contains(&hit.ordinal) {
                    continue;
                }
                let Some((_, title, body)) = docs.get(&hit.ordinal) else {
                    continue;
                };
                let excerpt = mask_focal(title, target).or_else(|| mask_focal(body, target));
                if let Some(masked_context) = excerpt {
                    row.new_context_hits.push(ContextHit {
                        ordinal: hit.ordinal,
                        score: hit.score,
                        masked_context,
                    });
                }
            }
            serde_json::to_writer(&mut writer, row)?;
            writer.write_all(b"\n")?;
        }
        summary.insert(
            spec.name,
            serde_json::json!({
                "search_rows": rendered.len(), "new_doc_contexts": wanted.len(),
                "corpus_sha256": format!("{:x}", Sha256::digest(&map[..])),
                "corpus_bytes": map.len(),
            }),
        );
        eprintln!("QPS finished dataset");
        drop(index);
    }
    writer.flush()?;
    fs::write(
        out.join("qps-search-receipt.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema": "phoenix.lexical.weighted-gate-qps-search/v1",
            "status": "FULL_CORPUS_QPS_LANES_SEARCHED_NO_AUTHORITY_CHANGE",
            "alpha": ALPHA, "top_k": TOP_K, "datasets": summary,
            "opportunities_sha256": format!("{:x}", Sha256::digest(&input)),
        }))?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{mask_focal, source_target, words};

    #[test]
    fn masks_whole_candidate_and_preserves_local_context() {
        assert_eq!(
            mask_focal("The vehicle is here", "vehicle"),
            Some("The [FOCAL] is here".to_owned())
        );
        assert_eq!(mask_focal("The vehicles are here", "vehicle"), None);
        assert_eq!(
            mask_focal("Élan: VEHICLE arrived", "vehicle"),
            Some("Élan: [FOCAL] arrived".to_owned())
        );
    }

    #[test]
    fn direction_and_query_tokenization_remain_separate() {
        assert_eq!(source_target("vehicle->car").unwrap(), ("vehicle", "car"));
        assert_eq!(
            words("A vehicle-based query?"),
            vec!["a", "vehicle", "based", "query"]
        );
    }
}
