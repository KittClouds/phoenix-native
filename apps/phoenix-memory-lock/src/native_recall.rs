use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use hashbrown::HashMap;
use phoenix_app_core::ResidentMemory;
use phoenix_memory_contract::{deterministic_id, ParticipantRole, SourceId};
use phoenix_memory_coordinator::{
    CommittedTurn, ConversationKey, IngestTurn, IngestionOrigin, MemoryScope, PendingTurn,
    RecallTurn, MAX_CONTEXT_ITEMS,
};

use crate::artifact::{read_artifact, write_artifact};
use crate::baseline::verify_binding;
use crate::model::{
    FreezeManifest, HistorySession, HistoryTurn, RankedSession, RetrievalArtifact, RetrievalCase,
    WorkloadArtifact, RETRIEVAL_CONTRACT, RETRIEVAL_MAGIC, WORKLOAD_CONTRACT, WORKLOAD_MAGIC,
};

const ENGINE: &str = "phoenix.memory.production-recall/v3";

pub fn run(
    manifest: &FreezeManifest,
    workload_path: &Path,
    output_path: &Path,
    top_k: usize,
) -> Result<NativeRecallReceipt> {
    if top_k == 0 || top_k > MAX_CONTEXT_ITEMS {
        bail!("top-k must be in 1..={MAX_CONTEXT_ITEMS}");
    }
    let workload: WorkloadArtifact = read_artifact(workload_path, WORKLOAD_MAGIC)?;
    if workload.contract != WORKLOAD_CONTRACT {
        bail!("unsupported workload contract {}", workload.contract);
    }
    verify_binding(manifest, &workload.source)?;

    let authority_root = authority_root(output_path)?;
    let mut output_cases = Vec::with_capacity(workload.cases.len());
    let mut published_generations = 0_u64;
    let mut maximum_queue_high_water = 0_u64;
    for (case_index, case) in workload.cases.iter().enumerate() {
        let case_root = authority_root.join(format!("case-{case_index:06}"));
        let workspace_path = case_root.join("workspace.anchor");
        let memory = ResidentMemory::open_with_context_limit(&workspace_path, 0, top_k)
            .with_context(|| format!("open production memory for {}", case.question_id))?;
        let source_bindings = ingest_history(&memory, &case.sessions)?;
        published_generations = published_generations.saturating_add(
            case.sessions
                .iter()
                .map(|session| session.turns.len() as u64)
                .sum::<u64>(),
        );
        memory.set_scope(MemoryScope::Workspace)?;
        let packet = memory.recall(recall_request(case)?)?;
        maximum_queue_high_water =
            maximum_queue_high_water.max(memory.snapshot()?.commands.queue_high_water);
        output_cases.push(RetrievalCase {
            question_id: case.question_id.clone(),
            ranked_sessions: rank_sessions(&packet.items, &source_bindings, top_k),
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
    Ok(NativeRecallReceipt {
        engine: ENGINE,
        cases: case_count,
        top_k,
        published_generations,
        maximum_queue_high_water,
        output_path: output_path.display().to_string(),
        authority_root: authority_root.display().to_string(),
    })
}

fn ingest_history(
    memory: &ResidentMemory,
    sessions: &[HistorySession],
) -> Result<HashMap<SourceId, String>> {
    let mut bindings = HashMap::with_capacity(sessions.len());
    for session in sessions {
        let external_id: Arc<[u8]> = Arc::from(session.stable_id.as_bytes());
        let source_id = memory.conversation_source_id(&external_id);
        if bindings
            .insert(source_id, session.stable_id.clone())
            .is_some()
        {
            bail!(
                "duplicate or colliding session identity {}",
                session.stable_id
            );
        }
        let started_at_millis = parse_date_millis(&session.date)
            .with_context(|| format!("invalid session date for {}", session.stable_id))?;
        let conversation = ConversationKey {
            external_id: Arc::clone(&external_id),
            started_at_millis,
        };
        for (ordinal, turn) in session.turns.iter().enumerate() {
            let ordinal = u32::try_from(ordinal).context("turn ordinal overflow")?;
            memory.ingest_turn(IngestTurn {
                conversation: conversation.clone(),
                committed_turn: committed_turn(
                    &session.stable_id,
                    started_at_millis,
                    ordinal,
                    turn,
                )?,
            })?;
        }
    }
    Ok(bindings)
}

fn committed_turn(
    session_id: &str,
    started_at_millis: i64,
    ordinal: u32,
    turn: &HistoryTurn,
) -> Result<CommittedTurn> {
    let role = participant_role(&turn.role);
    let external_id = format!("{session_id}/{ordinal}");
    let event_time_millis = started_at_millis
        .checked_add(i64::from(ordinal))
        .context("turn timestamp overflow")?;
    Ok(CommittedTurn {
        external_id: Arc::from(external_id.into_bytes()),
        ordinal,
        role,
        event_time_millis,
        reply_to_ordinal: ordinal.checked_sub(1),
        actor_entity_id: deterministic_id(
            b"longmemeval/participant",
            &[session_id.as_bytes(), turn.role.as_bytes()],
        ),
        model_identity_index: None,
        content: Arc::from(turn.content.as_str()),
        origin: IngestionOrigin::LongMemEvalHistory,
    })
}

fn recall_request(case: &crate::model::WorkloadCase) -> Result<RecallTurn> {
    let question_time = parse_date_millis(&case.question_date)
        .with_context(|| format!("invalid question date for {}", case.question_id))?;
    let external_id: Arc<[u8]> =
        Arc::from(format!("longmemeval/query/{}", case.question_id).into_bytes());
    Ok(RecallTurn {
        conversation: ConversationKey {
            external_id: Arc::clone(&external_id),
            started_at_millis: question_time,
        },
        pending_turn: PendingTurn {
            external_id,
            ordinal: 0,
            role: ParticipantRole::User,
            event_time_millis: question_time,
            reply_to_ordinal: None,
            content: Arc::from(case.question.as_str()),
            origin: IngestionOrigin::LongMemEvalQuery,
        },
        scope: MemoryScope::Workspace,
    })
}

fn rank_sessions(
    items: &[phoenix_memory_coordinator::ContextItem],
    source_bindings: &HashMap<SourceId, String>,
    top_k: usize,
) -> Vec<RankedSession> {
    let mut scores = HashMap::<SourceId, u64>::with_capacity(items.len());
    for item in items {
        scores
            .entry(item.source_id)
            .and_modify(|score| *score = (*score).max(u64::from(item.score_micros)))
            .or_insert_with(|| u64::from(item.score_micros));
    }
    let mut ranked = scores
        .into_iter()
        .filter_map(|(source_id, score)| {
            source_bindings
                .get(&source_id)
                .map(|stable_id| (stable_id.clone(), score))
        })
        .collect::<Vec<_>>();
    ranked.sort_unstable_by(|left, right| {
        right
            .1
            .cmp(&left.1)
            .then_with(|| left.0.as_str().cmp(right.0.as_str()))
    });
    ranked.truncate(top_k.min(ranked.len()));
    ranked
        .into_iter()
        .map(|(stable_id, score)| RankedSession {
            stable_id,
            score_bits: (score as f64 / 1_000_000.0).to_bits(),
        })
        .collect()
}

fn participant_role(role: &str) -> ParticipantRole {
    if role.eq_ignore_ascii_case("user") || role.eq_ignore_ascii_case("human") {
        ParticipantRole::User
    } else if role.eq_ignore_ascii_case("assistant") {
        ParticipantRole::Assistant
    } else if role.eq_ignore_ascii_case("system") {
        ParticipantRole::System
    } else if role.eq_ignore_ascii_case("tool") {
        ParticipantRole::Tool
    } else {
        ParticipantRole::Other
    }
}

fn authority_root(output_path: &Path) -> Result<PathBuf> {
    let parent = output_path
        .parent()
        .context("retrieval output path has no parent")?;
    let stem = output_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .context("retrieval output filename is not UTF-8")?;
    Ok(parent.join(format!("{stem}.memory-v3")))
}

fn parse_date_millis(date: &str) -> Result<i64> {
    let date = date.get(..10).context("date must begin with YYYY-MM-DD")?;
    let mut parts = date.split('-');
    let year = parts
        .next()
        .context("missing year")?
        .parse::<i64>()
        .context("invalid year")?;
    let month = parts
        .next()
        .context("missing month")?
        .parse::<u32>()
        .context("invalid month")?;
    let day = parts
        .next()
        .context("missing day")?
        .parse::<u32>()
        .context("invalid day")?;
    if parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        bail!("date is outside the supported civil range");
    }
    civil_days(year, month, day)
        .checked_mul(86_400_000)
        .context("date milliseconds overflow")
}

// Howard Hinnant's proleptic Gregorian civil-date conversion. This keeps the
// adapter deterministic and independent of the machine timezone.
fn civil_days(year: i64, month: u32, day: u32) -> i64 {
    let adjusted_year = year - i64::from(month <= 2);
    let era = adjusted_year.div_euclid(400);
    let year_of_era = adjusted_year - era * 400;
    let shifted_month = i64::from(month) + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[derive(Debug, serde::Serialize)]
pub struct NativeRecallReceipt {
    pub engine: &'static str,
    pub cases: usize,
    pub top_k: usize,
    pub published_generations: u64,
    pub maximum_queue_high_water: u64,
    pub output_path: String,
    pub authority_root: String,
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;
    use crate::artifact::write_artifact;
    use crate::model::{
        GoldArtifact, GoldCase, SourceBinding, WorkloadCase, GOLD_CONTRACT, GOLD_MAGIC,
    };
    use crate::verify;

    #[test]
    fn production_adapter_is_deterministic_and_gold_blind() {
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
        run(&manifest, &workload_path, &first, 2).expect("first production recall");
        run(&manifest, &workload_path, &second, 2).expect("second production recall");
        assert_eq!(read_retrieval(&first).cases, read_retrieval(&second).cases);
        assert_eq!(
            read_retrieval(&first).cases[0].ranked_sessions[0].stable_id,
            "s1"
        );
        let error = run(
            &manifest,
            &gold_path,
            &directory.path().join("forbidden.plmr"),
            2,
        )
        .expect_err("gold cannot enter production recall");
        assert!(error.to_string().contains("artifact type mismatch"));
    }

    #[test]
    fn civil_date_conversion_has_unix_epoch() {
        assert_eq!(parse_date_millis("1970-01-01").expect("epoch"), 0);
        assert_eq!(
            parse_date_millis("1970-01-02T00:00:00Z").expect("next day"),
            86_400_000
        );
    }

    fn read_retrieval(path: &Path) -> RetrievalArtifact {
        read_artifact(path, RETRIEVAL_MAGIC).expect("read retrieval")
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
