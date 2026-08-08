use super::atlas_run::AtlasRunReceiptError;
use super::{
    AtlasRunReceiptV1, KernelError, PhoenixKernel, PRODUCTION_FALLBACK_COUNT,
    PRODUCTION_JSON_GRAPH_FREIGHT, PRODUCTION_RESIDENT_GENERATION_COUNT,
};
use bytemuck::Pod;
use memmap2::Mmap;
use phoenix_graph_generation_v2::{
    CandidateStatus, CausalCandidateRecord, DecisionRecord, EpisodeMembershipRecord, EpisodeRecord,
    EventRecord, IdentityCandidateRecord, MemoryStateCandidateRecord,
    PageKind as GenerationPageKind, TemporalCandidateRecord, TypedRelationshipCandidateRecord,
    VerifiedGraphGenerationV2,
};
use phoenix_scene_archive::{ArchiveManifold, PageKey, PageKind as ScenePageKind};
use phoenix_semantic_review::{ReviewCandidateLocation, ReviewCatalog, ReviewPage};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;

pub const RELEASE_MANIFEST_CONTRACT: &str = "phoenix.native.release-manifest/v2";
pub const RELEASE_MANIFOLD_COUNT: usize = ArchiveManifold::ALL.len();
const MAGIC: [u8; 8] = *b"PHXRLM02";
const FORMAT_VERSION: u32 = 2;
const HEADER_LEN: usize = 64;
const MAX_PAYLOAD_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReleaseGateTargetsV1 {
    pub manifold_switch_cpu_p95_micros: u64,
    pub interaction_frame_p95_micros: u64,
    pub manifold_switches: u32,
    pub drawer_toggles: u32,
}

impl Default for ReleaseGateTargetsV1 {
    fn default() -> Self {
        Self {
            manifold_switch_cpu_p95_micros: 8_000,
            interaction_frame_p95_micros: 16_700,
            manifold_switches: 200,
            drawer_toggles: 200,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReleaseCohortAuthorityV1 {
    pub document_id: u64,
    pub document_revision: u64,
    pub document_hash: [u8; 32],
    pub document_bytes: u64,
    pub registry_revision: u64,
    pub analysis_generation: u64,
    pub graph_generation_hash: [u8; 32],
    pub scene_generation: u64,
    pub archive_cohort_hash: [u8; 32],
    pub product_index_hash: [u8; 32],
    pub runtime_binary_hash: [u8; 32],
    pub atlas_run_hash: [u8; 32],
    pub decision_ledger_hash: [u8; 32],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReleaseCohortCountsV1 {
    pub chunks: u64,
    pub sentences: u64,
    pub spans: u64,
    pub canonical_entities: u64,
    pub mentions: u64,
    pub evidence: u64,
    pub accepted_edges: u64,
    pub candidate_edges: u64,
    pub adjudications: u64,
    pub durable_decisions: u64,
    pub scene_nodes: u64,
    pub scene_edges: u64,
    pub entity_node_mappings: u64,
    pub receipt_backed_promotions: u64,
    pub unreceipted_promotions: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReleaseCohortDigestsV2 {
    pub document_structure: [u8; 32],
    pub entity_mentions_evidence: [u8; 32],
    pub accepted_topology: [u8; 32],
    pub candidate_semantics: [u8; 32],
    pub durable_decisions: [u8; 32],
    pub producer_capabilities: [u8; 32],
    pub shared_scene_pages: [u8; 32],
    pub manifold_positions: [[u8; 32]; RELEASE_MANIFOLD_COUNT],
    pub manifold_guides: [[u8; 32]; RELEASE_MANIFOLD_COUNT],
    pub manifold_paths: [[u8; 32]; RELEASE_MANIFOLD_COUNT],
    pub product_families_reviews_scopes: [u8; 32],
    pub product_labels: [u8; 32],
    pub entity_node_mappings: [u8; 32],
    pub inspector_provenance: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PhoenixReleaseManifestV2 {
    pub contract: String,
    pub authority: ReleaseCohortAuthorityV1,
    pub counts: ReleaseCohortCountsV1,
    pub digests: ReleaseCohortDigestsV2,
    pub run: AtlasRunReceiptV1,
    pub gates: ReleaseGateTargetsV1,
    pub json_graph_freight: u64,
    pub fallback_count: u64,
    pub resident_generation_count: u8,
}

impl PhoenixReleaseManifestV2 {
    pub fn validate(&self) -> Result<(), ReleaseLockError> {
        self.run.validate()?;
        if self.contract != RELEASE_MANIFEST_CONTRACT
            || self.authority.document_id == 0
            || self.authority.document_revision == 0
            || self.authority.document_hash == [0; 32]
            || self.authority.graph_generation_hash == [0; 32]
            || self.authority.runtime_binary_hash == [0; 32]
            || self.authority.atlas_run_hash == [0; 32]
            || self.authority.decision_ledger_hash == [0; 32]
            || self.counts.scene_nodes == 0
            || self.resident_generation_count != 1
            || self.digests.contains_zero()
        {
            return Err(ReleaseLockError::Invalid(
                "required cohort authority is incomplete",
            ));
        }
        let run = &self.run.authority;
        if (
            run.document_id,
            run.document_revision,
            run.content_hash,
            run.registry_revision,
            run.analysis_generation.unwrap_or_default(),
            run.published_generation,
            run.archive_cohort_hash,
            run.product_index_hash,
        ) != (
            self.authority.document_id,
            self.authority.document_revision,
            self.authority.document_hash,
            self.authority.registry_revision,
            self.authority.analysis_generation,
            self.authority.scene_generation,
            self.authority.archive_cohort_hash,
            self.authority.product_index_hash,
        ) {
            return Err(ReleaseLockError::Invalid(
                "run receipt and frozen authority disagree",
            ));
        }
        if self.run.resources.graph_nodes != self.counts.scene_nodes
            || self.run.resources.graph_edges != self.counts.scene_edges
            || self.run.resources.canonical_entities != self.counts.canonical_entities
        {
            return Err(ReleaseLockError::Invalid(
                "run receipt and frozen resource counts disagree",
            ));
        }
        if self.counts.unreceipted_promotions != 0
            || self.json_graph_freight != 0
            || self.fallback_count != 0
        {
            return Err(ReleaseLockError::Invalid(
                "release architecture hard contract failed",
            ));
        }
        Ok(())
    }

    #[must_use]
    pub fn exact_semantic_mismatches(&self, candidate: &Self) -> Vec<&'static str> {
        let mut mismatches = Vec::with_capacity(8);
        if (
            self.authority.document_id,
            self.authority.document_revision,
            self.authority.document_hash,
            self.authority.document_bytes,
        ) != (
            candidate.authority.document_id,
            candidate.authority.document_revision,
            candidate.authority.document_hash,
            candidate.authority.document_bytes,
        ) {
            mismatches.push("document authority");
        }
        if self.authority.registry_revision != candidate.authority.registry_revision {
            mismatches.push("registry revision");
        }
        if self.authority.runtime_binary_hash != candidate.authority.runtime_binary_hash {
            mismatches.push("producer binary");
        }
        if self.run.producers != candidate.run.producers {
            mismatches.push("model identities");
        }
        if (
            self.run.resources,
            self.run.semantics,
            self.run.graph_reviews,
            self.run.decisions,
        ) != (
            candidate.run.resources,
            candidate.run.semantics,
            candidate.run.graph_reviews,
            candidate.run.decisions,
        ) {
            mismatches.push("producer capabilities");
        }
        if self.counts != candidate.counts {
            mismatches.push("resource counts");
        }
        if self.digests != candidate.digests {
            mismatches.push("semantic products");
        }
        if self.json_graph_freight != candidate.json_graph_freight
            || self.fallback_count != candidate.fallback_count
            || self.resident_generation_count != candidate.resident_generation_count
        {
            mismatches.push("architecture hard contracts");
        }
        mismatches
    }

    pub fn write_new(&self, path: &Path) -> Result<[u8; 32], ReleaseLockError> {
        self.validate()?;
        let payload = postcard::to_allocvec(self)
            .map_err(|error| ReleaseLockError::Codec(error.to_string()))?;
        if payload.len() > MAX_PAYLOAD_BYTES {
            return Err(ReleaseLockError::Oversized(payload.len()));
        }
        let payload_hash = *blake3::hash(&payload).as_bytes();
        let mut header = [0_u8; HEADER_LEN];
        header[..8].copy_from_slice(&MAGIC);
        header[8..12].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
        header[12..16].copy_from_slice(&(HEADER_LEN as u32).to_le_bytes());
        header[16..24].copy_from_slice(&(payload.len() as u64).to_le_bytes());
        header[24..56].copy_from_slice(&payload_hash);
        write_new_file(path, &header, &payload)?;
        Ok(payload_hash)
    }

    pub fn open(path: &Path) -> Result<Self, ReleaseLockError> {
        let file = File::open(path).map_err(|source| ReleaseLockError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let file_len = file
            .metadata()
            .map_err(|source| ReleaseLockError::Io {
                path: path.to_path_buf(),
                source,
            })?
            .len();
        if file_len > (HEADER_LEN + MAX_PAYLOAD_BYTES) as u64 {
            return Err(ReleaseLockError::Oversized(
                usize::try_from(file_len).unwrap_or(usize::MAX),
            ));
        }
        // SAFETY: release manifests are immutable after atomic publication.
        let mmap = unsafe { Mmap::map(&file) }.map_err(|source| ReleaseLockError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        if mmap.len() < HEADER_LEN || mmap[..8] != MAGIC {
            return Err(ReleaseLockError::InvalidHeader);
        }
        let version = read_u32(&mmap[8..12]);
        if version != FORMAT_VERSION {
            return Err(ReleaseLockError::UnsupportedVersion(version));
        }
        let payload_len = usize::try_from(read_u64(&mmap[16..24]))
            .map_err(|_| ReleaseLockError::InvalidHeader)?;
        if payload_len > MAX_PAYLOAD_BYTES {
            return Err(ReleaseLockError::Oversized(payload_len));
        }
        if read_u32(&mmap[12..16]) as usize != HEADER_LEN
            || mmap.len() != HEADER_LEN + payload_len
            || mmap[56..HEADER_LEN].iter().any(|byte| *byte != 0)
        {
            return Err(ReleaseLockError::InvalidHeader);
        }
        let payload = &mmap[HEADER_LEN..];
        if mmap[24..56] != *blake3::hash(payload).as_bytes() {
            return Err(ReleaseLockError::HashMismatch);
        }
        let manifest: Self = postcard::from_bytes(payload)
            .map_err(|error| ReleaseLockError::Codec(error.to_string()))?;
        manifest.validate()?;
        Ok(manifest)
    }
}

impl ReleaseCohortDigestsV2 {
    fn contains_zero(&self) -> bool {
        [
            self.document_structure,
            self.entity_mentions_evidence,
            self.accepted_topology,
            self.candidate_semantics,
            self.durable_decisions,
            self.producer_capabilities,
            self.shared_scene_pages,
            self.product_families_reviews_scopes,
            self.product_labels,
            self.entity_node_mappings,
            self.inspector_provenance,
        ]
        .into_iter()
        .chain(self.manifold_positions)
        .chain(self.manifold_guides)
        .chain(self.manifold_paths)
        .any(|digest| digest == [0; 32])
    }
}

impl PhoenixKernel {
    pub fn release_manifest(&self) -> Result<PhoenixReleaseManifestV2, ReleaseLockError> {
        let snapshot = self.snapshot()?;
        let control = self.atlas_control_snapshot()?;
        let lease = snapshot
            .active_document_lease
            .as_deref()
            .ok_or(ReleaseLockError::Missing("active document"))?;
        let scene = snapshot
            .resident_scene
            .as_deref()
            .ok_or(ReleaseLockError::Missing("resident scene"))?;
        let index = snapshot
            .scene_product_index
            .as_deref()
            .ok_or(ReleaseLockError::Missing("scene product index"))?;
        let generation = snapshot
            .graph_generation_v2
            .as_deref()
            .ok_or(ReleaseLockError::Missing("packed V2 graph generation"))?;
        let catalog = snapshot
            .review_catalog_v2
            .as_deref()
            .ok_or(ReleaseLockError::Missing("V2 review catalog"))?;
        let run = control
            .last_run
            .ok_or(ReleaseLockError::Missing("matching Atlas run receipt"))?;
        let analysis_generation = run
            .authority
            .analysis_generation
            .ok_or(ReleaseLockError::Missing("production analysis generation"))?;
        if generation.header().producer_generation != analysis_generation {
            return Err(ReleaseLockError::Invalid(
                "V2 generation and analysis producer disagree",
            ));
        }
        let run_bytes = postcard::to_allocvec(&run)
            .map_err(|error| ReleaseLockError::Codec(error.to_string()))?;
        let decisions = self.atlas_decision_receipts()?;
        let mut decision_hasher = blake3::Hasher::new();
        decision_hasher.update(b"phoenix.native.release-decision-ledger/v1\0");
        for receipt in &decisions {
            decision_hasher.update(&receipt.receipt_id);
        }
        let (receipt_backed_promotions, unreceipted_promotions) =
            promotion_counts(generation, catalog)?;
        let inventory = scene.inventory();
        let digests = release_digests(generation, scene.archive(), index)?;
        let manifest = PhoenixReleaseManifestV2 {
            contract: RELEASE_MANIFEST_CONTRACT.to_owned(),
            authority: ReleaseCohortAuthorityV1 {
                document_id: lease.entry_id.0,
                document_revision: lease.revision.0,
                document_hash: lease.content_hash.0,
                document_bytes: lease.content.len() as u64,
                registry_revision: snapshot.atlas_registry.registry_revision,
                analysis_generation,
                graph_generation_hash: generation.header().generation_hash,
                scene_generation: scene.generation().0,
                archive_cohort_hash: scene.archive_identity().cohort_hash,
                product_index_hash: index.header().index_hash,
                runtime_binary_hash: current_binary_hash()?,
                atlas_run_hash: *blake3::hash(&run_bytes).as_bytes(),
                decision_ledger_hash: *decision_hasher.finalize().as_bytes(),
            },
            counts: ReleaseCohortCountsV1 {
                chunks: page_count(generation, GenerationPageKind::Chunks),
                sentences: page_count(generation, GenerationPageKind::Sentences),
                spans: page_count(generation, GenerationPageKind::Spans),
                canonical_entities: page_count(generation, GenerationPageKind::Entities),
                mentions: page_count(generation, GenerationPageKind::Mentions),
                evidence: page_count(generation, GenerationPageKind::Evidence),
                accepted_edges: receipt_backed_promotions,
                candidate_edges: catalog.candidates().len() as u64,
                adjudications: page_count(generation, GenerationPageKind::NliAdjudications),
                durable_decisions: page_count(generation, GenerationPageKind::Decisions),
                scene_nodes: inventory.node_count as u64,
                scene_edges: inventory.edge_count as u64,
                entity_node_mappings: index.header().mapping_count as u64,
                receipt_backed_promotions,
                unreceipted_promotions,
            },
            digests,
            run,
            gates: ReleaseGateTargetsV1::default(),
            json_graph_freight: PRODUCTION_JSON_GRAPH_FREIGHT,
            fallback_count: PRODUCTION_FALLBACK_COUNT,
            resident_generation_count: PRODUCTION_RESIDENT_GENERATION_COUNT,
        };
        manifest.validate()?;
        Ok(manifest)
    }
}

fn release_digests(
    generation: &VerifiedGraphGenerationV2,
    archive: &phoenix_scene_archive::PhoenixSceneArchiveV1,
    index: &phoenix_scene_product_index::PhoenixSceneProductIndexV1,
) -> Result<ReleaseCohortDigestsV2, ReleaseLockError> {
    let document_structure = hash_record_groups(
        b"phoenix.release.document-structure/v1\0",
        &[
            generation.page_bytes(GenerationPageKind::Documents),
            generation.page_bytes(GenerationPageKind::Chapters),
            generation.page_bytes(GenerationPageKind::Paragraphs),
            generation.page_bytes(GenerationPageKind::Chunks),
            generation.page_bytes(GenerationPageKind::Sentences),
            generation.page_bytes(GenerationPageKind::Spans),
        ],
    );
    let entity_mentions_evidence = hash_record_groups(
        b"phoenix.release.entity-mention-evidence/v1\0",
        &[
            generation.page_bytes(GenerationPageKind::Entities),
            generation.page_bytes(GenerationPageKind::CanonicalEntityBindings),
            generation.page_bytes(GenerationPageKind::Mentions),
            generation.page_bytes(GenerationPageKind::Evidence),
        ],
    );
    let accepted_topology = hash_record_groups(
        b"phoenix.release.accepted-topology/v2\0",
        &[
            generation.page_bytes(GenerationPageKind::StructuralEdges),
            generation.page_bytes(GenerationPageKind::Decisions),
        ],
    );
    let candidate_semantics = hash_record_groups(
        b"phoenix.release.candidate-semantics/v2\0",
        &[
            generation.page_bytes(GenerationPageKind::TypedRelationshipCandidates),
            generation.page_bytes(GenerationPageKind::IdentityCandidates),
            generation.page_bytes(GenerationPageKind::Events),
            generation.page_bytes(GenerationPageKind::Episodes),
            generation.page_bytes(GenerationPageKind::EpisodeMemberships),
            generation.page_bytes(GenerationPageKind::TemporalCandidates),
            generation.page_bytes(GenerationPageKind::CausalCandidates),
            generation.page_bytes(GenerationPageKind::MemoryStateCandidates),
            generation.page_bytes(GenerationPageKind::ContextualEvidence),
            generation.page_bytes(GenerationPageKind::CandidateEvidenceBindings),
            generation.page_bytes(GenerationPageKind::NliAdjudications),
        ],
    );
    let durable_decisions = domain_hash(
        b"phoenix.release.durable-decisions/v2\0",
        generation.page_bytes(GenerationPageKind::Decisions),
    );
    let producer_capabilities = hash_record_groups(
        b"phoenix.release.producer-capabilities/v2\0",
        &[
            generation.page_bytes(GenerationPageKind::Capabilities),
            generation.page_bytes(GenerationPageKind::ModelIdentities),
            generation.page_bytes(GenerationPageKind::StageReceipts),
            generation.page_bytes(GenerationPageKind::PublicationReceipts),
        ],
    );

    let shared_scene_pages = hash_archive_pages(
        archive,
        b"phoenix.release.shared-scene-pages/v1\0",
        &[
            PageKey::shared(ScenePageKind::NodeIdentity),
            PageKey::shared(ScenePageKind::NodeStyle),
            PageKey::shared(ScenePageKind::Topology),
            PageKey::shared(ScenePageKind::Edge),
            PageKey::shared(ScenePageKind::LabelPriority),
            PageKey::shared(ScenePageKind::RelationMasks),
            PageKey::shared(ScenePageKind::PalettePolicy),
        ],
    )?;
    let mut manifold_positions = [[0; 32]; RELEASE_MANIFOLD_COUNT];
    let mut manifold_guides = [[0; 32]; RELEASE_MANIFOLD_COUNT];
    let mut manifold_paths = [[0; 32]; RELEASE_MANIFOLD_COUNT];
    for (slot, manifold) in ArchiveManifold::ALL.into_iter().enumerate() {
        manifold_positions[slot] = hash_archive_pages(
            archive,
            b"phoenix.release.manifold-positions/v1\0",
            &[PageKey::manifold(ScenePageKind::Positions, manifold)],
        )?;
        manifold_guides[slot] = hash_archive_pages(
            archive,
            b"phoenix.release.manifold-guides/v1\0",
            &[PageKey::manifold(ScenePageKind::Guides, manifold)],
        )?;
        manifold_paths[slot] = hash_archive_pages(
            archive,
            b"phoenix.release.manifold-paths/v1\0",
            &[
                PageKey::manifold(ScenePageKind::StraightPaths, manifold),
                PageKey::manifold(ScenePageKind::CurvedPaths, manifold),
                PageKey::manifold(ScenePageKind::BundledPaths, manifold),
            ],
        )?;
    }

    Ok(ReleaseCohortDigestsV2 {
        document_structure,
        entity_mentions_evidence,
        accepted_topology,
        candidate_semantics,
        durable_decisions,
        producer_capabilities,
        shared_scene_pages,
        manifold_positions,
        manifold_guides,
        manifold_paths,
        product_families_reviews_scopes: hash_record_groups(
            b"phoenix.release.product-view-state/v1\0",
            &[
                bytemuck::cast_slice(index.nodes()),
                bytemuck::cast_slice(index.edges()),
            ],
        ),
        product_labels: domain_hash(
            b"phoenix.release.product-labels/v1\0",
            index.label_slab().as_bytes(),
        ),
        entity_node_mappings: hash_records(
            b"phoenix.release.entity-node-mappings/v1\0",
            index.mappings(),
        ),
        inspector_provenance: hash_records(
            b"phoenix.release.inspector-provenance/v1\0",
            index.references(),
        ),
    })
}

fn hash_records<T: Pod>(domain: &[u8], records: &[T]) -> [u8; 32] {
    domain_hash(domain, bytemuck::cast_slice(records))
}

fn hash_record_groups(domain: &[u8], groups: &[&[u8]]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(domain);
    for group in groups {
        hasher.update(&(group.len() as u64).to_le_bytes());
        hasher.update(group);
    }
    *hasher.finalize().as_bytes()
}

fn hash_archive_pages(
    archive: &phoenix_scene_archive::PhoenixSceneArchiveV1,
    domain: &[u8],
    keys: &[PageKey],
) -> Result<[u8; 32], ReleaseLockError> {
    let mut hasher = blake3::Hasher::new();
    hasher.update(domain);
    for &key in keys {
        let page = archive.page(key)?;
        hasher.update(&(key.kind as u16).to_le_bytes());
        hasher.update(&[key.manifold.map_or(u8::MAX, |manifold| manifold as u8)]);
        hasher.update(&(page.bytes.len() as u64).to_le_bytes());
        hasher.update(page.bytes);
    }
    Ok(*hasher.finalize().as_bytes())
}

fn domain_hash(domain: &[u8], bytes: &[u8]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(domain);
    hasher.update(bytes);
    *hasher.finalize().as_bytes()
}

fn page_count(generation: &VerifiedGraphGenerationV2, kind: GenerationPageKind) -> u64 {
    generation.descriptor(kind).count
}

fn promotion_counts(
    generation: &VerifiedGraphGenerationV2,
    catalog: &ReviewCatalog,
) -> Result<(u64, u64), ReleaseLockError> {
    let decisions: &[DecisionRecord] = generation.typed_page(GenerationPageKind::Decisions)?;
    let mut receipt_backed = 0_u64;
    let mut missing = 0_u64;
    for candidate in catalog.candidates() {
        if candidate_status(generation, candidate.location)? != CandidateStatus::Accepted as u16 {
            continue;
        }
        let has_receipt = decisions.iter().any(|decision| {
            decision.candidate_id == candidate.binding.origin.candidate_id
                && decision.action == phoenix_graph_generation_v2::DecisionAction::Accept as u16
                && decision.status == CandidateStatus::Accepted as u16
                && decision.evidence_hash == candidate.binding.evidence_hash
        });
        if has_receipt {
            receipt_backed += 1;
        } else {
            missing += 1;
        }
    }
    Ok((receipt_backed, missing))
}

fn candidate_status(
    generation: &VerifiedGraphGenerationV2,
    location: ReviewCandidateLocation,
) -> Result<u16, ReleaseLockError> {
    let index = location.row_index as usize;
    let status = match ReviewPage::from_raw(location.page) {
        Some(ReviewPage::TypedRelationship) => generation
            .typed_page::<TypedRelationshipCandidateRecord>(
                GenerationPageKind::TypedRelationshipCandidates,
            )?
            .get(index)
            .map(|row| row.status),
        Some(ReviewPage::Identity) => generation
            .typed_page::<IdentityCandidateRecord>(GenerationPageKind::IdentityCandidates)?
            .get(index)
            .map(|row| row.status),
        Some(ReviewPage::Event) => generation
            .typed_page::<EventRecord>(GenerationPageKind::Events)?
            .get(index)
            .map(|row| row.status),
        Some(ReviewPage::Episode) => generation
            .typed_page::<EpisodeRecord>(GenerationPageKind::Episodes)?
            .get(index)
            .map(|row| row.status),
        Some(ReviewPage::EpisodeMembership) => generation
            .typed_page::<EpisodeMembershipRecord>(GenerationPageKind::EpisodeMemberships)?
            .get(index)
            .map(|row| row.status),
        Some(ReviewPage::Temporal) => generation
            .typed_page::<TemporalCandidateRecord>(GenerationPageKind::TemporalCandidates)?
            .get(index)
            .map(|row| row.status),
        Some(ReviewPage::Causal) => generation
            .typed_page::<CausalCandidateRecord>(GenerationPageKind::CausalCandidates)?
            .get(index)
            .map(|row| row.status),
        Some(ReviewPage::MemoryState) => generation
            .typed_page::<MemoryStateCandidateRecord>(GenerationPageKind::MemoryStateCandidates)?
            .get(index)
            .map(|row| row.status),
        None => None,
    };
    status.ok_or(ReleaseLockError::Invalid(
        "review catalog candidate location is invalid",
    ))
}

fn current_binary_hash() -> Result<[u8; 32], ReleaseLockError> {
    let path = std::env::current_exe().map_err(|source| ReleaseLockError::Io {
        path: PathBuf::from("<current-executable>"),
        source,
    })?;
    let mut file = File::open(&path).map_err(|source| ReleaseLockError::Io {
        path: path.clone(),
        source,
    })?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|source| ReleaseLockError::Io {
                path: path.clone(),
                source,
            })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(*hasher.finalize().as_bytes())
}

fn write_new_file(path: &Path, header: &[u8], payload: &[u8]) -> Result<(), ReleaseLockError> {
    if path.exists() {
        return Err(ReleaseLockError::AlreadyExists(path.to_path_buf()));
    }
    let parent = path
        .parent()
        .ok_or_else(|| ReleaseLockError::MissingParent(path.to_path_buf()))?;
    std::fs::create_dir_all(parent).map_err(|source| ReleaseLockError::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|source| ReleaseLockError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    file.write_all(header)
        .and_then(|()| file.write_all(payload))
        .and_then(|()| file.sync_all())
        .map_err(|source| ReleaseLockError::Io {
            path: path.to_path_buf(),
            source,
        })
}

fn read_u32(bytes: &[u8]) -> u32 {
    let mut value = [0; 4];
    value.copy_from_slice(bytes);
    u32::from_le_bytes(value)
}

fn read_u64(bytes: &[u8]) -> u64 {
    let mut value = [0; 8];
    value.copy_from_slice(bytes);
    u64::from_le_bytes(value)
}

#[derive(Debug, Error)]
pub enum ReleaseLockError {
    #[error("kernel authority unavailable: {0}")]
    Kernel(#[from] KernelError),
    #[error("Atlas run receipt invalid: {0}")]
    AtlasRun(#[from] AtlasRunReceiptError),
    #[error("scene archive verification failed: {0}")]
    Archive(#[from] phoenix_scene_archive::ArchiveError),
    #[error("V2 graph generation verification failed: {0}")]
    GraphGenerationV2(#[from] phoenix_graph_generation_v2::GraphGenerationV2Error),
    #[error("release manifest is missing {0}")]
    Missing(&'static str),
    #[error("release manifest is invalid: {0}")]
    Invalid(&'static str),
    #[error("release manifest payload is oversized: {0} bytes")]
    Oversized(usize),
    #[error("release manifest header is invalid")]
    InvalidHeader,
    #[error("release manifest version {0} is unsupported")]
    UnsupportedVersion(u32),
    #[error("release manifest payload hash mismatch")]
    HashMismatch,
    #[error("release manifest codec failed: {0}")]
    Codec(String),
    #[error("release manifest already exists: {0}")]
    AlreadyExists(PathBuf),
    #[error("release manifest path has no parent: {0}")]
    MissingParent(PathBuf),
    #[error("release manifest I/O failed at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}
