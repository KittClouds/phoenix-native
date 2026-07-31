use std::borrow::Cow;
use std::fs::File;
use std::path::Path;

use anyhow::{bail, Context, Result};
use memmap2::Mmap;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::artifact::write_artifact;
use crate::model::{
    DatasetLock, FreezeManifest, GoldArtifact, GoldCase, HistorySession, HistoryTurn,
    SourceBinding, WorkloadArtifact, WorkloadCase, GOLD_CONTRACT, GOLD_MAGIC, WORKLOAD_CONTRACT,
    WORKLOAD_MAGIC,
};

pub fn prepare(
    manifest: &FreezeManifest,
    variant: &str,
    source_path: &Path,
    workload_path: &Path,
    gold_path: &Path,
) -> Result<PrepareReceipt> {
    let lock = manifest
        .benchmark
        .datasets
        .iter()
        .find(|dataset| dataset.variant == variant)
        .with_context(|| format!("variant {variant:?} is not frozen"))?;
    if variant != manifest.benchmark.baseline_variant {
        bail!(
            "Cut 0 prepares only baseline variant {:?}; {:?} is identity-locked but not enabled",
            manifest.benchmark.baseline_variant,
            variant
        );
    }
    let mapped = verified_source(source_path, lock)?;
    let raw: Vec<RawCase<'_>> =
        serde_json::from_slice(&mapped).context("decode cleaned LongMemEval source")?;
    let binding = SourceBinding {
        freeze_id: manifest.freeze_id.clone(),
        variant: lock.variant.clone(),
        filename: lock.filename.clone(),
        bytes: lock.bytes,
        sha256: lock.sha256.clone(),
    };
    let (workload_cases, gold_cases) = split_cases(raw)?;
    let case_count = workload_cases.len();
    let workload = WorkloadArtifact {
        contract: WORKLOAD_CONTRACT.to_owned(),
        source: binding.clone(),
        cases: workload_cases,
    };
    let gold = GoldArtifact {
        contract: GOLD_CONTRACT.to_owned(),
        source: binding,
        cases: gold_cases,
    };
    write_artifact(workload_path, WORKLOAD_MAGIC, &workload)?;
    write_artifact(gold_path, GOLD_MAGIC, &gold)?;
    Ok(PrepareReceipt {
        cases: case_count,
        source_sha256: lock.sha256.clone(),
        workload_path: workload_path.display().to_string(),
        gold_path: gold_path.display().to_string(),
    })
}

fn split_cases(raw: Vec<RawCase<'_>>) -> Result<(Vec<WorkloadCase>, Vec<GoldCase>)> {
    let mut workload_cases = Vec::with_capacity(raw.len());
    let mut gold_cases = Vec::with_capacity(raw.len());
    for case in raw {
        if case.haystack_session_ids.len() != case.haystack_sessions.len()
            || case.haystack_dates.len() != case.haystack_sessions.len()
        {
            bail!(
                "case {} has mismatched session IDs, dates, and bodies",
                case.question_id
            );
        }
        let sessions = case
            .haystack_sessions
            .into_iter()
            .zip(case.haystack_session_ids)
            .zip(case.haystack_dates)
            .map(|((turns, stable_id), date)| HistorySession {
                stable_id: stable_id.into_owned(),
                date: date.into_owned(),
                turns: turns
                    .into_iter()
                    .map(|turn| {
                        let _gold_marker_was_present = turn.has_answer;
                        HistoryTurn {
                            role: turn.role.into_owned(),
                            content: turn.content.into_owned(),
                        }
                    })
                    .collect(),
            })
            .collect();
        workload_cases.push(WorkloadCase {
            question_id: case.question_id.clone().into_owned(),
            question_type: case.question_type.clone().into_owned(),
            question: case.question.into_owned(),
            question_date: case.question_date.into_owned(),
            sessions,
        });
        gold_cases.push(GoldCase {
            question_id: case.question_id.into_owned(),
            question_type: case.question_type.into_owned(),
            answer: canonical_answer(case.answer)?,
            answer_session_ids: case
                .answer_session_ids
                .into_iter()
                .map(Cow::into_owned)
                .collect(),
        });
    }
    Ok((workload_cases, gold_cases))
}

fn canonical_answer(answer: serde_json::Value) -> Result<String> {
    match answer {
        serde_json::Value::String(answer) => Ok(answer),
        other => serde_json::to_string(&other).context("encode non-string gold answer"),
    }
}

fn verified_source(path: &Path, lock: &DatasetLock) -> Result<Mmap> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let metadata = file.metadata()?;
    if metadata.len() != lock.bytes {
        bail!(
            "source size mismatch: expected {}, found {}",
            lock.bytes,
            metadata.len()
        );
    }
    // SAFETY: read-only mapping; the source file is never opened for mutation
    // by this harness.
    let mapped = unsafe { Mmap::map(&file) }.context("map source dataset")?;
    let actual = format!("{:x}", Sha256::digest(&mapped));
    if actual != lock.sha256 {
        bail!(
            "source SHA-256 mismatch: expected {}, found {actual}",
            lock.sha256
        );
    }
    Ok(mapped)
}

#[derive(Debug, serde::Serialize)]
pub struct PrepareReceipt {
    pub cases: usize,
    pub source_sha256: String,
    pub workload_path: String,
    pub gold_path: String,
}

#[derive(Debug, Deserialize)]
struct RawCase<'a> {
    #[serde(borrow)]
    question_id: Cow<'a, str>,
    #[serde(borrow)]
    question_type: Cow<'a, str>,
    #[serde(borrow)]
    question: Cow<'a, str>,
    answer: serde_json::Value,
    #[serde(borrow)]
    question_date: Cow<'a, str>,
    #[serde(borrow)]
    haystack_session_ids: Vec<Cow<'a, str>>,
    #[serde(borrow)]
    haystack_dates: Vec<Cow<'a, str>>,
    #[serde(borrow)]
    haystack_sessions: Vec<Vec<RawTurn<'a>>>,
    #[serde(borrow)]
    answer_session_ids: Vec<Cow<'a, str>>,
}

#[derive(Debug, Deserialize)]
struct RawTurn<'a> {
    #[serde(borrow)]
    role: Cow<'a, str>,
    #[serde(borrow)]
    content: Cow<'a, str>,
    #[serde(default)]
    has_answer: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_removes_all_gold_fields_from_workload() {
        let source = br#"[{
          "question_id":"q1",
          "question_type":"single-session-user",
          "question":"Where?",
          "answer":"SECRET_GOLD_4f8f",
          "question_date":"2026-07-29",
          "haystack_session_ids":["s1"],
          "haystack_dates":["2026-07-28"],
          "haystack_sessions":[[{
            "role":"user",
            "content":"I visited Rome.",
            "has_answer":true
          }]],
          "answer_session_ids":["s1"]
        }]"#;
        let raw: Vec<RawCase<'_>> = serde_json::from_slice(source).expect("fixture decodes");
        let (workload, gold) = split_cases(raw).expect("split succeeds");
        let workload_bytes = postcard::to_allocvec(&workload).expect("workload encodes");
        let gold_bytes = postcard::to_allocvec(&gold).expect("gold encodes");
        assert!(!contains(&workload_bytes, b"SECRET_GOLD_4f8f"));
        assert!(!contains(&workload_bytes, b"answer_session_ids"));
        assert!(!contains(&workload_bytes, b"has_answer"));
        assert!(contains(&gold_bytes, b"SECRET_GOLD_4f8f"));
        assert_eq!(workload[0].sessions[0].turns[0].content, "I visited Rome.");
        assert_eq!(gold[0].answer_session_ids, ["s1"]);
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack
            .windows(needle.len())
            .any(|candidate| candidate == needle)
    }
}
