use gpui::{AppContext as _, Bounds, TestAppContext, point, px, size};

use super::{SelectionCommand, SelectionMarkState};
use crate::components::StyleFlag;
use crate::editor::{CrossBlockSelection, CrossBlockSelectionEndpoint, Editor, ViewMode};
use crate::i18n::I18nManager;
use crate::theme::ThemeManager;

fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        I18nManager::init(cx);
        ThemeManager::init(cx);
        crate::components::init(cx);
    });
}

fn redraw(cx: &mut gpui::VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear());
    cx.run_until_parked();
}

#[gpui::test]
async fn rendered_selection_opens_toolbar_from_native_layout(cx: &mut TestAppContext) {
    init(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::embedded_from_markdown(cx, "alpha beta".into()));
    redraw(cx);

    editor.update_in(cx, |editor, window, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        block.update(cx, |block, cx| {
            block.selected_range = 0..5;
            block.focus_handle.focus(window);
            cx.notify();
        });
        editor.active_entity_id = Some(block.entity_id());
        editor.on_selection_changed(cx);
    });
    redraw(cx);

    editor.update_in(cx, |editor, _window, _cx| {
        assert!(editor.selection_toolbar.lease.is_some());
    });

    editor.update_in(cx, |editor, window, cx| {
        editor.open_link_editor(window, cx);
    });
    redraw(cx);

    editor.update_in(cx, |editor, _window, _cx| {
        assert!(editor.selection_toolbar.lease.is_some());
        assert!(editor.selection_toolbar.link_input.is_some());
    });
}

#[gpui::test]
async fn entity_menu_opens_after_toolbar_focus_settles(cx: &mut TestAppContext) {
    init(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::embedded_from_markdown(cx, "alpha beta".into()));
    redraw(cx);
    editor.update_in(cx, |editor, window, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        block.update(cx, |block, cx| {
            block.selected_range = 0..5;
            block.focus_handle.focus(window);
            cx.notify();
        });
        editor.active_entity_id = Some(block.entity_id());
        editor.on_selection_changed(cx);
    });
    redraw(cx);
    editor.update_in(cx, |editor, window, cx| {
        editor.on_selection_toolbar_command(SelectionCommand::Entity, window, cx);
    });
    editor.update_in(cx, |editor, _window, cx| {
        editor.on_selection_changed(cx);
    });
    redraw(cx);
    editor.update_in(cx, |editor, _window, _cx| {
        assert!(editor.selection_toolbar.entity_panel_open);
        assert!(editor.selection_toolbar.lease.is_some());
    });
}

#[gpui::test]
async fn mixed_same_block_format_becomes_uniform_and_records_one_undo(cx: &mut TestAppContext) {
    init(cx);
    let editor = cx.new(|cx| Editor::from_markdown(cx, "**one** two".into(), None));

    editor.update(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        block.update(cx, |block, _| {
            block.selected_range = 0..7;
            block.selection_reversed = false;
        });
        editor.active_entity_id = Some(block.entity_id());

        let identity = editor.current_selection_identity(cx);
        assert_eq!(
            Editor::format_state(
                &editor.current_selection_slices(cx).expect("selection"),
                StyleFlag::Bold,
                cx,
            ),
            SelectionMarkState::Mixed
        );
        assert!(editor.execute_style_command(SelectionCommand::Bold, &identity, cx));
        assert_eq!(editor.markdown_text(cx), "**one two**");
        assert_eq!(editor.undo_history.len(), 1);
    });
}

#[gpui::test]
async fn fully_active_same_block_format_is_removed(cx: &mut TestAppContext) {
    init(cx);
    let editor = cx.new(|cx| Editor::from_markdown(cx, "**one two**".into(), None));

    editor.update(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        block.update(cx, |block, _| {
            block.selected_range = 0..7;
            block.selection_reversed = true;
        });
        editor.active_entity_id = Some(block.entity_id());

        let identity = editor.current_selection_identity(cx);
        assert!(editor.execute_style_command(SelectionCommand::Bold, &identity, cx));
        assert_eq!(editor.markdown_text(cx), "one two");
        assert_eq!(editor.undo_history.len(), 1);
        assert!(block.read(cx).selection_reversed);
        assert_eq!(block.read(cx).selected_range, 0..7);
    });
}

#[gpui::test]
async fn cross_block_format_is_one_transaction_and_preserves_selection(cx: &mut TestAppContext) {
    init(cx);
    let editor = cx.new(|cx| Editor::from_markdown(cx, "alpha\n\nbeta".into(), None));

    editor.update(cx, |editor, cx| {
        let visible = editor.document.visible_blocks().to_vec();
        assert_eq!(visible.len(), 2);
        let first = visible[0].entity.clone();
        let second = visible[1].entity.clone();
        editor.cross_block_selection = Some(CrossBlockSelection {
            anchor: CrossBlockSelectionEndpoint {
                entity_id: first.entity_id(),
                offset: 1,
            },
            focus: CrossBlockSelectionEndpoint {
                entity_id: second.entity_id(),
                offset: 3,
            },
        });
        editor.active_entity_id = Some(second.entity_id());
        editor.sync_cross_block_selection_visuals(cx);

        let identity = editor.current_selection_identity(cx);
        assert!(editor.execute_style_command(SelectionCommand::Italic, &identity, cx));
        assert_eq!(editor.markdown_text(cx), "a*lpha*\n\n*bet*a");
        assert_eq!(editor.undo_history.len(), 1);
        assert!(editor.cross_block_selection.is_some());
    });
}

#[gpui::test]
async fn cross_block_selection_with_raw_block_fails_closed(cx: &mut TestAppContext) {
    init(cx);
    let editor =
        cx.new(|cx| Editor::from_markdown(cx, "alpha\n\n```rust\nlet x = 1;\n```".into(), None));

    editor.update(cx, |editor, cx| {
        let visible = editor.document.visible_blocks().to_vec();
        let first = visible.first().expect("first").entity.clone();
        let last = visible.last().expect("last").entity.clone();
        editor.cross_block_selection = Some(CrossBlockSelection {
            anchor: CrossBlockSelectionEndpoint {
                entity_id: first.entity_id(),
                offset: 0,
            },
            focus: CrossBlockSelectionEndpoint {
                entity_id: last.entity_id(),
                offset: last.read(cx).visible_len(),
            },
        });
        editor.active_entity_id = Some(last.entity_id());

        let identity = editor.current_selection_identity(cx);
        assert!(!editor.execute_style_command(SelectionCommand::Bold, &identity, cx));
        assert_eq!(
            editor.markdown_text(cx),
            "alpha\n\n```rust\nlet x = 1;\n```"
        );
        assert!(editor.undo_history.is_empty());
    });
}

#[gpui::test]
async fn stale_selection_identity_cannot_mutate_document(cx: &mut TestAppContext) {
    init(cx);
    let editor = cx.new(|cx| Editor::from_markdown(cx, "alpha".into(), None));

    editor.update(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        block.update(cx, |block, _| block.selected_range = 0..5);
        editor.active_entity_id = Some(block.entity_id());
        let stale = editor.current_selection_identity(cx);
        editor.document_revision = editor.document_revision.wrapping_add(1);

        assert!(!editor.execute_style_command(SelectionCommand::Bold, &stale, cx));
        assert_eq!(editor.markdown_text(cx), "alpha");
        assert!(editor.undo_history.is_empty());
    });
}

#[gpui::test]
async fn inline_link_round_trips_through_native_tree(cx: &mut TestAppContext) {
    init(cx);
    let editor = cx.new(|cx| Editor::from_markdown(cx, "alpha beta".into(), None));

    editor.update(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        block.update(cx, |block, _| block.selected_range = 0..5);
        editor.active_entity_id = Some(block.entity_id());

        let identity = editor.current_selection_identity(cx);
        assert!(editor.execute_link_command(&identity, Some("https://example.com"), cx));
        assert_eq!(
            editor.markdown_text(cx),
            "[alpha](https://example.com) beta"
        );
        assert_eq!(editor.undo_history.len(), 1);

        let identity = editor.current_selection_identity(cx);
        assert!(editor.execute_link_command(&identity, None, cx));
        assert_eq!(editor.markdown_text(cx), "alpha beta");
        assert_eq!(editor.undo_history.len(), 2);
    });
}

#[gpui::test]
async fn source_mode_never_exposes_rich_selection_slices(cx: &mut TestAppContext) {
    init(cx);
    let editor = cx.new(|cx| Editor::from_markdown(cx, "alpha".into(), None));

    editor.update(cx, |editor, cx| {
        editor.view_mode = ViewMode::Source;
        let block = editor.document.visible_blocks()[0].entity.clone();
        block.update(cx, |block, _| block.selected_range = 0..5);
        assert!(editor.current_selection_slices(cx).is_none());
    });
}

#[test]
fn toolbar_position_clamps_and_flips_inside_viewport() {
    use super::{Editor, SelectionFormatStates, SelectionIdentity, SelectionLease};
    use crate::components::SelectionLinkState;
    use crate::editor::UndoSelectionSnapshot;

    let lease = SelectionLease {
        identity: SelectionIdentity {
            source: UndoSelectionSnapshot {
                range: 0..5,
                reversed: false,
            },
            document_revision: 0,
        },
        bounds: Bounds::new(point(px(2.0), px(4.0)), size(px(30.0), px(18.0))),
        formats: SelectionFormatStates::default(),
        link_state: SelectionLinkState::Off,
        can_link: true,
    };
    let viewport = Bounds::new(point(px(0.0), px(0.0)), size(px(500.0), px(300.0)));
    let position = Editor::toolbar_position(&lease, viewport, 316.0);
    assert_eq!(position.x, px(8.0));
    assert!(position.y > lease.bounds.bottom());

    let embedded_viewport = Bounds::new(point(px(344.0), px(88.0)), size(px(648.0), px(680.0)));
    let mut embedded_lease = lease;
    embedded_lease.bounds = Bounds::new(point(px(427.0), px(187.0)), size(px(253.0), px(26.0)));
    let window_position = Editor::toolbar_position(&embedded_lease, embedded_viewport, 316.0);
    let local_position =
        Editor::toolbar_overlay_position(&embedded_lease, embedded_viewport, 316.0, true);
    assert_eq!(
        local_position,
        point(
            window_position.x - embedded_viewport.left(),
            window_position.y - embedded_viewport.top()
        )
    );
}

#[test]
fn link_destination_validation_is_bounded_and_fail_closed() {
    assert_eq!(
        Editor::validate_link_destination(" https://example.com "),
        Ok("https://example.com")
    );
    assert!(Editor::validate_link_destination("").is_err());
    assert!(Editor::validate_link_destination("https://example.com/a b").is_err());
    assert!(Editor::validate_link_destination(&"x".repeat(2_049)).is_err());
}
