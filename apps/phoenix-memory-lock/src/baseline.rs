use std::path::Path;

use anyhow::{bail, Context, Result};
use hashbrown::{HashMap, HashSet};

use crate::artifact::{read_artifact, write_artifact};
use crate::model::{
    EvaluationReceipt, FreezeManifest, GoldArtifact, RankedSession, RetrievalArtifact,
    RetrievalCase, SourceBinding, WorkloadArtifact, GOLD_CONTRACT, GOLD_MAGIC, RETRIEVAL_CONTRACT,
    RETRIEVAL_MAGIC, WORKLOAD_CONTRACT, WORKLOAD_MAGIC,
};

const ENGINE: &str = "phoenix-bm25-session-v1";
const K1: f64 = 1.2;
const B: f64 = 0.75;

pub fn run(
    manifest: &FreezeManifest,
    workload_path: &Path,
    output_path: &Path,
    top_k: usize,
) -> Result<BaselineReceipt> {
    if top_k == 0 || top_k > 1_024 {
        bail!("top-k must be in 1..=1024");
    }
    let workload: WorkloadArtifact = read_artifact(workload_path, WORKLOAD_MAGIC)?;
    if workload.contract != WORKLOAD_CONTRACT {
        bail!("unsupported workload contract {}", workload.contract);
    }
    verify_binding(manifest, &workload.source)?;
    let mut output_cases = Vec::with_capacity(workload.cases.len());
    for case in &workload.cases {
        let query = unique_tokens(&case.question);
        let mut documents = Vec::with_capacity(case.sessions.len());
        let mut document_frequency = HashMap::<u64, u32>::new();
        let mut total_terms = 0_u64;
        for session in &case.sessions {
            let mut tokens = Vec::new();
            for turn in &session.turns {
                append_tokens(&turn.content, &mut tokens);
            }
            tokens.sort_unstable();
            let term_count = u32::try_from(tokens.len()).context("session term count overflow")?;
            total_terms += u64::from(term_count);
            let counts = compress_counts(&tokens);
            for &(term, _) in &counts {
                *document_frequency.entry(term).or_insert(0) += 1;
            }
            documents.push(SessionDocument {
                stable_id: &session.stable_id,
                term_count,
                counts,
            });
        }
        let document_count = documents.len() as f64;
        let average_length = if documents.is_empty() {
            1.0
        } else {
            total_terms as f64 / document_count
        };
        let mut ranked = documents
            .iter()
            .map(|document| {
                let score = bm25_score(
                    document,
                    &query,
                    &document_frequency,
                    document_count,
                    average_length,
                );
                (document.stable_id, score)
            })
            .collect::<Vec<_>>();
        ranked.sort_unstable_by(|left, right| {
            right.1.total_cmp(&left.1).then_with(|| left.0.cmp(right.0))
        });
        ranked.truncate(top_k.min(ranked.len()));
        output_cases.push(RetrievalCase {
            question_id: case.question_id.clone(),
            ranked_sessions: ranked
                .into_iter()
                .map(|(stable_id, score)| RankedSession {
                    stable_id: stable_id.to_owned(),
                    score_bits: score.to_bits(),
                })
                .collect(),
        });
    }
    let case_count = output_cases.len();
    let artifact = RetrievalArtifact {
        contract: RETRIEVAL_CONTRACT.to_owned(),
        source: workload.source,
        engine: ENGINE.to_owned(),
        top_k: u32::try_from(top_k).context("top-k overflow")?,
        cases: output_cases,
    };
    write_artifact(output_path, RETRIEVAL_MAGIC, &artifact)?;
    Ok(BaselineReceipt {
        engine: ENGINE,
        cases: case_count,
        top_k,
        output_path: output_path.display().to_string(),
    })
}

pub fn evaluate(
    manifest: &FreezeManifest,
    gold_path: &Path,
    retrieval_path: &Path,
) -> Result<EvaluationReceipt> {
    let gold: GoldArtifact = read_artifact(gold_path, GOLD_MAGIC)?;
    let retrieval: RetrievalArtifact = read_artifact(retrieval_path, RETRIEVAL_MAGIC)?;
    if gold.contract != GOLD_CONTRACT || retrieval.contract != RETRIEVAL_CONTRACT {
        bail!("unsupported evaluation artifact contract");
    }
    if gold.source != retrieval.source {
        bail!("gold and retrieval artifacts are bound to different sources");
    }
    verify_binding(manifest, &gold.source)?;
    let by_question = retrieval
        .cases
        .iter()
        .map(|case| (case.question_id.as_str(), case))
        .collect::<HashMap<_, _>>();
    let mut answerable = 0_usize;
    let mut hits = 0_usize;
    let mut reciprocal_rank = 0.0_f64;
    for expected in &gold.cases {
        if expected.answer_session_ids.is_empty() {
            continue;
        }
        answerable += 1;
        let actual = by_question
            .get(expected.question_id.as_str())
            .with_context(|| format!("missing retrieval case {}", expected.question_id))?;
        let expected_ids = expected
            .answer_session_ids
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        if let Some(rank) = actual
            .ranked_sessions
            .iter()
            .position(|session| expected_ids.contains(session.stable_id.as_str()))
        {
            hits += 1;
            reciprocal_rank += 1.0 / (rank + 1) as f64;
        }
    }
    let denominator = answerable.max(1) as f64;
    Ok(EvaluationReceipt {
        contract: "phoenix.memory.longmemeval-evaluation/v1",
        freeze_id: gold.source.freeze_id,
        source_sha256: gold.source.sha256,
        engine: retrieval.engine,
        cases: gold.cases.len(),
        answerable_cases: answerable,
        hit_at_k: hits as f64 / denominator,
        mean_reciprocal_rank: reciprocal_rank / denominator,
    })
}

pub(crate) fn verify_binding(manifest: &FreezeManifest, binding: &SourceBinding) -> Result<()> {
    if binding.freeze_id != manifest.freeze_id {
        bail!("artifact freeze ID does not match manifest");
    }
    let dataset = manifest
        .benchmark
        .datasets
        .iter()
        .find(|dataset| dataset.variant == binding.variant)
        .context("artifact variant is not frozen")?;
    if binding.filename != dataset.filename
        || binding.bytes != dataset.bytes
        || binding.sha256 != dataset.sha256
    {
        bail!("artifact source binding does not match manifest");
    }
    Ok(())
}

fn bm25_score(
    document: &SessionDocument<'_>,
    query: &[u64],
    document_frequency: &HashMap<u64, u32>,
    document_count: f64,
    average_length: f64,
) -> f64 {
    let length_ratio = f64::from(document.term_count) / average_length;
    query
        .iter()
        .map(|term| {
            let frequency = document
                .counts
                .binary_search_by_key(term, |&(candidate, _)| candidate)
                .ok()
                .map(|index| document.counts[index].1)
                .unwrap_or(0);
            if frequency == 0 {
                return 0.0;
            }
            let df = f64::from(document_frequency.get(term).copied().unwrap_or(0));
            let idf = (1.0 + (document_count - df + 0.5) / (df + 0.5)).ln();
            let tf = f64::from(frequency);
            idf * (tf * (K1 + 1.0)) / (tf + K1 * (1.0 - B + B * length_ratio))
        })
        .sum()
}

fn unique_tokens(text: &str) -> Vec<u64> {
    let mut tokens = Vec::new();
    append_tokens(text, &mut tokens);
    tokens.sort_unstable();
    tokens.dedup();
    tokens
}

fn append_tokens(text: &str, output: &mut Vec<u64>) {
    for token in text
        .as_bytes()
        .split(|byte| !byte.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
    {
        let mut hash = 0xcbf29ce484222325_u64;
        for &byte in token {
            hash ^= u64::from(byte.to_ascii_lowercase());
            hash = hash.wrapping_mul(0x100000001b3);
        }
        output.push(hash);
    }
}

fn compress_counts(tokens: &[u64]) -> Vec<(u64, u32)> {
    let mut counts = Vec::with_capacity(tokens.len());
    for &token in tokens {
        if let Some((last, count)) = counts.last_mut() {
            if *last == token {
                *count += 1;
                continue;
            }
        }
        counts.push((token, 1));
    }
    counts
}

struct SessionDocument<'a> {
    stable_id: &'a str,
    term_count: u32,
    counts: Vec<(u64, u32)>,
}

#[derive(Debug, serde::Serialize)]
pub struct BaselineReceipt {
    pub engine: &'static str,
    pub cases: usize,
    pub top_k: usize,
    pub output_path: String,
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;
    use crate::artifact::write_artifact;
    use crate::model::{
        GoldCase, HistorySession, HistoryTurn, WorkloadCase, GOLD_CONTRACT, WORKLOAD_CONTRACT,
    };
    use crate::verify;

    #[test]
    fn baseline_is_deterministic_and_cannot_open_gold() {
        let manifest_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../memory-lock/longmemeval-cleaned-v1.json");
        let (manifest, _) =
            verify::load_and_verify(&manifest_path).expect("frozen manifest verifies");
        let dataset = &manifest.benchmark.datasets[0];
        let binding = SourceBinding {
            freeze_id: manifest.freeze_id.clone(),
            variant: dataset.variant.clone(),
            filename: dataset.filename.clone(),
            bytes: dataset.bytes,
            sha256: dataset.sha256.clone(),
        };
        let workload = WorkloadArtifact {
            contract: WORKLOAD_CONTRACT.to_owned(),
            source: binding.clone(),
            cases: vec![WorkloadCase {
                question_id: "q1".to_owned(),
                question_type: "single-session-user".to_owned(),
                question: "Where did I see the phoenix?".to_owned(),
                question_date: "2026-07-29".to_owned(),
                sessions: vec![
                    session("s2", "We discussed the ocean."),
                    session("s1", "You saw the phoenix in Rome."),
                ],
            }],
        };
        let gold = GoldArtifact {
            contract: GOLD_CONTRACT.to_owned(),
            source: binding,
            cases: vec![GoldCase {
                question_id: "q1".to_owned(),
                question_type: "single-session-user".to_owned(),
                answer: "Rome".to_owned(),
                answer_session_ids: vec!["s1".to_owned()],
            }],
        };
        let directory = tempdir().expect("temp directory");
        let workload_path = directory.path().join("workload.plmw");
        let gold_path = directory.path().join("gold.plmg");
        let first = directory.path().join("first.plmr");
        let second = directory.path().join("second.plmr");
        write_artifact(&workload_path, WORKLOAD_MAGIC, &workload).expect("write workload");
        write_artifact(&gold_path, GOLD_MAGIC, &gold).expect("write gold");
        run(&manifest, &workload_path, &first, 2).expect("first baseline");
        run(&manifest, &workload_path, &second, 2).expect("second baseline");
        assert_eq!(
            fs::read(&first).expect("read first"),
            fs::read(&second).expect("read second")
        );
        let receipt = evaluate(&manifest, &gold_path, &first).expect("evaluate");
        assert_eq!(receipt.hit_at_k, 1.0);
        let error = run(
            &manifest,
            &gold_path,
            &directory.path().join("forbidden.plmr"),
            2,
        )
        .expect_err("gold cannot enter baseline");
        assert!(error.to_string().contains("artifact type mismatch"));
    }

    #[test]
    fn package_has_no_mentedb_dependency() {
        let cargo = include_str!("../Cargo.toml").to_ascii_lowercase();
        assert!(!cargo.contains("mentedb"));
    }

    fn session(id: &str, content: &str) -> HistorySession {
        HistorySession {
            stable_id: id.to_owned(),
            date: "2026-07-28".to_owned(),
            turns: vec![HistoryTurn {
                role: "user".to_owned(),
                content: content.to_owned(),
            }],
        }
    }
}
