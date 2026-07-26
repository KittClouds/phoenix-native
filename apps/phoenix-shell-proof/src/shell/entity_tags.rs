use super::PhoenixShell;
use gpui::{Context, Entity};
use phoenix_app_core::{EntityTagCommand, KernelCommand, KernelOutcome};
use phoenix_scene_contract::EntityKind;
use phoenix_workspace::EntityTag;
use std::sync::Arc;
use velotype::{EntityTagKind, EntityTagRequest};

impl PhoenixShell {
    pub(super) fn tag_entity_selection(
        &mut self,
        editor: Entity<velotype::Editor>,
        request: &EntityTagRequest,
        cx: &mut Context<Self>,
    ) {
        let Some(lease) = self.editor_lease.as_ref().map(Arc::clone) else {
            self.status = "TAG BLOCKED / NO ACTIVE KERNEL DOCUMENT LEASE".into();
            cx.notify();
            return;
        };
        let editor_revision = editor.read_with(cx, |editor, _| editor.document_revision());
        if request.editor_revision != editor_revision {
            self.status = "TAG BLOCKED / EDITOR SELECTION REVISION IS STALE".into();
            cx.notify();
            return;
        }
        let Ok(start) = u32::try_from(request.source_range.start) else {
            self.status = "TAG BLOCKED / SELECTION START EXCEEDS NATIVE RANGE".into();
            cx.notify();
            return;
        };
        let Ok(end) = u32::try_from(request.source_range.end) else {
            self.status = "TAG BLOCKED / SELECTION END EXCEEDS NATIVE RANGE".into();
            cx.notify();
            return;
        };
        let content: Arc<str> =
            Arc::from(editor.read_with(cx, |editor, cx| editor.host_document_text(cx)));
        let kind = native_kind(request.kind);
        let custom_kind = request.custom_kind.as_ref().map(ToString::to_string);
        let command = KernelCommand::TagSelection(Box::new(EntityTagCommand {
            lease: lease.token(),
            content,
            tag: EntityTag {
                kind,
                custom_kind: custom_kind.clone(),
                start,
                end,
                surface: request.surface.to_string(),
            },
        }));
        match self.kernel.execute(command) {
            Ok(receipt) => {
                let KernelOutcome::EntityTagged(result) = receipt.outcome else {
                    self.status = "TAG BLOCKED / UNEXPECTED KERNEL RECEIPT".into();
                    cx.notify();
                    return;
                };
                let Ok(snapshot) = self.kernel.snapshot() else {
                    self.status = "TAG BLOCKED / KERNEL SNAPSHOT UNAVAILABLE".into();
                    cx.notify();
                    return;
                };
                self.editor_lease = snapshot.active_document_lease;
                editor.update(cx, |editor, cx| editor.mark_embedded_saved(cx));
                self.apply_kernel_highlights(cx);
                let entity_count = snapshot.entity_registry.entities().len();
                let mention_count = self
                    .editor_lease
                    .as_deref()
                    .map(|lease| snapshot.entity_registry.active_mentions_for(lease).count())
                    .unwrap_or(0);
                let kind_label = custom_kind.as_deref().unwrap_or_else(|| kind.label());
                self.status = format!(
                    "TAGGED / {} / {} / ENTITY {:016X} / REGISTRY {} / MENTIONS {} / SEQUENCE {}",
                    request.surface,
                    kind_label,
                    result.entity_id,
                    entity_count,
                    mention_count,
                    receipt.sequence
                )
                .into();
                let _ = self.kernel.drain_events();
            }
            Err(error) => {
                if let Ok(snapshot) = self.kernel.snapshot() {
                    self.editor_lease = snapshot.active_document_lease;
                }
                self.status = format!("TAG BLOCKED / {error}").into();
            }
        }
        cx.notify();
    }
}

const fn native_kind(kind: EntityTagKind) -> EntityKind {
    match kind {
        EntityTagKind::Character => EntityKind::Character,
        EntityTagKind::Location => EntityKind::Location,
        EntityTagKind::Npc => EntityKind::Npc,
        EntityTagKind::Faction => EntityKind::Faction,
        EntityTagKind::Event => EntityKind::Event,
        EntityTagKind::Concept => EntityKind::Concept,
        EntityTagKind::Custom => EntityKind::Custom,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toolbar_kind_adapter_is_complete() {
        assert_eq!(native_kind(EntityTagKind::Character), EntityKind::Character);
        assert_eq!(native_kind(EntityTagKind::Npc), EntityKind::Npc);
        assert_eq!(native_kind(EntityTagKind::Faction), EntityKind::Faction);
        assert_eq!(native_kind(EntityTagKind::Custom), EntityKind::Custom);
    }
}
