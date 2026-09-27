//! Source-local view: the graph collapses visually to the evidence explicitly
//! bound to the selected object's verified source passages. Scope and source
//! navigation come only from stored provenance; this module never infers.

use super::{drawer::ACCENT, PhoenixShell, TEXT, TEXT_MUTED};
use gpui::{div, prelude::*, rgb, Context, IntoElement, SharedString};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::Sizable;
use phoenix_app_core::{
    KernelSnapshot, SourceAnchorKind, SourceBinding, SourceScope, SourceScopeResolution,
    SourceUnavailable,
};
use phoenix_scene_product_index::NodeId;

const STRIP_BG: u32 = 0x0f1a17;
const SOURCE_WARN: u32 = 0xe3b26a;

/// Per-anchor "Open source" cursor and the last navigation notice. Both are
/// keyed by anchor so a new selection never inherits the previous state.
#[derive(Default)]
pub(super) struct SourceOpenState {
    anchor: Option<u64>,
    next: usize,
    notice: Option<SharedString>,
}

impl SourceOpenState {
    fn for_anchor(&mut self, anchor: u64) -> &mut Self {
        if self.anchor != Some(anchor) {
            *self = Self {
                anchor: Some(anchor),
                ..Self::default()
            };
        }
        self
    }

    fn next_for(&self, anchor: u64, len: usize) -> usize {
        if self.anchor == Some(anchor) && len > 0 {
            self.next % len
        } else {
            0
        }
    }

    fn notice_for(&self, anchor: u64) -> Option<SharedString> {
        (self.anchor == Some(anchor))
            .then(|| self.notice.clone())
            .flatten()
    }
}

impl PhoenixShell {
    fn resolve_source_scope(&self, snapshot: &KernelSnapshot) -> SourceScopeResolution {
        self.source_scope_cache.borrow_mut().resolve(
            snapshot.graph_generation_v2.as_deref(),
            snapshot.graph_selection.node_id,
        )
    }

    pub(super) fn render_source_local_strip(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = self.kernel_snapshot();
        let resolution = snapshot.as_ref().map_or(
            SourceScopeResolution::Unavailable(SourceUnavailable::NoVerifiedSource),
            |snapshot| self.resolve_source_scope(snapshot),
        );
        let mut strip = super::graph_toolbar::mode_strip(STRIP_BG)
            .child(super::graph_toolbar::strip_mark("Source", ACCENT));
        match &resolution {
            SourceScopeResolution::Scoped(scope) => {
                let label = snapshot
                    .as_ref()
                    .and_then(|snapshot| node_label(snapshot, scope.anchor))
                    .unwrap_or_else(|| format!("Node {}", scope.anchor));
                strip = strip.child(scope_summary(scope, label));
                if let Some(notice) = self.source_open.notice_for(scope.anchor) {
                    strip = strip.child(div().text_xs().text_color(rgb(SOURCE_WARN)).child(notice));
                }
                strip = strip
                    .child(div().flex_1())
                    .child(self.open_source_button(scope, cx));
            }
            SourceScopeResolution::Unavailable(reason) => {
                strip = strip
                    .child(unavailable_summary(*reason))
                    .child(div().flex_1());
            }
        }
        strip.child(self.provenance_popover(cx)).child(
            Button::new("source-local-exit")
                .label("Exit")
                .tooltip("Leave source-local view. The graph returns to its prior view.")
                .small()
                .ghost()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.source_open = SourceOpenState::default();
                    this.mutate_graph_view(|next| next.source_local = false, "SOURCE", cx);
                })),
        )
    }

    fn open_source_button(&self, scope: &SourceScope, cx: &mut Context<Self>) -> impl IntoElement {
        let count = scope.targets.len();
        let next = self.source_open.next_for(scope.anchor, count);
        let label: SharedString = if count > 1 {
            format!("Open source {}/{}", next + 1, count).into()
        } else {
            "Open source".into()
        };
        Button::new("source-local-open")
            .label(label)
            .tooltip("Open the exact bound span in the bound document revision.")
            .small()
            .ghost()
            .on_click(cx.listener(|this, _, _, cx| this.open_source_target(cx)))
    }

    fn open_source_target(&mut self, cx: &mut Context<Self>) {
        let Some(snapshot) = self.kernel_snapshot() else {
            self.status = "SOURCE BLOCKED / KERNEL SNAPSHOT".into();
            cx.notify();
            return;
        };
        let SourceScopeResolution::Scoped(scope) = self.resolve_source_scope(&snapshot) else {
            self.status = "SOURCE UNAVAILABLE".into();
            cx.notify();
            return;
        };
        let count = scope.targets.len();
        let index = self.source_open.next_for(scope.anchor, count);
        let Some(target) = scope.targets.get(index).copied() else {
            self.set_source_notice(scope.anchor, "Source unavailable · no bound span", cx);
            return;
        };
        if let Err(notice) = check_binding(self.editor_lease.as_deref(), target) {
            self.set_source_notice(scope.anchor, notice, cx);
            return;
        }
        let (Ok(start), Ok(end)) = (usize::try_from(target.start), usize::try_from(target.end))
        else {
            self.set_source_notice(scope.anchor, "Source stale · span cannot be resolved", cx);
            return;
        };
        if !self
            .editor
            .update(cx, |editor, cx| editor.focus_source_range(start..end, cx))
        {
            self.set_source_notice(scope.anchor, "Source stale · span cannot be resolved", cx);
            return;
        }
        let state = self.source_open.for_anchor(scope.anchor);
        state.notice = None;
        state.next = (index + 1) % count.max(1);
        self.status = format!(
            "SOURCE / REV {} / {}..{}",
            target.document_revision, target.start, target.end
        )
        .into();
        cx.notify();
    }

    fn set_source_notice(
        &mut self,
        anchor: u64,
        notice: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        let notice = notice.into();
        self.status = format!("SOURCE / {notice}").to_uppercase().into();
        self.source_open.for_anchor(anchor).notice = Some(notice);
        cx.notify();
    }
}

/// The open note must be the exact bound document revision and content;
/// otherwise the binding is stale and nothing is opened.
fn check_binding(
    lease: Option<&phoenix_workspace::DocumentLease>,
    target: SourceBinding,
) -> Result<(), SharedString> {
    let Some(lease) = lease else {
        return Err("Source unavailable · bound note is not open".into());
    };
    if lease.entry_id.0 != target.document_id {
        return Err("Source stale · bound to a different note".into());
    }
    if lease.revision.0 != target.document_revision {
        return Err(format!(
            "Source stale · bound to revision {}, note is at revision {}",
            target.document_revision, lease.revision.0
        )
        .into());
    }
    if lease.content_hash.0 != target.content_hash {
        return Err("Source stale · note content differs from the bound revision".into());
    }
    let (start, end) = (target.start as usize, target.end as usize);
    if start >= end || lease.content.get(start..end).is_none() {
        return Err("Source stale · span cannot be resolved".into());
    }
    Ok(())
}

pub(super) fn node_label(snapshot: &KernelSnapshot, node: u64) -> Option<String> {
    let index = snapshot.scene_product_index.as_ref()?;
    let slot = index
        .nodes()
        .iter()
        .position(|record| record.node_id == node)?;
    index
        .label(slot)
        .filter(|label| !label.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            index
                .entity_for_node(NodeId(node))
                .map(|_| format!("Entity {node}"))
        })
}

fn scope_summary(scope: &SourceScope, label: String) -> impl IntoElement {
    let kind = match scope.kind {
        SourceAnchorKind::Passage => "Passage",
        SourceAnchorKind::Evidence => "Evidence",
        SourceAnchorKind::Entity => "Entity",
    };
    let passages = scope.passages.len();
    div()
        .flex()
        .items_center()
        .gap_2()
        .min_w_0()
        .child(
            div()
                .text_xs()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(TEXT))
                .child(label),
        )
        .child(div().text_xs().text_color(rgb(TEXT_MUTED)).child(format!(
            "{kind} · {passages} passage{} · {} in scope",
            if passages == 1 { "" } else { "s" },
            scope.members.len()
        )))
}

fn unavailable_summary(reason: SourceUnavailable) -> impl IntoElement {
    let (headline, detail, warn) = match reason {
        SourceUnavailable::NoSelection => (
            "Select a node or passage",
            "The selection becomes the source-scope anchor.",
            false,
        ),
        SourceUnavailable::NoVerifiedSource => (
            "Source unavailable",
            "This scene has no verified source generation.",
            true,
        ),
        SourceUnavailable::Unbound(_) => (
            "Source unavailable",
            "No verified source binding is stored for this object.",
            true,
        ),
    };
    div()
        .flex()
        .items_center()
        .gap_2()
        .min_w_0()
        .child(
            div()
                .text_xs()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(if warn { SOURCE_WARN } else { TEXT }))
                .child(headline),
        )
        .child(div().text_xs().text_color(rgb(TEXT_MUTED)).child(detail))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_state_never_leaks_between_anchors() {
        let mut state = SourceOpenState::default();
        let entry = state.for_anchor(10);
        entry.next = 2;
        entry.notice = Some("Source stale".into());
        assert_eq!(state.next_for(10, 3), 2);
        assert!(state.notice_for(10).is_some());
        assert_eq!(state.next_for(11, 3), 0);
        assert!(state.notice_for(11).is_none());
        state.for_anchor(11);
        assert_eq!(state.next_for(10, 3), 0);
        assert!(state.notice_for(10).is_none());
    }
}
