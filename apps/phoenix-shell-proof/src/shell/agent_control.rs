use super::PhoenixShell;
use anyhow::{Context as _, Result};
use gpui::Context;
use hashbrown::HashMap;
use phoenix_agent_control::{
    AgentControlRequestV1, AgentControlResponseV1, AgentControlStatusV1, HostRequest, PhxCommandV1,
    AGENT_CONTROL_SCHEMA_V1,
};
use phoenix_app_core::{KernelCommand, KernelSnapshot};
use phoenix_workspace::{open_document, DocumentLease, EntryId, EntryKind};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;
use velotype::{AgentBlockDraft, AgentDocumentOp, AgentInvocationReceipt};

const RECEIPT_SUFFIX: &str = ".agent-receipts-v1.jsonl";
const MAX_RECEIPT_LOG_BYTES: u64 = 64 * 1024 * 1024;
const DEFAULT_NOTE_READ_BYTES: usize = 64 * 1024;
const MAX_NOTE_READ_BYTES: usize = 512 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct DurableAgentReceipt {
    idempotency_key: String,
    canonical_command: String,
    response: AgentControlResponseV1,
}

pub(super) struct AgentReceiptJournal {
    path: PathBuf,
    receipts: HashMap<String, DurableAgentReceipt>,
}

impl AgentReceiptJournal {
    pub(super) fn open(workspace_path: &Path) -> Result<Self> {
        let mut path = workspace_path.as_os_str().to_owned();
        path.push(RECEIPT_SUFFIX);
        let path = PathBuf::from(path);
        let mut receipts = HashMap::new();
        if path.exists() {
            let bytes = fs::metadata(&path)?.len();
            anyhow::ensure!(
                bytes <= MAX_RECEIPT_LOG_BYTES,
                "agent receipt journal exceeds its bounded load budget"
            );
            let reader =
                BufReader::new(fs::File::open(&path).with_context(|| {
                    format!("open agent receipt journal at {}", path.display())
                })?);
            for (line_number, line) in reader.lines().enumerate() {
                let line = line?;
                if line.trim().is_empty() {
                    continue;
                }
                let receipt: DurableAgentReceipt =
                    serde_json::from_str(&line).with_context(|| {
                        format!(
                            "decode agent receipt journal {} line {}",
                            path.display(),
                            line_number + 1
                        )
                    })?;
                receipts.insert(receipt.idempotency_key.clone(), receipt);
            }
        }
        Ok(Self { path, receipts })
    }

    fn replay(
        &self,
        request: &AgentControlRequestV1,
        key: &str,
        canonical_command: &str,
    ) -> Result<Option<AgentControlResponseV1>, String> {
        let Some(receipt) = self.receipts.get(key) else {
            return Ok(None);
        };
        if receipt.canonical_command != canonical_command {
            return Err("idempotency key is already bound to a different command".into());
        }
        let mut response = receipt.response.clone();
        response.command_id.clone_from(&request.command_id);
        response.replayed = true;
        Ok(Some(response))
    }

    fn record(
        &mut self,
        key: String,
        canonical_command: String,
        response: AgentControlResponseV1,
    ) -> Result<()> {
        let receipt = DurableAgentReceipt {
            idempotency_key: key.clone(),
            canonical_command,
            response,
        };
        let parent = self
            .path
            .parent()
            .context("agent receipt path has no parent")?;
        fs::create_dir_all(parent)?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .with_context(|| format!("append agent receipt at {}", self.path.display()))?;
        serde_json::to_writer(&mut file, &receipt)?;
        file.write_all(b"\n")?;
        file.sync_data()?;
        self.receipts.insert(key, receipt);
        Ok(())
    }
}

impl PhoenixShell {
    pub(super) fn start_agent_control_loop(
        &mut self,
        receiver: async_channel::Receiver<HostRequest>,
        cx: &mut Context<Self>,
    ) {
        self.agent_control_task = Some(cx.spawn(async move |shell, async_cx| {
            while let Ok(host_request) = receiver.recv().await {
                if shell
                    .update(async_cx, |shell, cx| {
                        shell.apply_agent_control_request(host_request, cx);
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
    }

    fn apply_agent_control_request(&mut self, host_request: HostRequest, cx: &mut Context<Self>) {
        let HostRequest {
            request,
            command,
            reply,
        } = host_request;
        let response = self.execute_agent_control_request(&request, command, cx);
        let _ = reply.try_send(response);
    }

    fn execute_agent_control_request(
        &mut self,
        request: &AgentControlRequestV1,
        command: PhxCommandV1,
        cx: &mut Context<Self>,
    ) -> AgentControlResponseV1 {
        let canonical = command.canonical();
        let snapshot = match self.kernel.snapshot() {
            Ok(snapshot) => snapshot,
            Err(error) => {
                return AgentControlResponseV1::error(request, error.to_string());
            }
        };
        match command {
            PhxCommandV1::AppStatus => self.agent_app_status(request, &canonical, &snapshot),
            PhxCommandV1::WorkspaceList => {
                self.agent_workspace_list(request, &canonical, &snapshot)
            }
            PhxCommandV1::NoteStat { entry_id } => {
                self.agent_note_stat(request, &canonical, &snapshot, entry_id)
            }
            PhxCommandV1::NoteRead { entry_id, from, to } => {
                self.agent_note_read(request, &canonical, &snapshot, entry_id, from, to)
            }
            PhxCommandV1::BlockList {
                entry_id,
                from,
                limit,
            } => self.agent_block_list(request, &canonical, &snapshot, entry_id, from, limit, cx),
            PhxCommandV1::BlockInsert {
                entry_id,
                after,
                text,
                expected_document_revision,
                idempotency_key,
            } => self.agent_block_insert(
                request,
                &canonical,
                &snapshot,
                entry_id,
                after,
                text,
                expected_document_revision,
                idempotency_key,
                cx,
            ),
            PhxCommandV1::EventsAfter { sequence } => {
                self.agent_events_after(request, &canonical, &snapshot, sequence)
            }
        }
    }

    fn agent_app_status(
        &self,
        request: &AgentControlRequestV1,
        canonical: &str,
        snapshot: &KernelSnapshot,
    ) -> AgentControlResponseV1 {
        let metrics = self.kernel.metrics();
        self.agent_response(
            request,
            canonical,
            AgentControlStatusV1::Ok,
            snapshot,
            metrics.last_sequence,
            json!({
                "app": "phoenix-native",
                "process_id": std::process::id(),
                "active_entry": snapshot.active_entry.0,
                "active_document": snapshot.active_document.map(|document| document.0),
                "shutting_down": snapshot.shutting_down,
                "kernel_metrics": {
                    "commands_submitted": metrics.commands_submitted,
                    "commands_completed": metrics.commands_completed,
                    "commands_rejected": metrics.commands_rejected,
                    "commands_pending": metrics.commands_pending,
                    "command_queue_high_water": metrics.command_queue_high_water,
                    "events_published": metrics.events_published,
                    "events_evicted": metrics.events_evicted,
                    "events_pending": metrics.events_pending,
                    "event_queue_high_water": metrics.event_queue_high_water,
                    "worker_exited": metrics.worker_exited,
                }
            }),
            None,
        )
    }

    fn agent_workspace_list(
        &self,
        request: &AgentControlRequestV1,
        canonical: &str,
        snapshot: &KernelSnapshot,
    ) -> AgentControlResponseV1 {
        let entries = snapshot
            .workspace
            .entries()
            .iter()
            .map(|entry| {
                json!({
                    "id": entry.id.0,
                    "parent": entry.parent.map(|parent| parent.0),
                    "kind": entry.kind.label(),
                    "name": entry.name,
                    "path": snapshot.workspace.path_for(entry.id).unwrap_or_default(),
                })
            })
            .collect::<Vec<_>>();
        self.agent_response(
            request,
            canonical,
            AgentControlStatusV1::Ok,
            snapshot,
            self.kernel.metrics().last_sequence,
            json!({ "entries": entries }),
            None,
        )
    }

    fn agent_note_stat(
        &self,
        request: &AgentControlRequestV1,
        canonical: &str,
        snapshot: &KernelSnapshot,
        entry_id: Option<u64>,
    ) -> AgentControlResponseV1 {
        match self.agent_document_lease(snapshot, entry_id) {
            Ok(lease) => {
                let path = snapshot
                    .workspace
                    .path_for(lease.entry_id)
                    .unwrap_or_default();
                let mut response = self.agent_response(
                    request,
                    canonical,
                    AgentControlStatusV1::Ok,
                    snapshot,
                    self.kernel.metrics().last_sequence,
                    json!({
                        "entry_id": lease.entry_id.0,
                        "path": path,
                        "bytes": lease.content.len(),
                        "content_blake3": lease.content_hash.to_hex(),
                    }),
                    None,
                );
                response.document_revision = Some(lease.revision.0);
                response
            }
            Err(error) => self.agent_failure(
                request,
                canonical,
                AgentControlStatusV1::Error,
                snapshot,
                error,
            ),
        }
    }

    fn agent_note_read(
        &self,
        request: &AgentControlRequestV1,
        canonical: &str,
        snapshot: &KernelSnapshot,
        entry_id: Option<u64>,
        from: usize,
        to: Option<usize>,
    ) -> AgentControlResponseV1 {
        let lease = match self.agent_document_lease(snapshot, entry_id) {
            Ok(lease) => lease,
            Err(error) => {
                return self.agent_failure(
                    request,
                    canonical,
                    AgentControlStatusV1::Error,
                    snapshot,
                    error,
                );
            }
        };
        let end = to.unwrap_or_else(|| {
            from.saturating_add(DEFAULT_NOTE_READ_BYTES)
                .min(lease.content.len())
        });
        if from > end
            || end > lease.content.len()
            || end.saturating_sub(from) > MAX_NOTE_READ_BYTES
            || !lease.content.is_char_boundary(from)
            || !lease.content.is_char_boundary(end)
        {
            return self.agent_failure(
                request,
                canonical,
                AgentControlStatusV1::Conflict,
                snapshot,
                "requested byte range is outside the note, exceeds 512 KiB, or splits UTF-8".into(),
            );
        }
        let mut response = self.agent_response(
            request,
            canonical,
            AgentControlStatusV1::Ok,
            snapshot,
            self.kernel.metrics().last_sequence,
            json!({
                "entry_id": lease.entry_id.0,
                "from": from,
                "to": end,
                "total_bytes": lease.content.len(),
                "next_from": (end < lease.content.len()).then_some(end),
                "content": &lease.content[from..end],
            }),
            None,
        );
        response.document_revision = Some(lease.revision.0);
        response
    }

    fn agent_block_list(
        &self,
        request: &AgentControlRequestV1,
        canonical: &str,
        snapshot: &KernelSnapshot,
        entry_id: Option<u64>,
        from: usize,
        limit: usize,
        cx: &mut Context<Self>,
    ) -> AgentControlResponseV1 {
        let selected = entry_id.unwrap_or(snapshot.active_entry.0);
        if selected != snapshot.active_entry.0 {
            return self.agent_failure(
                request,
                canonical,
                AgentControlStatusV1::Conflict,
                snapshot,
                "block commands currently require the active note".into(),
            );
        }
        let document = self
            .editor
            .read_with(cx, |editor, cx| editor.agent_document_snapshot(cx));
        if from > document.blocks.len() {
            return self.agent_failure(
                request,
                canonical,
                AgentControlStatusV1::Conflict,
                snapshot,
                "block page starts beyond the document".into(),
            );
        }
        let end = from.saturating_add(limit).min(document.blocks.len());
        let blocks = document
            .blocks
            .get(from..end)
            .unwrap_or_default()
            .iter()
            .map(|block| {
                json!({
                    "block_id": block.block_id,
                    "kind": format!("{:?}", block.kind),
                    "text": block.text,
                })
            })
            .collect::<Vec<_>>();
        self.agent_response(
            request,
            canonical,
            AgentControlStatusV1::Ok,
            snapshot,
            self.kernel.metrics().last_sequence,
            json!({
                "entry_id": selected,
                "editor_revision": document.editor_revision,
                "from": from,
                "to": end,
                "total_blocks": document.blocks.len(),
                "next_from": (end < document.blocks.len()).then_some(end),
                "blocks": blocks,
            }),
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn agent_block_insert(
        &mut self,
        request: &AgentControlRequestV1,
        canonical: &str,
        snapshot: &KernelSnapshot,
        entry_id: Option<u64>,
        after: Option<Uuid>,
        text: String,
        expected_document_revision: u64,
        idempotency_key: String,
        cx: &mut Context<Self>,
    ) -> AgentControlResponseV1 {
        match self
            .agent_receipts
            .replay(request, &idempotency_key, canonical)
        {
            Ok(Some(response)) => return response,
            Ok(None) => {}
            Err(error) => {
                return self.agent_failure(
                    request,
                    canonical,
                    AgentControlStatusV1::Conflict,
                    snapshot,
                    error,
                );
            }
        }
        let selected = entry_id.unwrap_or(snapshot.active_entry.0);
        if selected != snapshot.active_entry.0 {
            return self.agent_failure(
                request,
                canonical,
                AgentControlStatusV1::Conflict,
                snapshot,
                "block mutation currently requires the active note".into(),
            );
        }
        let Some(lease) = self.editor_lease.as_ref().map(Arc::clone) else {
            return self.agent_failure(
                request,
                canonical,
                AgentControlStatusV1::Conflict,
                snapshot,
                "active note has no document lease".into(),
            );
        };
        if lease.revision.0 != expected_document_revision {
            return self.agent_failure(
                request,
                canonical,
                AgentControlStatusV1::Conflict,
                snapshot,
                format!(
                    "stale document revision: expected {expected_document_revision}, current {}",
                    lease.revision.0
                ),
            );
        }
        let editor_snapshot = self
            .editor
            .read_with(cx, |editor, cx| editor.agent_document_snapshot(cx));
        let Some(anchor_block) =
            after.or_else(|| editor_snapshot.blocks.last().map(|block| block.block_id))
        else {
            return self.agent_failure(
                request,
                canonical,
                AgentControlStatusV1::Conflict,
                snapshot,
                "active note has no insertion anchor".into(),
            );
        };
        let anchor = match self.editor.read_with(cx, |editor, cx| {
            editor.agent_anchor_after_block(anchor_block, cx)
        }) {
            Ok(anchor) => anchor,
            Err(error) => {
                return self.agent_failure(
                    request,
                    canonical,
                    AgentControlStatusV1::Conflict,
                    snapshot,
                    format!("invalid insertion anchor: {error:?}"),
                );
            }
        };
        let original_markdown = self
            .editor
            .read_with(cx, |editor, cx| editor.host_document_text(cx));
        let invocation = self.editor.update(cx, |editor, cx| {
            editor.execute_agent_document_op(
                AgentDocumentOp::InsertAfter {
                    anchor,
                    invocation_id: Uuid::new_v4(),
                    turn_id: Uuid::new_v4(),
                    model: "phoenixctl/v1".into(),
                    context_digest: *blake3::hash(canonical.as_bytes()).as_bytes(),
                    blocks: vec![AgentBlockDraft::paragraph(text)],
                },
                cx,
            )
        });
        let inserted = match invocation {
            Ok(AgentInvocationReceipt::Inserted(receipt)) => receipt,
            Ok(_) => unreachable!("insert operation returned a disposition receipt"),
            Err(error) => {
                return self.agent_failure(
                    request,
                    canonical,
                    AgentControlStatusV1::Conflict,
                    snapshot,
                    format!("editor rejected block insertion: {error:?}"),
                );
            }
        };
        let content: Arc<str> = Arc::from(
            self.editor
                .read_with(cx, |editor, cx| editor.host_document_text(cx)),
        );
        let commit = match self.kernel.execute(KernelCommand::SaveDocument {
            lease: lease.token(),
            content,
        }) {
            Ok(receipt) => receipt,
            Err(error) => {
                self.editor.update(cx, |editor, cx| {
                    editor.replace_embedded_document(original_markdown, cx);
                });
                if let Ok(current) = self.kernel.snapshot() {
                    self.editor_lease = current.active_document_lease;
                }
                return self.agent_failure(
                    request,
                    canonical,
                    AgentControlStatusV1::Conflict,
                    snapshot,
                    format!("document commit failed and editor was restored: {error}"),
                );
            }
        };
        let committed_snapshot = match self.kernel.snapshot() {
            Ok(snapshot) => snapshot,
            Err(error) => {
                return AgentControlResponseV1::error(
                    request,
                    format!("document committed but snapshot refresh failed: {error}"),
                );
            }
        };
        self.editor_lease = committed_snapshot.active_document_lease.clone();
        self.editor
            .update(cx, |editor, cx| editor.mark_embedded_saved(cx));
        self.initialize_highlights(cx);
        self.status = format!(
            "AGENT INSERT SAVED / DOCUMENT REVISION {} / SEQUENCE {}",
            self.editor_lease
                .as_ref()
                .map_or(0, |lease| lease.revision.0),
            commit.sequence
        )
        .into();
        cx.notify();
        let mut response = self.agent_response(
            request,
            canonical,
            AgentControlStatusV1::Ok,
            &committed_snapshot,
            commit.sequence,
            json!({
                "entry_id": selected,
                "inserted_block_ids": inserted.inserted_block_ids,
                "editor_revision": inserted.editor_revision,
                "idempotency_key": idempotency_key,
            }),
            None,
        );
        if let Err(error) =
            self.agent_receipts
                .record(idempotency_key, canonical.to_owned(), response.clone())
        {
            response.status = AgentControlStatusV1::Error;
            response.error = Some(format!(
                "mutation committed but durable idempotency receipt failed: {error:#}; inspect before retry"
            ));
        }
        response
    }

    fn agent_events_after(
        &self,
        request: &AgentControlRequestV1,
        canonical: &str,
        snapshot: &KernelSnapshot,
        sequence: u64,
    ) -> AgentControlResponseV1 {
        let events = match self.kernel.events_after(sequence) {
            Ok(events) => events,
            Err(error) => {
                return self.agent_failure(
                    request,
                    canonical,
                    AgentControlStatusV1::Error,
                    snapshot,
                    error.to_string(),
                );
            }
        };
        let metrics = self.kernel.metrics();
        let oldest_available = events.first().map(|event| event.sequence);
        let gap = oldest_available.is_some_and(|oldest| oldest > sequence.saturating_add(1));
        let values = events
            .iter()
            .map(|event| {
                json!({
                    "sequence": event.sequence,
                    "kernel_revision": event.kernel_revision,
                    "kind": format!("{:?}", event.kind),
                })
            })
            .collect::<Vec<_>>();
        self.agent_response(
            request,
            canonical,
            AgentControlStatusV1::Ok,
            snapshot,
            metrics.last_sequence,
            json!({
                "requested_after": sequence,
                "oldest_available": oldest_available,
                "gap": gap,
                "events_evicted": metrics.events_evicted,
                "events": values,
            }),
            None,
        )
    }

    fn agent_document_lease(
        &self,
        snapshot: &KernelSnapshot,
        entry_id: Option<u64>,
    ) -> Result<DocumentLease, String> {
        let entry_id = EntryId(entry_id.unwrap_or(snapshot.active_entry.0));
        let entry = snapshot
            .workspace
            .entry(entry_id)
            .ok_or_else(|| format!("workspace entry {} does not exist", entry_id.0))?;
        if entry.kind != EntryKind::Note {
            return Err(format!("workspace entry {} is not a note", entry_id.0));
        }
        if let Some(lease) = snapshot
            .active_document_lease
            .as_ref()
            .filter(|lease| lease.entry_id == entry_id)
        {
            return Ok((**lease).clone());
        }
        open_document(self.kernel.workspace_path(), &snapshot.workspace, entry_id)
            .map_err(|error| error.to_string())
    }

    #[allow(clippy::too_many_arguments)]
    fn agent_response(
        &self,
        request: &AgentControlRequestV1,
        canonical: &str,
        status: AgentControlStatusV1,
        snapshot: &KernelSnapshot,
        sequence: u64,
        payload: Value,
        error: Option<String>,
    ) -> AgentControlResponseV1 {
        AgentControlResponseV1 {
            schema: AGENT_CONTROL_SCHEMA_V1.to_string(),
            command_id: request.command_id.clone(),
            canonical_command: canonical.to_owned(),
            status,
            sequence,
            kernel_revision: snapshot.revision,
            workspace_revision: snapshot.workspace.revision(),
            document_revision: snapshot
                .active_document_lease
                .as_ref()
                .map(|lease| lease.revision.0),
            replayed: false,
            payload,
            error,
        }
    }

    fn agent_failure(
        &self,
        request: &AgentControlRequestV1,
        canonical: &str,
        status: AgentControlStatusV1,
        snapshot: &KernelSnapshot,
        error: String,
    ) -> AgentControlResponseV1 {
        self.agent_response(
            request,
            canonical,
            status,
            snapshot,
            self.kernel.metrics().last_sequence,
            Value::Null,
            Some(error),
        )
    }
}
