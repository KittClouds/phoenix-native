//! Typed, editor-owned block mutations.
//!
//! Toolbars, context menus, and keyboard shortcuts all enter through this
//! module. Commands operate on resident block records; supported native blocks
//! are never synthesized as Markdown and reparsed.

use gpui::{Entity, EntityId, SharedString};

use super::{Editor, TableData, UndoCaptureKind};
use crate::components::{Block, BlockKind, BlockRecord, CalloutVariant, InlineTextTree};

/// A semantic conversion of one existing native block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockTransform {
    Paragraph,
    Heading(u8),
    BulletedList,
    NumberedList,
    TaskList,
    Quote,
    Callout(CalloutVariant),
}

/// A native block to create at a structural insertion point.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockInsert {
    Task,
    Quote,
    Callout(CalloutVariant),
    Separator,
    CodeBlock { language: Option<SharedString> },
    Footnote { id: SharedString },
    Table { body_rows: usize, columns: usize },
}

/// Stable location resolved immediately before a command mutates the tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockInsertTarget {
    After(EntityId),
    Append,
}

/// One editor mutation and therefore one undo-history operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockCommand {
    Transform {
        target: EntityId,
        transform: BlockTransform,
    },
    Insert {
        target: BlockInsertTarget,
        insert: BlockInsert,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockCommandOutcome {
    Changed { block: EntityId, focused: EntityId },
    Unchanged { block: EntityId },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockCommandError {
    TargetMissing(EntityId),
    UnsupportedTransform(BlockKind),
    InvalidHeadingLevel(u8),
    InvalidTableDimensions { body_rows: usize, columns: usize },
    EmptyFootnoteId,
}

struct InsertPlan {
    parent: Option<Entity<Block>>,
    index: usize,
    primary: Entity<Block>,
    focus: Entity<Block>,
    add_trailing_paragraph: bool,
    rebuild_tables: bool,
    rebuild_footnotes: bool,
}

impl Editor {
    /// Executes one validated native block command.
    ///
    /// Validation occurs before undo capture. A successful change captures one
    /// non-coalescing undo entry, performs a bounded tree mutation, restores a
    /// deterministic focus target, marks the document dirty, and refreshes the
    /// stable serialized snapshot.
    pub fn execute_block_command(
        &mut self,
        command: BlockCommand,
        cx: &mut gpui::Context<Self>,
    ) -> Result<BlockCommandOutcome, BlockCommandError> {
        match command {
            BlockCommand::Transform { target, transform } => {
                self.execute_block_transform(target, transform, cx)
            }
            BlockCommand::Insert { target, insert } => {
                self.execute_block_insert(target, insert, cx)
            }
        }
    }

    fn execute_block_transform(
        &mut self,
        target: EntityId,
        transform: BlockTransform,
        cx: &mut gpui::Context<Self>,
    ) -> Result<BlockCommandOutcome, BlockCommandError> {
        let block = self
            .document
            .block_entity_by_id(target)
            .ok_or(BlockCommandError::TargetMissing(target))?;
        let current = block.read(cx).kind();
        let next = validated_transform_kind(&current, transform)?;
        if current == next {
            self.focus_block(target);
            return Ok(BlockCommandOutcome::Unchanged { block: target });
        }

        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        self.document.with_structure_mutation(cx, |_document, cx| {
            block.update(cx, |block, cx| {
                block.record.kind = next;
                block.record.raw_fallback = None;
                block.sync_edit_mode_from_kind();
                block.sync_render_cache();
                cx.notify();
            });
        });
        self.focus_block(target);
        self.mark_dirty(cx);
        self.finalize_pending_undo_capture(cx);
        self.request_active_block_scroll_into_view(cx);
        cx.notify();

        Ok(BlockCommandOutcome::Changed {
            block: target,
            focused: target,
        })
    }

    fn execute_block_insert(
        &mut self,
        target: BlockInsertTarget,
        insert: BlockInsert,
        cx: &mut gpui::Context<Self>,
    ) -> Result<BlockCommandOutcome, BlockCommandError> {
        validate_insert(&insert)?;
        let (parent, index) = self.resolve_insert_target(target)?;
        let at_end = parent
            .as_ref()
            .map_or(self.document.root_count(), |parent| {
                parent.read(cx).children.len()
            })
            == index;
        let mut plan = self.build_insert_plan(parent, index, insert, at_end, cx);
        let primary_id = plan.primary.entity_id();

        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        let mut inserted = Vec::with_capacity(1 + usize::from(plan.add_trailing_paragraph));
        inserted.push(plan.primary.clone());
        if plan.add_trailing_paragraph {
            inserted.push(Self::new_block(cx, BlockRecord::paragraph(String::new())));
        }
        self.document
            .insert_blocks_at(plan.parent.take(), plan.index, inserted, cx);

        if plan.rebuild_tables {
            self.rebuild_table_runtimes(cx);
            if let Some(first_cell) = plan
                .primary
                .read(cx)
                .table_runtime
                .as_ref()
                .and_then(|runtime| runtime.header.first())
            {
                plan.focus = first_cell.clone();
            }
        }
        if plan.rebuild_footnotes {
            self.rebuild_footnote_registry(cx);
        }

        let focus_id = plan.focus.entity_id();
        self.focus_block(focus_id);
        self.mark_dirty(cx);
        self.finalize_pending_undo_capture(cx);
        self.request_active_block_scroll_into_view(cx);
        cx.notify();

        Ok(BlockCommandOutcome::Changed {
            block: primary_id,
            focused: focus_id,
        })
    }

    fn resolve_insert_target(
        &self,
        target: BlockInsertTarget,
    ) -> Result<(Option<Entity<Block>>, usize), BlockCommandError> {
        match target {
            BlockInsertTarget::After(entity_id) => {
                let location = self
                    .document
                    .find_block_location(entity_id)
                    .ok_or(BlockCommandError::TargetMissing(entity_id))?;
                Ok((location.parent, location.index + 1))
            }
            BlockInsertTarget::Append => Ok((None, self.document.root_count())),
        }
    }

    fn build_insert_plan(
        &mut self,
        parent: Option<Entity<Block>>,
        index: usize,
        insert: BlockInsert,
        at_end: bool,
        cx: &mut gpui::Context<Self>,
    ) -> InsertPlan {
        let (primary, focus, rebuild_tables, rebuild_footnotes) = match insert {
            BlockInsert::Task => {
                let block = Self::new_block(
                    cx,
                    BlockRecord::new(
                        BlockKind::TaskListItem { checked: false },
                        InlineTextTree::plain(String::new()),
                    ),
                );
                (block.clone(), block, false, false)
            }
            BlockInsert::Quote => {
                let block = Self::new_block(
                    cx,
                    BlockRecord::new(BlockKind::Quote, InlineTextTree::plain(String::new())),
                );
                (block.clone(), block, false, false)
            }
            BlockInsert::Callout(variant) => {
                let body = Self::new_block(cx, BlockRecord::paragraph(String::new()));
                let block = Self::new_block(
                    cx,
                    BlockRecord::new(
                        BlockKind::Callout(variant),
                        InlineTextTree::plain(String::new()),
                    ),
                );
                block.update(cx, {
                    let body = body.clone();
                    move |block, _cx| block.children.push(body.clone())
                });
                (block, body, false, false)
            }
            BlockInsert::Separator => {
                let block = Self::new_block(
                    cx,
                    BlockRecord::new(BlockKind::Separator, InlineTextTree::plain(String::new())),
                );
                (block.clone(), block, false, false)
            }
            BlockInsert::CodeBlock { language } => {
                let block = Self::new_block(
                    cx,
                    BlockRecord::new(
                        BlockKind::CodeBlock { language },
                        InlineTextTree::plain(String::new()),
                    ),
                );
                (block.clone(), block, false, false)
            }
            BlockInsert::Footnote { id } => {
                let body = Self::new_block(cx, BlockRecord::paragraph(String::new()));
                let block = Self::new_block(
                    cx,
                    BlockRecord::new(
                        BlockKind::FootnoteDefinition,
                        InlineTextTree::plain(id.to_string()),
                    ),
                );
                block.update(cx, {
                    let body = body.clone();
                    move |block, _cx| block.children.push(body.clone())
                });
                (block, body, false, true)
            }
            BlockInsert::Table { body_rows, columns } => {
                let block = Self::new_table_block(cx, TableData::new_empty(body_rows, columns));
                (block.clone(), block, true, false)
            }
        };
        let add_trailing_paragraph = at_end && {
            let primary = primary.read(cx);
            primary.kind().is_atomic_structural()
                || primary.kind().is_quote_container()
                || primary.kind().is_footnote_definition()
        };

        InsertPlan {
            parent,
            index,
            primary,
            focus,
            add_trailing_paragraph,
            rebuild_tables,
            rebuild_footnotes,
        }
    }
}

fn validated_transform_kind(
    current: &BlockKind,
    transform: BlockTransform,
) -> Result<BlockKind, BlockCommandError> {
    if matches!(
        current,
        BlockKind::Table
            | BlockKind::CodeBlock { .. }
            | BlockKind::Comment
            | BlockKind::HtmlBlock
            | BlockKind::MathBlock
            | BlockKind::MermaidBlock
            | BlockKind::RawMarkdown
            | BlockKind::FootnoteDefinition
    ) {
        return Err(BlockCommandError::UnsupportedTransform(current.clone()));
    }

    Ok(match transform {
        BlockTransform::Paragraph => BlockKind::Paragraph,
        BlockTransform::Heading(level) => {
            if !(1..=6).contains(&level) {
                return Err(BlockCommandError::InvalidHeadingLevel(level));
            }
            BlockKind::Heading { level }
        }
        BlockTransform::BulletedList => BlockKind::BulletedListItem,
        BlockTransform::NumberedList => BlockKind::NumberedListItem,
        BlockTransform::TaskList => match current {
            BlockKind::TaskListItem { checked } => BlockKind::TaskListItem { checked: *checked },
            _ => BlockKind::TaskListItem { checked: false },
        },
        BlockTransform::Quote => BlockKind::Quote,
        BlockTransform::Callout(variant) => BlockKind::Callout(variant),
    })
}

fn validate_insert(insert: &BlockInsert) -> Result<(), BlockCommandError> {
    match insert {
        BlockInsert::Table { body_rows, columns } if *body_rows == 0 || *columns == 0 => {
            Err(BlockCommandError::InvalidTableDimensions {
                body_rows: *body_rows,
                columns: *columns,
            })
        }
        BlockInsert::Footnote { id } if id.trim().is_empty() => {
            Err(BlockCommandError::EmptyFootnoteId)
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{AppContext, TestAppContext};

    fn init_editor_test_app(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
    }

    #[test]
    fn heading_levels_are_validated_before_mutation() {
        assert_eq!(
            validated_transform_kind(&BlockKind::Paragraph, BlockTransform::Heading(0)),
            Err(BlockCommandError::InvalidHeadingLevel(0))
        );
        assert_eq!(
            validated_transform_kind(&BlockKind::Paragraph, BlockTransform::Heading(6)),
            Ok(BlockKind::Heading { level: 6 })
        );
        assert_eq!(
            validated_transform_kind(&BlockKind::Paragraph, BlockTransform::Heading(7)),
            Err(BlockCommandError::InvalidHeadingLevel(7))
        );
    }

    #[test]
    fn task_transform_preserves_existing_checked_state() {
        assert_eq!(
            validated_transform_kind(
                &BlockKind::TaskListItem { checked: true },
                BlockTransform::TaskList,
            ),
            Ok(BlockKind::TaskListItem { checked: true })
        );
        assert_eq!(
            validated_transform_kind(&BlockKind::Paragraph, BlockTransform::TaskList),
            Ok(BlockKind::TaskListItem { checked: false })
        );
    }

    #[test]
    fn opaque_and_structural_blocks_fail_closed() {
        for kind in [
            BlockKind::Table,
            BlockKind::CodeBlock { language: None },
            BlockKind::HtmlBlock,
            BlockKind::RawMarkdown,
            BlockKind::FootnoteDefinition,
        ] {
            assert_eq!(
                validated_transform_kind(&kind, BlockTransform::Paragraph),
                Err(BlockCommandError::UnsupportedTransform(kind))
            );
        }
    }

    #[test]
    fn insert_payloads_are_bounded_by_native_shape_requirements() {
        assert_eq!(
            validate_insert(&BlockInsert::Table {
                body_rows: 0,
                columns: 2,
            }),
            Err(BlockCommandError::InvalidTableDimensions {
                body_rows: 0,
                columns: 2,
            })
        );
        assert_eq!(
            validate_insert(&BlockInsert::Footnote { id: " ".into() }),
            Err(BlockCommandError::EmptyFootnoteId)
        );
    }

    #[gpui::test]
    async fn transform_mutates_one_resident_block_and_undo_restores_source(
        cx: &mut TestAppContext,
    ) {
        init_editor_test_app(cx);
        let editor = cx.new(|cx| Editor::embedded_from_markdown(cx, "alpha\n\nbeta".into()));

        editor.update(cx, |editor, cx| {
            let first = editor.document.visible_blocks()[0].entity.clone();
            let second = editor.document.visible_blocks()[1].entity.clone();
            let first_id = first.entity_id();
            let second_id = second.entity_id();

            let outcome = editor
                .execute_block_command(
                    BlockCommand::Transform {
                        target: first_id,
                        transform: BlockTransform::Heading(2),
                    },
                    cx,
                )
                .expect("native heading transform should succeed");

            assert_eq!(
                outcome,
                BlockCommandOutcome::Changed {
                    block: first_id,
                    focused: first_id,
                }
            );
            assert_eq!(
                editor.document.visible_blocks()[0].entity.entity_id(),
                first_id
            );
            assert_eq!(
                editor.document.visible_blocks()[1].entity.entity_id(),
                second_id
            );
            assert_eq!(first.read(cx).kind(), BlockKind::Heading { level: 2 });
            assert_eq!(editor.markdown_text(cx), "## alpha\n\nbeta");
            assert_eq!(editor.undo_history.len(), 1);
            assert_eq!(editor.pending_focus, Some(first_id));

            editor.undo_document(cx);
            assert_eq!(editor.markdown_text(cx), "alpha\n\nbeta");
            assert_eq!(
                editor.document.visible_blocks()[0].entity.read(cx).kind(),
                BlockKind::Paragraph
            );
        });
    }

    #[gpui::test]
    async fn paragraph_and_all_heading_levels_round_trip_exactly(cx: &mut TestAppContext) {
        init_editor_test_app(cx);
        let editor = cx.new(|cx| Editor::embedded_from_markdown(cx, "alpha *beta*".into()));

        editor.update(cx, |editor, cx| {
            let target = editor.document.visible_blocks()[0].entity.entity_id();
            for level in 1..=6 {
                editor
                    .execute_block_command(
                        BlockCommand::Transform {
                            target,
                            transform: BlockTransform::Heading(level),
                        },
                        cx,
                    )
                    .expect("heading transform should succeed");
                assert_eq!(
                    editor.markdown_text(cx),
                    format!(r#"{} alpha *beta*"#, "#".repeat(level as usize))
                );

                editor
                    .execute_block_command(
                        BlockCommand::Transform {
                            target,
                            transform: BlockTransform::Paragraph,
                        },
                        cx,
                    )
                    .expect("paragraph transform should succeed");
                assert_eq!(editor.markdown_text(cx), "alpha *beta*");
            }
            assert_eq!(editor.undo_history.len(), 12);
        });
    }

    #[gpui::test]
    async fn list_and_task_transforms_preserve_inline_text_exactly(cx: &mut TestAppContext) {
        init_editor_test_app(cx);
        let editor = cx.new(|cx| Editor::embedded_from_markdown(cx, "alpha *beta*".into()));

        editor.update(cx, |editor, cx| {
            let target = editor.document.visible_blocks()[0].entity.entity_id();
            for (transform, expected) in [
                (BlockTransform::BulletedList, "- alpha *beta*"),
                (BlockTransform::NumberedList, "1. alpha *beta*"),
                (BlockTransform::TaskList, "- [ ] alpha *beta*"),
                (BlockTransform::Paragraph, "alpha *beta*"),
            ] {
                editor
                    .execute_block_command(BlockCommand::Transform { target, transform }, cx)
                    .expect("native list transform should succeed");
                assert_eq!(editor.markdown_text(cx), expected);
                assert_eq!(
                    editor
                        .document
                        .block_entity_by_id(target)
                        .expect("stable target")
                        .read(cx)
                        .display_text(),
                    "alpha beta"
                );
            }
            assert_eq!(editor.undo_history.len(), 4);
        });
    }

    #[gpui::test]
    async fn table_insert_uses_one_command_and_preserves_native_table_behavior(
        cx: &mut TestAppContext,
    ) {
        init_editor_test_app(cx);
        let editor = cx.new(|cx| Editor::embedded_from_markdown(cx, "alpha".into()));

        editor.update(cx, |editor, cx| {
            let outcome = editor
                .execute_block_command(
                    BlockCommand::Insert {
                        target: BlockInsertTarget::Append,
                        insert: BlockInsert::Table {
                            body_rows: 2,
                            columns: 3,
                        },
                    },
                    cx,
                )
                .expect("native table insert should succeed");

            let BlockCommandOutcome::Changed { block, focused } = outcome else {
                panic!("table insertion must change the document");
            };
            let table = editor
                .document
                .block_entity_by_id(block)
                .expect("inserted table must remain in the resident tree");
            let runtime = table
                .read(cx)
                .table_runtime
                .clone()
                .expect("table runtime must be installed by the command executor");
            assert_eq!(table.read(cx).kind(), BlockKind::Table);
            assert_eq!(runtime.header.len(), 3);
            assert_eq!(runtime.rows.len(), 2);
            assert_eq!(runtime.header[0].entity_id(), focused);
            assert_eq!(editor.pending_focus, Some(focused));
            assert_eq!(editor.undo_history.len(), 1);
            assert_eq!(editor.document.root_count(), 3);

            editor.undo_document(cx);
            assert_eq!(editor.markdown_text(cx), "alpha");
        });
    }

    #[gpui::test]
    async fn native_callout_and_footnote_inserts_do_not_inject_markdown(cx: &mut TestAppContext) {
        init_editor_test_app(cx);
        let editor = cx.new(|cx| Editor::embedded_from_markdown(cx, "alpha".into()));

        editor.update(cx, |editor, cx| {
            let callout = editor
                .execute_block_command(
                    BlockCommand::Insert {
                        target: BlockInsertTarget::Append,
                        insert: BlockInsert::Callout(CalloutVariant::Warning),
                    },
                    cx,
                )
                .expect("native callout insert should succeed");
            let BlockCommandOutcome::Changed { block, focused } = callout else {
                panic!("callout insertion must change the document");
            };
            let callout = editor
                .document
                .block_entity_by_id(block)
                .expect("callout must be in the resident tree");
            assert_eq!(
                callout.read(cx).kind(),
                BlockKind::Callout(CalloutVariant::Warning)
            );
            assert_eq!(callout.read(cx).children[0].entity_id(), focused);

            let footnote = editor
                .execute_block_command(
                    BlockCommand::Insert {
                        target: BlockInsertTarget::Append,
                        insert: BlockInsert::Footnote { id: "proof".into() },
                    },
                    cx,
                )
                .expect("native footnote insert should succeed");
            let BlockCommandOutcome::Changed { block, focused } = footnote else {
                panic!("footnote insertion must change the document");
            };
            let footnote = editor
                .document
                .block_entity_by_id(block)
                .expect("footnote must be in the resident tree");
            assert_eq!(footnote.read(cx).kind(), BlockKind::FootnoteDefinition);
            assert_eq!(footnote.read(cx).children[0].entity_id(), focused);
            assert!(editor.footnote_registry.binding("proof").is_some());
        });
    }
}
