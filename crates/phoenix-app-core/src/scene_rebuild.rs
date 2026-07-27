use super::*;
use phoenix_scene_compiler::{
    compile_active_document, NativeSceneCompileReceipt, NativeSceneCompilerInput,
};
use phoenix_scene_contract::AnchorSource;

#[derive(Debug)]
pub struct NativeScenePublishCommand {
    pub(super) publication: NativeScenePublication,
    pub(super) anchors: Option<Arc<VerifiedDocumentAnchors>>,
    pub(super) compile_receipt: Option<NativeSceneCompileReceipt>,
}

impl NativeScenePublishCommand {
    pub fn backend(publication: NativeScenePublication) -> Self {
        Self {
            publication,
            anchors: None,
            compile_receipt: None,
        }
    }

    fn compiled(
        publication: NativeScenePublication,
        anchors: Arc<VerifiedDocumentAnchors>,
        compile_receipt: NativeSceneCompileReceipt,
    ) -> Self {
        Self {
            publication,
            anchors: Some(anchors),
            compile_receipt: Some(compile_receipt),
        }
    }
}

impl PhoenixKernel {
    pub fn rebuild_active_scene(&self) -> Result<CommandReceipt, KernelError> {
        if self.shutdown_started.load(Ordering::Acquire) {
            return Err(KernelError::ShuttingDown);
        }
        let run_id = atlas_control::begin_graph_build(&self.shared)?;
        let result = (|| {
            let publisher = {
                self.shared
                    .publisher
                    .as_ref()
                    .map(Arc::clone)
                    .ok_or(KernelError::ProductionPublisherUnavailable)?
            };
            #[cfg(not(test))]
            {
                let analysis_generation = publisher.next_generation()?;
                self.analyze_active_document(analysis_generation)?;
            }
            let generation_id = publisher.next_generation()?;
            let (document, registry, palette, registry_revision) = {
                let state = self
                    .shared
                    .state
                    .read()
                    .map_err(|_| KernelError::Poisoned("state read"))?;
                (
                    state
                        .active_document_lease
                        .as_ref()
                        .map(Arc::clone)
                        .ok_or(KernelError::ActiveSceneDocumentUnavailable)?,
                    Arc::clone(&state.entity_registry),
                    *state.highlight_palette,
                    state.atlas_registry.registry_revision,
                )
            };
            let compiled = compile_active_document(NativeSceneCompilerInput {
                generation_id,
                registry_revision,
                document: &document,
                registry: &registry,
                palette,
            })?;
            let anchors = Arc::new(VerifiedDocumentAnchors::verify(
                DocumentId(document.entry_id.0),
                document.revision.0,
                document.content_hash.0,
                Some(GraphGeneration(generation_id)),
                AnchorSource::ResidentGraph,
                &document.content,
                compiled.anchors,
            )?);
            self.execute(KernelCommand::PublishNativeScene(Box::new(
                NativeScenePublishCommand::compiled(
                    compiled.publication,
                    anchors,
                    compiled.receipt,
                ),
            )))
        })();
        atlas_control::finish_graph_build(&self.shared, run_id, &result)?;
        result
    }
}
