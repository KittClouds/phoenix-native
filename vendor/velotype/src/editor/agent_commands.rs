//! Provider-neutral agent operations against the resident document tree.
//!
//! This module deliberately owns no model runtime. A simulator, local model,
//! or remote provider must all submit the same validated operation. The editor
//! remains sovereign: it resolves the anchor, performs one bounded mutation,
//! captures one undo entry, and serializes only ordinary Markdown blocks.

use gpui::{Entity, EntityId, SharedString};
use uuid::Uuid;

use super::{Editor, UndoCaptureKind};
use crate::components::{
    AgentBlockOrigin, AgentBlockState, Block, BlockKind, BlockOrigin, BlockRecord, InlineTextTree,
};

const MAX_AGENT_BLOCKS_PER_OP: usize = 64;
const MAX_AGENT_TEXT_BYTES_PER_OP: usize = 512 * 1024;

/// Exact editor-local insertion point captured before an agent is invoked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AgentAnchor {
    pub editor_revision: u64,
    pub block_id: Uuid,
    pub byte_offset: usize,
}

/// One ordinary native block emitted by an agent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentBlockDraft {
    pub kind: BlockKind,
    pub text: SharedString,
}

impl AgentBlockDraft {
    pub fn paragraph(text: impl Into<SharedString>) -> Self {
        Self {
            kind: BlockKind::Paragraph,
            text: text.into(),
        }
    }
}

/// Provider-neutral operation accepted by the editor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentDocumentOp {
    InsertAfter {
        anchor: AgentAnchor,
        invocation_id: Uuid,
        turn_id: Uuid,
        model: SharedString,
        context_digest: [u8; 32],
        blocks: Vec<AgentBlockDraft>,
    },
    SetDisposition {
        invocation_id: Uuid,
        disposition: AgentInvocationDisposition,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentInvocationDisposition {
    KeepAsResponse,
    ConvertToProse,
    Remove,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentInsertionReceipt {
    pub invocation_id: Uuid,
    pub turn_id: Uuid,
    pub inserted_block_ids: Vec<Uuid>,
    pub editor_revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentInvocationReceipt {
    Inserted(AgentInsertionReceipt),
    DispositionChanged {
        invocation_id: Uuid,
        disposition: AgentInvocationDisposition,
        affected_blocks: usize,
        editor_revision: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentCommandError {
    NoActiveBlock,
    StaleAnchor { expected: u64, actual: u64 },
    AnchorMissing(Uuid),
    InvalidAnchorOffset { offset: usize, block_len: usize },
    EmptyInvocation,
    TooManyBlocks { count: usize, limit: usize },
    PayloadTooLarge { bytes: usize, limit: usize },
    UnsupportedBlockKind(BlockKind),
    EmptyModelIdentity,
    InvocationMissing(Uuid),
}

impl Editor {
    /// Development-only caret entry point used by host shells before a model
    /// runtime exists. The resulting operation is indistinguishable from one
    /// submitted by a future provider at the editor boundary.
    pub fn simulate_agent_response_at_caret(
        &mut self,
        text: impl Into<SharedString>,
        cx: &mut gpui::Context<Self>,
    ) -> Result<AgentInvocationReceipt, AgentCommandError> {
        let anchor = self.current_agent_anchor(cx)?;
        self.execute_agent_document_op(
            AgentDocumentOp::InsertAfter {
                anchor,
                invocation_id: Uuid::new_v4(),
                turn_id: Uuid::new_v4(),
                model: SharedString::new("phoenix-notes-simulator/v1"),
                context_digest: [0xA1; 32],
                blocks: vec![AgentBlockDraft::paragraph(text)],
            },
            cx,
        )
    }

    /// Development-only producer shim. It deliberately enters through
    /// [`execute_agent_document_op`](Self::execute_agent_document_op), proving
    /// the same boundary a real provider must use without embedding a model.
    pub fn simulate_agent_response_after_block(
        &mut self,
        entity_id: EntityId,
        text: impl Into<SharedString>,
        cx: &mut gpui::Context<Self>,
    ) -> Result<AgentInvocationReceipt, AgentCommandError> {
        let block = self
            .document
            .block_entity_by_id(entity_id)
            .ok_or(AgentCommandError::NoActiveBlock)?;
        let anchor = {
            let block = block.read(cx);
            AgentAnchor {
                editor_revision: self.document_revision,
                block_id: block.record.id,
                byte_offset: block.display_text().len(),
            }
        };
        self.execute_agent_document_op(
            AgentDocumentOp::InsertAfter {
                anchor,
                invocation_id: Uuid::new_v4(),
                turn_id: Uuid::new_v4(),
                model: SharedString::new("phoenix-notes-simulator/v1"),
                context_digest: [0xA1; 32],
                blocks: vec![AgentBlockDraft::paragraph(text)],
            },
            cx,
        )
    }

    /// Captures the current caret as an immutable insertion anchor.
    pub fn current_agent_anchor(&self, cx: &gpui::App) -> Result<AgentAnchor, AgentCommandError> {
        let entity_id = self
            .active_entity_id
            .ok_or(AgentCommandError::NoActiveBlock)?;
        let block = self
            .document
            .block_entity_by_id(entity_id)
            .ok_or(AgentCommandError::NoActiveBlock)?;
        let block = block.read(cx);
        Ok(AgentAnchor {
            editor_revision: self.document_revision,
            block_id: block.record.id,
            byte_offset: block.cursor_offset(),
        })
    }

    /// Executes one agent operation as one editor transaction.
    pub fn execute_agent_document_op(
        &mut self,
        op: AgentDocumentOp,
        cx: &mut gpui::Context<Self>,
    ) -> Result<AgentInvocationReceipt, AgentCommandError> {
        match op {
            AgentDocumentOp::InsertAfter {
                anchor,
                invocation_id,
                turn_id,
                model,
                context_digest,
                blocks,
            } => self.insert_agent_blocks(
                anchor,
                invocation_id,
                turn_id,
                model,
                context_digest,
                blocks,
                cx,
            ),
            AgentDocumentOp::SetDisposition {
                invocation_id,
                disposition,
            } => self.set_agent_disposition(invocation_id, disposition, cx),
        }
    }

    fn insert_agent_blocks(
        &mut self,
        anchor: AgentAnchor,
        invocation_id: Uuid,
        turn_id: Uuid,
        model: SharedString,
        context_digest: [u8; 32],
        drafts: Vec<AgentBlockDraft>,
        cx: &mut gpui::Context<Self>,
    ) -> Result<AgentInvocationReceipt, AgentCommandError> {
        if anchor.editor_revision != self.document_revision {
            return Err(AgentCommandError::StaleAnchor {
                expected: anchor.editor_revision,
                actual: self.document_revision,
            });
        }
        if drafts.is_empty() {
            return Err(AgentCommandError::EmptyInvocation);
        }
        if drafts.len() > MAX_AGENT_BLOCKS_PER_OP {
            return Err(AgentCommandError::TooManyBlocks {
                count: drafts.len(),
                limit: MAX_AGENT_BLOCKS_PER_OP,
            });
        }
        if model.trim().is_empty() {
            return Err(AgentCommandError::EmptyModelIdentity);
        }
        let payload_bytes = drafts.iter().map(|draft| draft.text.len()).sum::<usize>();
        if payload_bytes > MAX_AGENT_TEXT_BYTES_PER_OP {
            return Err(AgentCommandError::PayloadTooLarge {
                bytes: payload_bytes,
                limit: MAX_AGENT_TEXT_BYTES_PER_OP,
            });
        }
        for draft in &drafts {
            if !agent_insert_kind_supported(&draft.kind) {
                return Err(AgentCommandError::UnsupportedBlockKind(draft.kind.clone()));
            }
        }

        let anchor_block = self
            .document
            .block_entity_by_uuid(anchor.block_id)
            .ok_or(AgentCommandError::AnchorMissing(anchor.block_id))?;
        let block_len = anchor_block.read(cx).display_text().len();
        if anchor.byte_offset > block_len
            || !anchor_block
                .read(cx)
                .display_text()
                .is_char_boundary(anchor.byte_offset)
        {
            return Err(AgentCommandError::InvalidAnchorOffset {
                offset: anchor.byte_offset,
                block_len,
            });
        }
        let location = self
            .document
            .find_block_location(anchor_block.entity_id())
            .ok_or(AgentCommandError::AnchorMissing(anchor.block_id))?;

        let mut entities: Vec<Entity<Block>> = Vec::with_capacity(drafts.len());
        let mut block_ids = Vec::with_capacity(drafts.len());
        for (ordinal, draft) in drafts.into_iter().enumerate() {
            let mut record = BlockRecord::new(
                draft.kind,
                InlineTextTree::from_markdown(draft.text.as_ref()),
            );
            record.origin = BlockOrigin::Agent(AgentBlockOrigin {
                invocation_id,
                turn_id,
                model: model.clone(),
                context_digest,
                state: AgentBlockState::Provisional,
                group_ordinal: ordinal as u32,
            });
            block_ids.push(record.id);
            entities.push(Self::new_block(cx, record));
        }

        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        self.document
            .insert_blocks_at(location.parent, location.index + 1, entities.clone(), cx);
        if let Some(first) = entities.first() {
            self.focus_block(first.entity_id());
        }
        self.mark_dirty(cx);
        self.finalize_pending_undo_capture(cx);
        self.request_active_block_scroll_into_view(cx);
        cx.notify();

        Ok(AgentInvocationReceipt::Inserted(AgentInsertionReceipt {
            invocation_id,
            turn_id,
            inserted_block_ids: block_ids,
            editor_revision: self.document_revision,
        }))
    }

    fn set_agent_disposition(
        &mut self,
        invocation_id: Uuid,
        disposition: AgentInvocationDisposition,
        cx: &mut gpui::Context<Self>,
    ) -> Result<AgentInvocationReceipt, AgentCommandError> {
        let targets: Vec<EntityId> = self
            .document
            .visible_blocks()
            .iter()
            .filter_map(|visible| {
                let block = visible.entity.read(cx);
                matches!(
                    &block.record.origin,
                    BlockOrigin::Agent(origin) if origin.invocation_id == invocation_id
                )
                .then_some(visible.entity.entity_id())
            })
            .collect();
        if targets.is_empty() {
            return Err(AgentCommandError::InvocationMissing(invocation_id));
        }

        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        if disposition == AgentInvocationDisposition::Remove {
            self.document.with_structure_mutation(cx, |document, cx| {
                for entity_id in targets.iter().rev().copied() {
                    let _ = document.remove_block_by_id_raw(entity_id, cx);
                }
            });
            if self
                .active_entity_id
                .is_some_and(|entity_id| targets.contains(&entity_id))
            {
                self.pending_focus = self.first_focusable_entity_id(cx);
                self.active_entity_id = self.pending_focus;
            }
        } else {
            let next_state = match disposition {
                AgentInvocationDisposition::KeepAsResponse => AgentBlockState::Committed,
                AgentInvocationDisposition::ConvertToProse => AgentBlockState::Adopted,
                AgentInvocationDisposition::Remove => unreachable!(),
            };
            for entity_id in &targets {
                if let Some(block) = self.document.block_entity_by_id(*entity_id) {
                    block.update(cx, |block, cx| {
                        if let BlockOrigin::Agent(origin) = &mut block.record.origin {
                            origin.state = next_state;
                        }
                        cx.notify();
                    });
                }
            }
        }
        self.mark_dirty(cx);
        self.finalize_pending_undo_capture(cx);
        cx.notify();

        Ok(AgentInvocationReceipt::DispositionChanged {
            invocation_id,
            disposition,
            affected_blocks: targets.len(),
            editor_revision: self.document_revision,
        })
    }
}

fn agent_insert_kind_supported(kind: &BlockKind) -> bool {
    matches!(
        kind,
        BlockKind::Paragraph
            | BlockKind::Heading { level: 1..=6 }
            | BlockKind::BulletedListItem
            | BlockKind::NumberedListItem
            | BlockKind::TaskListItem { .. }
            | BlockKind::Quote
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::{AgentBlockState, BlockOrigin};
    use gpui::{AppContext, TestAppContext};

    fn init_editor_test_app(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
    }

    fn insert_two_block_response(
        editor: &mut Editor,
        invocation_id: Uuid,
        cx: &mut gpui::Context<Editor>,
    ) -> AgentInvocationReceipt {
        let anchor = {
            let anchor_block = editor.document.visible_blocks()[0].entity.read(cx);
            AgentAnchor {
                editor_revision: editor.document_revision,
                block_id: anchor_block.record.id,
                byte_offset: anchor_block.display_text().len(),
            }
        };
        editor
            .execute_agent_document_op(
                AgentDocumentOp::InsertAfter {
                    anchor,
                    invocation_id,
                    turn_id: Uuid::from_u128(22),
                    model: SharedString::new("notes-simulator-test/v1"),
                    context_digest: [7; 32],
                    blocks: vec![
                        AgentBlockDraft::paragraph("First proposal."),
                        AgentBlockDraft {
                            kind: BlockKind::Heading { level: 2 },
                            text: SharedString::new("Second proposal"),
                        },
                    ],
                },
                cx,
            )
            .expect("valid simulated response")
    }

    #[gpui::test]
    fn insertion_is_one_transaction_with_clean_markdown_and_stable_origin(cx: &mut TestAppContext) {
        init_editor_test_app(cx);
        let editor = cx.new(|cx| Editor::from_markdown(cx, "Alpha.\n\nOmega.".to_string(), None));
        let invocation_id = Uuid::from_u128(11);

        editor.update(cx, |editor, cx| {
            let receipt = insert_two_block_response(editor, invocation_id, cx);
            let AgentInvocationReceipt::Inserted(receipt) = receipt else {
                panic!("expected insertion receipt");
            };
            assert_eq!(receipt.inserted_block_ids.len(), 2);
            assert_eq!(editor.undo_history.len(), 1);
            assert_eq!(
                editor.current_document_source(cx),
                "Alpha.\n\nFirst proposal.\n\n## Second proposal\n\nOmega."
            );
            let inserted = &editor.document.visible_blocks()[1..=2];
            for (ordinal, visible) in inserted.iter().enumerate() {
                let block = visible.entity.read(cx);
                let BlockOrigin::Agent(origin) = &block.record.origin else {
                    panic!("inserted block lost agent origin");
                };
                assert_eq!(origin.invocation_id, invocation_id);
                assert_eq!(origin.state, AgentBlockState::Provisional);
                assert_eq!(origin.group_ordinal, ordinal as u32);
            }
        });

        editor.update(cx, |editor, cx| editor.undo_document(cx));
        editor.update(cx, |editor, cx| {
            assert_eq!(editor.current_document_source(cx), "Alpha.\n\nOmega.");
            assert_eq!(editor.document.visible_blocks().len(), 2);
        });
        editor.update(cx, |editor, cx| editor.redo_document(cx));
        editor.update(cx, |editor, cx| {
            assert_eq!(editor.document.visible_blocks().len(), 4);
            let block = editor.document.visible_blocks()[1].entity.read(cx);
            assert!(matches!(
                &block.record.origin,
                BlockOrigin::Agent(origin)
                    if origin.invocation_id == invocation_id
                        && origin.state == AgentBlockState::Provisional
            ));
        });
    }

    #[gpui::test]
    fn stale_anchor_fails_without_mutating_document(cx: &mut TestAppContext) {
        init_editor_test_app(cx);
        let editor = cx.new(|cx| Editor::from_markdown(cx, "Alpha.".to_string(), None));
        editor.update(cx, |editor, cx| {
            let anchor = {
                let block = editor.document.visible_blocks()[0].entity.read(cx);
                AgentAnchor {
                    editor_revision: editor.document_revision + 1,
                    block_id: block.record.id,
                    byte_offset: block.display_text().len(),
                }
            };
            let result = editor.execute_agent_document_op(
                AgentDocumentOp::InsertAfter {
                    anchor,
                    invocation_id: Uuid::from_u128(11),
                    turn_id: Uuid::from_u128(22),
                    model: SharedString::new("notes-simulator-test/v1"),
                    context_digest: [7; 32],
                    blocks: vec![AgentBlockDraft::paragraph("Nope")],
                },
                cx,
            );
            assert!(matches!(result, Err(AgentCommandError::StaleAnchor { .. })));
            assert_eq!(editor.current_document_source(cx), "Alpha.");
            assert!(editor.undo_history.is_empty());
        });
    }

    #[gpui::test]
    fn dispositions_are_undoable_and_remove_the_whole_invocation(cx: &mut TestAppContext) {
        init_editor_test_app(cx);
        let editor = cx.new(|cx| Editor::from_markdown(cx, "Alpha.\n\nOmega.".to_string(), None));
        let invocation_id = Uuid::from_u128(11);
        editor.update(cx, |editor, cx| {
            insert_two_block_response(editor, invocation_id, cx);
            editor
                .execute_agent_document_op(
                    AgentDocumentOp::SetDisposition {
                        invocation_id,
                        disposition: AgentInvocationDisposition::ConvertToProse,
                    },
                    cx,
                )
                .expect("adopt response");
            for visible in &editor.document.visible_blocks()[1..=2] {
                let block = visible.entity.read(cx);
                assert!(matches!(
                    &block.record.origin,
                    BlockOrigin::Agent(origin) if origin.state == AgentBlockState::Adopted
                ));
            }
        });
        editor.update(cx, |editor, cx| editor.undo_document(cx));
        editor.update(cx, |editor, cx| {
            let block = editor.document.visible_blocks()[1].entity.read(cx);
            assert!(matches!(
                &block.record.origin,
                BlockOrigin::Agent(origin) if origin.state == AgentBlockState::Provisional
            ));
            editor
                .execute_agent_document_op(
                    AgentDocumentOp::SetDisposition {
                        invocation_id,
                        disposition: AgentInvocationDisposition::Remove,
                    },
                    cx,
                )
                .expect("remove response");
            assert_eq!(editor.current_document_source(cx), "Alpha.\n\nOmega.");
            assert_eq!(editor.document.visible_blocks().len(), 2);
        });
    }
}
