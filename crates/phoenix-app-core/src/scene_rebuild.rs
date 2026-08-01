use super::*;
use phoenix_scene_compiler::{
    compile_graph_generation_v2, NativeSceneCompileReceiptV2, NativeSceneCompilerV2Input,
};
use phoenix_scene_contract::{AnchorCandidate, AnchorSource};
use std::time::Instant;

#[derive(Debug)]
pub struct NativeScenePublishCommand {
    pub(super) publication: NativeScenePublication,
    pub(super) anchors: Option<Arc<VerifiedDocumentAnchors>>,
    pub(super) compile_receipt: Option<NativeSceneCompileReceiptV2>,
    pub(super) run_id: Option<u64>,
    pub(super) source_generation_v2: Option<Arc<VerifiedGraphGenerationV2>>,
    pub(super) review_catalog_v2: Option<Arc<ReviewCatalog>>,
}

impl NativeScenePublishCommand {
    #[cfg(test)]
    pub fn backend(publication: NativeScenePublication) -> Self {
        Self {
            publication,
            anchors: None,
            compile_receipt: None,
            run_id: None,
            source_generation_v2: None,
            review_catalog_v2: None,
        }
    }

    fn compiled(
        publication: NativeScenePublication,
        anchors: Arc<VerifiedDocumentAnchors>,
        compile_receipt: NativeSceneCompileReceiptV2,
        run_id: u64,
        source_generation_v2: Arc<VerifiedGraphGenerationV2>,
        review_catalog_v2: Arc<ReviewCatalog>,
    ) -> Self {
        Self {
            publication,
            anchors: Some(anchors),
            compile_receipt: Some(compile_receipt),
            run_id: Some(run_id),
            source_generation_v2: Some(source_generation_v2),
            review_catalog_v2: Some(review_catalog_v2),
        }
    }

    pub(super) fn reviewed(
        publication: NativeScenePublication,
        anchors: Arc<VerifiedDocumentAnchors>,
        compile_receipt: NativeSceneCompileReceiptV2,
        run_id: u64,
        source_generation_v2: Arc<VerifiedGraphGenerationV2>,
        review_catalog_v2: Arc<ReviewCatalog>,
    ) -> Self {
        Self {
            publication,
            anchors: Some(anchors),
            compile_receipt: Some(compile_receipt),
            run_id: Some(run_id),
            source_generation_v2: Some(source_generation_v2),
            review_catalog_v2: Some(review_catalog_v2),
        }
    }
}

impl PhoenixKernel {
    /// Runs the complete production pipeline used by Atlas Control:
    /// document-bound analysis, candidate-only NLI publication, native scene
    /// compilation, and atomic archive/product-index publication.
    pub fn run_active_document_pipeline(&self) -> Result<CommandReceipt, KernelError> {
        let config = NativeProducerRuntimeConfig::from_env()?;
        self.run_active_document_pipeline_with(&config)
    }

    pub fn run_active_document_pipeline_with(
        &self,
        config: &NativeProducerRuntimeConfig,
    ) -> Result<CommandReceipt, KernelError> {
        self.run_pipeline(Some(config))
    }

    /// Rebuild command used by graph controls. Production builds always route
    /// through the exact V2 producer pipeline above; only unit tests may use
    /// the isolated fixture authority.
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
        analysis: Option<&NativeProducerRuntimeConfig>,
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
                let (ner_revision, restored_analysis_generation) = {
                    let state = read_state(&self.shared)?;
                    (
                        state.entity_registry.ner_revision(),
                        state
                            .analysis_publication
                            .map(|receipt| receipt.analysis_generation),
                    )
                };
                let analysis_generation = next_analysis_generation(
                    publisher.next_generation()?,
                    ner_revision,
                    restored_analysis_generation,
                )?;
                let started = Instant::now();
                analysis_receipt =
                    Some(self.analyze_active_document_with(analysis_generation, config)?);
                analysis_total_micros = elapsed_micros(started);
            }
            let generation_id = publisher.next_generation()?;
            let (document, registry, analysis, structural, coordinator, _anchors, palette) = {
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
                    state.document_analysis.as_ref().map(Arc::clone),
                    state.structural_analysis.as_ref().map(Arc::clone),
                    state.producer_coordinator.as_ref().map(Arc::clone),
                    state.document_anchors.as_ref().map(Arc::clone),
                    *state.highlight_palette,
                )
            };
            #[cfg(test)]
            if analysis.is_none() && _anchors.is_none() {
                return Err(KernelError::DocumentAnchorsNotActive);
            }
            let authority = match (
                analysis.as_deref(),
                structural.as_deref(),
                coordinator.as_deref(),
            ) {
                (Some(analysis), Some(structural), Some(coordinator)) => {
                    scene_authority_v2::produce(
                        &self.shared.workspace_path,
                        &document,
                        &registry,
                        analysis,
                        structural,
                        coordinator,
                    )?
                }
                #[cfg(test)]
                (None, None, None) => scene_authority_v2::produce_test_fixture(
                    &self.shared.workspace_path,
                    &document,
                    &registry,
                )?,
                _ => return Err(KernelError::AnalysisAuthorityMismatch),
            };
            let compiled = compile_graph_generation_v2(NativeSceneCompilerV2Input {
                scene_generation_id: generation_id,
                generation: &authority.generation,
                review_catalog: &authority.catalog,
                palette,
            })?;
            let anchors = match analysis.as_deref() {
                Some(analysis) => {
                    analysis::verified_analysis_anchors(&analysis.ner, &document, &registry)?
                }
                #[cfg(test)]
                None => _anchors
                    .as_deref()
                    .cloned()
                    .ok_or(KernelError::DocumentAnchorsNotActive)?,
                #[cfg(not(test))]
                None => return Err(KernelError::AnalysisAuthorityMismatch),
            };
            let anchors = rebind_anchors(anchors, &document, generation_id)?;
            let started = Instant::now();
            let result = self.execute(KernelCommand::PublishNativeScene(Box::new(
                NativeScenePublishCommand::compiled(
                    compiled.publication,
                    anchors,
                    compiled.receipt,
                    run_id,
                    authority.generation,
                    authority.catalog,
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
                        let (nli, product_index) = {
                            let state = read_state(&self.shared)?;
                            (
                                state.nli_analysis.as_ref().map(Arc::clone),
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

pub(super) fn next_analysis_generation(
    next_scene_generation: u64,
    ner_revision: u64,
    restored_analysis_generation: Option<u64>,
) -> Result<u64, KernelError> {
    let next_ner_generation = ner_revision
        .checked_add(1)
        .ok_or(KernelError::AnalysisAuthorityMismatch)?;
    let next_restored_generation = restored_analysis_generation
        .unwrap_or_default()
        .checked_add(1)
        .ok_or(KernelError::AnalysisAuthorityMismatch)?;
    Ok(next_scene_generation
        .max(next_ner_generation)
        .max(next_restored_generation))
}

pub(super) fn rebind_anchors(
    anchors: VerifiedDocumentAnchors,
    document: &DocumentLease,
    generation_id: u64,
) -> Result<Arc<VerifiedDocumentAnchors>, KernelError> {
    let candidates = anchors
        .anchors()
        .iter()
        .map(|anchor| {
            let start = anchor.start as usize;
            let end = anchor.end as usize;
            let surface = document
                .content
                .get(start..end)
                .ok_or(KernelError::DocumentAnchorsNotActive)?;
            Ok(AnchorCandidate {
                start: anchor.start,
                end: anchor.end,
                node_id: anchor.node_id,
                entity_slot: anchor.entity_slot,
                family: anchor.family,
                surface: surface.to_owned(),
            })
        })
        .collect::<Result<Vec<_>, KernelError>>()?;
    Ok(Arc::new(VerifiedDocumentAnchors::verify(
        DocumentId(document.entry_id.0),
        document.revision.0,
        document.content_hash.0,
        Some(GraphGeneration(generation_id)),
        AnchorSource::ResidentGraph,
        &document.content,
        candidates,
    )?))
}

fn elapsed_micros(started: Instant) -> u64 {
    started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64
}
