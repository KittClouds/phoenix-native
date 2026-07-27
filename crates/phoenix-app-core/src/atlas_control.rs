use super::*;
use phoenix_scene_product_index::ReviewState;
use phoenix_scene_publisher::ScenePublicationKind;

pub const ATLAS_CONTROL_CONTRACT: &str = "phoenix.native.atlas-control/v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AtlasStage {
    Source,
    Entities,
    Connections,
    Review,
    Live,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AtlasStageState {
    Complete,
    Ready,
    Waiting,
    NeedsAttention,
    Blocked,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AtlasBuildState {
    WaitingForDocument,
    WaitingForEntities,
    Ready,
    Building,
    Published,
    VerificationRequired,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AtlasPrimaryAction {
    OpenDocument,
    TagEntities,
    BuildGraph,
    RebuildGraph,
    OpenGraph,
    Wait,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AtlasStageSummary {
    pub stage: AtlasStage,
    pub state: AtlasStageState,
    pub value: u64,
    pub detail: Arc<str>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AtlasControlSnapshot {
    pub contract: &'static str,
    pub kernel_revision: u64,
    pub build_state: AtlasBuildState,
    pub primary_action: AtlasPrimaryAction,
    pub headline: Arc<str>,
    pub guidance: Arc<str>,
    pub document_id: Option<u64>,
    pub document_revision: Option<u64>,
    pub content_hash: Option<[u8; 32]>,
    pub content_bytes: u64,
    pub registry_revision: u64,
    pub canonical_entities: u64,
    pub verified_mentions: u64,
    pub user_entities: u64,
    pub ner_entities: u64,
    pub generation_id: Option<u64>,
    pub node_count: u64,
    pub edge_count: u64,
    pub accepted_rows: u64,
    pub proposed_rows: u64,
    pub rejected_rows: u64,
    pub stages: [AtlasStageSummary; 5],
    pub last_build: Option<GraphRebuildReceipt>,
    pub last_error: Option<Arc<str>>,
}

#[derive(Debug, Default)]
pub(super) struct GraphBuildRuntime {
    next_run_id: u64,
    active_run: Option<u64>,
    last_build: Option<GraphRebuildReceipt>,
    last_error: Option<Arc<str>>,
}

impl GraphBuildRuntime {
    fn begin(&mut self) -> Result<u64, KernelError> {
        if self.active_run.is_some() {
            return Err(KernelError::GraphBuildAlreadyRunning);
        }
        self.next_run_id = self
            .next_run_id
            .checked_add(1)
            .ok_or(KernelError::CoordinatorUnavailable)?;
        self.active_run = Some(self.next_run_id);
        self.last_error = None;
        Ok(self.next_run_id)
    }

    fn finish(
        &mut self,
        run_id: u64,
        result: &Result<CommandReceipt, KernelError>,
    ) -> Result<(), KernelError> {
        if self.active_run != Some(run_id) {
            return Err(KernelError::GraphBuildRunMismatch);
        }
        self.active_run = None;
        match result {
            Ok(CommandReceipt {
                outcome: KernelOutcome::GraphRebuilt(receipt),
                ..
            }) => {
                self.last_build = Some(*receipt);
                self.last_error = None;
            }
            Ok(_) => {
                self.last_error = Some(Arc::from("graph rebuild receipt contract mismatch"));
            }
            Err(error) => {
                self.last_error = Some(Arc::from(error.to_string()));
            }
        }
        Ok(())
    }
}

impl PhoenixKernel {
    pub fn atlas_control_snapshot(&self) -> Result<AtlasControlSnapshot, KernelError> {
        let state = read_state(&self.shared)?;
        let runtime = self
            .shared
            .graph_build
            .lock()
            .map_err(|_| KernelError::Poisoned("graph build runtime"))?;
        let lease = state.active_document_lease.as_deref();
        let document_id = lease.map(|document| document.entry_id.0);
        let document_revision = lease.map(|document| document.revision.0);
        let content_hash = lease.map(|document| document.content_hash.0);
        let content_bytes = lease
            .and_then(|document| u64::try_from(document.content.len()).ok())
            .unwrap_or_default();
        let verified_mentions = lease
            .map(|document| state.entity_registry.active_mentions_for(document).count() as u64)
            .unwrap_or_default();
        let publication = state.scene_publication;
        let full_publication =
            publication.filter(|receipt| receipt.kind == ScenePublicationKind::Full);
        let full_for_document = full_publication.is_some_and(|receipt| {
            receipt.document_id == document_id
                && receipt.registry_revision == state.atlas_registry.registry_revision
        });
        let last_build_matches = runtime.last_build.is_some_and(|receipt| {
            Some(receipt.compile.document_id) == document_id
                && Some(receipt.compile.document_revision) == document_revision
                && Some(receipt.compile.content_hash) == content_hash
                && receipt.compile.registry_revision == state.atlas_registry.registry_revision
                && publication.is_some_and(|published| {
                    published.generation_id == receipt.publication.generation_id
                        && published.archive_cohort_hash == receipt.publication.archive_cohort_hash
                        && published.product_index_hash == receipt.publication.product_index_hash
                })
        });
        let (accepted_rows, proposed_rows, rejected_rows) = state
            .scene_product_index
            .as_ref()
            .map(|index| review_counts(index))
            .unwrap_or_default();
        let build_in_progress = runtime.active_run.is_some();
        let failed = runtime.last_error.is_some();
        let (build_state, primary_action, headline, guidance) = decide_control_state(
            lease.is_some(),
            verified_mentions,
            full_for_document,
            last_build_matches,
            build_in_progress,
            failed,
        );
        let generation_id = publication.map(|receipt| receipt.generation_id);
        let node_count = publication.map_or(0, |receipt| receipt.node_count);
        let edge_count = publication.map_or(0, |receipt| receipt.edge_count);
        let stages = build_stages(
            lease.is_some(),
            content_bytes,
            verified_mentions,
            state.atlas_registry.entities.len() as u64,
            full_for_document,
            edge_count,
            accepted_rows,
            proposed_rows,
            generation_id,
            failed,
        );
        Ok(AtlasControlSnapshot {
            contract: ATLAS_CONTROL_CONTRACT,
            kernel_revision: state.revision,
            build_state,
            primary_action,
            headline: Arc::from(headline),
            guidance: Arc::from(guidance),
            document_id,
            document_revision,
            content_hash,
            content_bytes,
            registry_revision: state.atlas_registry.registry_revision,
            canonical_entities: state.atlas_registry.entities.len() as u64,
            verified_mentions,
            user_entities: state.atlas_registry.user_tagged_source_count as u64,
            ner_entities: state.atlas_registry.ner_source_count as u64,
            generation_id,
            node_count,
            edge_count,
            accepted_rows,
            proposed_rows,
            rejected_rows,
            stages,
            last_build: runtime.last_build,
            last_error: runtime.last_error.clone(),
        })
    }
}

pub(super) fn begin_graph_build(shared: &KernelShared) -> Result<u64, KernelError> {
    shared
        .graph_build
        .lock()
        .map_err(|_| KernelError::Poisoned("graph build runtime"))?
        .begin()
}

pub(super) fn finish_graph_build(
    shared: &KernelShared,
    run_id: u64,
    result: &Result<CommandReceipt, KernelError>,
) -> Result<(), KernelError> {
    shared
        .graph_build
        .lock()
        .map_err(|_| KernelError::Poisoned("graph build runtime"))?
        .finish(run_id, result)
}

fn review_counts(index: &PhoenixSceneProductIndexV1) -> (u64, u64, u64) {
    let mut accepted = 0_u64;
    let mut proposed = 0_u64;
    let mut rejected = 0_u64;
    for mask in index
        .nodes()
        .iter()
        .map(|record| record.review_mask)
        .chain(index.edges().iter().map(|record| record.review_mask))
    {
        accepted += u64::from(mask & ReviewState::Accepted as u32 != 0);
        proposed += u64::from(mask & ReviewState::Proposed as u32 != 0);
        rejected += u64::from(mask & ReviewState::Rejected as u32 != 0);
    }
    (accepted, proposed, rejected)
}

fn decide_control_state(
    has_document: bool,
    verified_mentions: u64,
    full_for_document: bool,
    last_build_matches: bool,
    build_in_progress: bool,
    failed: bool,
) -> (
    AtlasBuildState,
    AtlasPrimaryAction,
    &'static str,
    &'static str,
) {
    if build_in_progress {
        return (
            AtlasBuildState::Building,
            AtlasPrimaryAction::Wait,
            "Building the graph",
            "Phoenix is compiling and publishing one verified native generation.",
        );
    }
    if failed {
        return (
            AtlasBuildState::Failed,
            if verified_mentions == 0 {
                AtlasPrimaryAction::TagEntities
            } else {
                AtlasPrimaryAction::RebuildGraph
            },
            "The last build stopped safely",
            "The previous generation is still intact. Fix the stated issue, then try again.",
        );
    }
    if !has_document {
        return (
            AtlasBuildState::WaitingForDocument,
            AtlasPrimaryAction::OpenDocument,
            "Choose a note",
            "Atlas needs one active document lease before it can build.",
        );
    }
    if verified_mentions == 0 {
        return (
            AtlasBuildState::WaitingForEntities,
            AtlasPrimaryAction::TagEntities,
            "Give Atlas an anchor",
            "Select a name in the editor and tag its type. Atlas will use that verified text span.",
        );
    }
    if full_for_document && last_build_matches {
        return (
            AtlasBuildState::Published,
            AtlasPrimaryAction::OpenGraph,
            "Your graph is live",
            "The active note, entity registry, archive, and product index agree.",
        );
    }
    if full_for_document {
        return (
            AtlasBuildState::VerificationRequired,
            AtlasPrimaryAction::RebuildGraph,
            "Verify the current note",
            "A graph is live, but this process has no matching source receipt. Rebuild once to prove it.",
        );
    }
    (
        AtlasBuildState::Ready,
        AtlasPrimaryAction::BuildGraph,
        "Ready to build",
        "Verified anchors are available. One build will create and publish the native graph.",
    )
}

#[allow(clippy::too_many_arguments)]
fn build_stages(
    has_document: bool,
    content_bytes: u64,
    verified_mentions: u64,
    canonical_entities: u64,
    full_for_document: bool,
    edge_count: u64,
    accepted_rows: u64,
    proposed_rows: u64,
    generation_id: Option<u64>,
    failed: bool,
) -> [AtlasStageSummary; 5] {
    let source_state = if has_document {
        AtlasStageState::Complete
    } else {
        AtlasStageState::NeedsAttention
    };
    let entity_state = if verified_mentions > 0 {
        AtlasStageState::Complete
    } else if has_document {
        AtlasStageState::NeedsAttention
    } else {
        AtlasStageState::Waiting
    };
    let connection_state = if failed {
        AtlasStageState::Blocked
    } else if full_for_document {
        AtlasStageState::Complete
    } else if verified_mentions > 0 {
        AtlasStageState::Ready
    } else {
        AtlasStageState::Waiting
    };
    let review_state = if proposed_rows > 0 {
        AtlasStageState::NeedsAttention
    } else if full_for_document {
        AtlasStageState::Complete
    } else {
        AtlasStageState::Waiting
    };
    let live_state = if full_for_document {
        AtlasStageState::Complete
    } else {
        AtlasStageState::Waiting
    };
    [
        AtlasStageSummary {
            stage: AtlasStage::Source,
            state: source_state,
            value: content_bytes,
            detail: Arc::from(if has_document {
                "Active note leased"
            } else {
                "No note selected"
            }),
        },
        AtlasStageSummary {
            stage: AtlasStage::Entities,
            state: entity_state,
            value: verified_mentions,
            detail: Arc::from(if verified_mentions > 0 {
                "Verified text anchors"
            } else if canonical_entities > 0 {
                "No anchors in this note"
            } else {
                "Tag text to begin"
            }),
        },
        AtlasStageSummary {
            stage: AtlasStage::Connections,
            state: connection_state,
            value: edge_count,
            detail: Arc::from(if full_for_document {
                "Native topology published"
            } else {
                "Built after anchors"
            }),
        },
        AtlasStageSummary {
            stage: AtlasStage::Review,
            state: review_state,
            value: proposed_rows,
            detail: Arc::from(if proposed_rows > 0 {
                "Needs your decision"
            } else if accepted_rows > 0 {
                "Nothing waiting"
            } else {
                "Available after build"
            }),
        },
        AtlasStageSummary {
            stage: AtlasStage::Live,
            state: live_state,
            value: generation_id.unwrap_or_default(),
            detail: Arc::from(if full_for_document {
                "Verified generation"
            } else {
                "Previous truth stays live"
            }),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_state_exposes_exactly_one_next_action() {
        assert_eq!(
            decide_control_state(true, 0, false, false, false, false).1,
            AtlasPrimaryAction::TagEntities
        );
        assert_eq!(
            decide_control_state(true, 2, false, false, false, false).1,
            AtlasPrimaryAction::BuildGraph
        );
        assert_eq!(
            decide_control_state(true, 2, true, true, false, false).1,
            AtlasPrimaryAction::OpenGraph
        );
    }

    #[test]
    fn failed_build_never_claims_publication() {
        let (state, action, _, _) = decide_control_state(true, 2, true, true, false, true);
        assert_eq!(state, AtlasBuildState::Failed);
        assert_eq!(action, AtlasPrimaryAction::RebuildGraph);
    }
}
