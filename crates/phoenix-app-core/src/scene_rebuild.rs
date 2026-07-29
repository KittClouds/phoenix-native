use super::*;
use phoenix_scene_compiler::{
    compile_active_document, compile_graph_generation, NativeSceneCompileReceipt,
    NativeSceneCompilerInput,
};
use phoenix_scene_contract::AnchorSource;
use std::time::Instant;

#[derive(Debug)]
pub struct NativeScenePublishCommand {
    pub(super) publication: NativeScenePublication,
    pub(super) anchors: Option<Arc<VerifiedDocumentAnchors>>,
    pub(super) compile_receipt: Option<NativeSceneCompileReceipt>,
    pub(super) run_id: Option<u64>,
    pub(super) graph_generation: Option<Arc<VerifiedGraphGeneration>>,
}

impl NativeScenePublishCommand {
    pub fn backend(publication: NativeScenePublication) -> Self {
        Self {
            publication,
            anchors: None,
            compile_receipt: None,
            run_id: None,
            graph_generation: None,
        }
    }

    fn compiled(
        publication: NativeScenePublication,
        anchors: Arc<VerifiedDocumentAnchors>,
        compile_receipt: NativeSceneCompileReceipt,
        run_id: u64,
    ) -> Self {
        Self {
            publication,
            anchors: Some(anchors),
            compile_receipt: Some(compile_receipt),
            run_id: Some(run_id),
            graph_generation: None,
        }
    }

    pub(super) fn reviewed(
        publication: NativeScenePublication,
        anchors: Arc<VerifiedDocumentAnchors>,
        compile_receipt: NativeSceneCompileReceipt,
        run_id: u64,
        graph_generation: Arc<VerifiedGraphGeneration>,
    ) -> Self {
        Self {
            publication,
            anchors: Some(anchors),
            compile_receipt: Some(compile_receipt),
            run_id: Some(run_id),
            graph_generation: Some(graph_generation),
        }
    }
}

impl PhoenixKernel {
    /// Runs the complete production pipeline used by Atlas Control:
    /// document-bound analysis, candidate-only NLI publication, native scene
    /// compilation, and atomic archive/product-index publication.
    pub fn run_active_document_pipeline(&self) -> Result<CommandReceipt, KernelError> {
        let config = LegacyAnalysisAdapterConfig::from_env()?;
        self.run_active_document_pipeline_with(&config)
    }

    pub fn run_active_document_pipeline_with(
        &self,
        config: &LegacyAnalysisAdapterConfig,
    ) -> Result<CommandReceipt, KernelError> {
        self.run_pipeline(Some(config))
    }

    /// Compatibility name for graph-only test fixtures and existing graph
    /// controls. Production builds route through the exact pipeline above.
    pub fn rebuild_active_scene(&self) -> Result<CommandReceipt, KernelError> {
        #[cfg(not(test))]
        {
            self.run_active_document_pipeline()
        }
        #[cfg(test)]
        {
            self.run_pipeline(None)
        }
    }

    fn run_pipeline(
        &self,
        analysis: Option<&LegacyAnalysisAdapterConfig>,
    ) -> Result<CommandReceipt, KernelError> {
        if self.shutdown_started.load(Ordering::Acquire) {
            return Err(KernelError::ShuttingDown);
        }
        let pipeline_started = Instant::now();
        let metrics_before = self.metrics();
        let previous_generation = read_state(&self.shared)?
            .scene_publication
            .map(|receipt| receipt.generation_id);
        let run_id = atlas_control::begin_graph_build(&self.shared)?;
        self.shared.producer_cancel.store(false, Ordering::Release);
        let mut analysis_receipt = None;
        let mut analysis_total_micros = 0_u64;
        let mut publisher_micros = 0_u64;
        let command_result = (|| {
            let publisher = {
                self.shared
                    .publisher
                    .as_ref()
                    .map(Arc::clone)
                    .ok_or(KernelError::ProductionPublisherUnavailable)?
            };
            if let Some(config) = analysis {
                let analysis_generation = publisher.next_generation()?;
                let started = Instant::now();
                analysis_receipt =
                    Some(self.analyze_active_document_with(analysis_generation, config)?);
                analysis_total_micros = elapsed_micros(started);
            }
            let generation_id = publisher.next_generation()?;
            let (document, registry, anchors, nli, graph_generation, palette, registry_revision) = {
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
                    state
                        .document_anchors
                        .as_ref()
                        .map(Arc::clone)
                        .ok_or(KernelError::DocumentAnchorsNotActive)?,
                    state.nli_analysis.as_ref().map(Arc::clone),
                    state.graph_generation.as_ref().map(Arc::clone),
                    *state.highlight_palette,
                    state.atlas_registry.registry_revision,
                )
            };
            let compiler_input = NativeSceneCompilerInput {
                generation_id,
                registry_revision,
                document: &document,
                registry: &registry,
                verified_anchors: Some(&anchors),
                nli: nli.as_deref(),
                palette,
            };
            let compiled = if analysis.is_some() {
                compile_graph_generation(
                    compiler_input,
                    graph_generation
                        .as_deref()
                        .ok_or(KernelError::AnalysisAuthorityMismatch)?,
                )?
            } else {
                compile_active_document(compiler_input)?
            };
            let anchors = Arc::new(VerifiedDocumentAnchors::verify(
                DocumentId(document.entry_id.0),
                document.revision.0,
                document.content_hash.0,
                Some(GraphGeneration(generation_id)),
                AnchorSource::ResidentGraph,
                &document.content,
                compiled.anchors,
            )?);
            let started = Instant::now();
            let result = self.execute(KernelCommand::PublishNativeScene(Box::new(
                NativeScenePublishCommand::compiled(
                    compiled.publication,
                    anchors,
                    compiled.receipt,
                    run_id,
                ),
            )));
            publisher_micros = elapsed_micros(started);
            result
        })();
        let metrics_after = self.metrics();
        let total_micros = elapsed_micros(pipeline_started);
        let (result, durable_run) = match command_result {
            Ok(command) => match command.outcome {
                KernelOutcome::GraphRebuilt(graph) => {
                    let durable = (|| {
                        let (nli, coordinator, product_index) = {
                            let state = read_state(&self.shared)?;
                            (
                                state.nli_analysis.as_ref().map(Arc::clone),
                                state.producer_coordinator.as_ref().map(Arc::clone),
                                state
                                    .scene_product_index
                                    .as_ref()
                                    .map(Arc::clone)
                                    .ok_or(KernelError::ProductIndexWithoutScene)?,
                            )
                        };
                        let receipt = atlas_run::completed_receipt(atlas_run::CompletedRunInput {
                            run_id,
                            previous_generation,
                            analysis: analysis_receipt,
                            analysis_total_micros,
                            graph,
                            publisher_micros,
                            total_micros,
                            metrics_before,
                            metrics_after,
                            nli: nli.as_deref(),
                            coordinator: coordinator.as_deref(),
                            product_index: &product_index,
                        })?;
                        let hash = atlas_run::persist(&self.shared.workspace_path, &receipt)?;
                        Ok::<_, KernelError>((hash, receipt))
                    })();
                    match durable {
                        Ok(run) => (Ok(command), Some(run)),
                        Err(error) => (Err(error), None),
                    }
                }
                _ => (Ok(command), None),
            },
            Err(error) => (Err(error), None),
        };
        atlas_control::finish_graph_build(&self.shared, run_id, &result, durable_run)?;
        result
    }
}

fn elapsed_micros(started: Instant) -> u64 {
    started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64
}
