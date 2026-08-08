use phoenix_graph_generation_v2::PageKind;
use phoenix_scene_contract::{CapsRole, VisualNodeKind};
use thiserror::Error;

#[derive(Debug, Error, Eq, PartialEq)]
pub enum NativeSceneCompilerError {
    #[error("scene generation zero is reserved")]
    ZeroGeneration,
    #[error("registry revision {provided} does not match canonical revision {canonical}")]
    RegistryRevisionMismatch { provided: u64, canonical: u64 },
    #[error("active document has no verified entity mentions")]
    NoVerifiedMentions,
    #[error("verified document anchors do not match compiler authority")]
    AnchorAuthorityMismatch,
    #[error("NLI candidate artifact does not match compiler authority")]
    NliAuthorityMismatch,
    #[error("packed graph generation does not match compiler authority")]
    GraphGenerationAuthorityMismatch,
    #[error("V2 generation page {0:?} is unavailable or has an invalid packed layout")]
    V2InvalidPage(PageKind),
    #[error("V2 generation, review catalog, or publication authority does not match")]
    V2AuthorityMismatch,
    #[error("V2 candidate at review page {page} row {row} has no exact review binding")]
    V2MissingReviewBinding { page: u16, row: u32 },
    #[error("V2 candidate {0:?} has no exact decision receipt")]
    V2DecisionReceiptMismatch(phoenix_graph_generation_v2::CandidateId),
    #[error("V2 candidate status {0} is unsupported")]
    V2CandidateStatus(u16),
    #[error("V2 entity kind {0} is unsupported")]
    V2EntityKind(u16),
    #[error("V2 topology endpoint {0} has no scene node")]
    V2MissingEndpoint(u64),
    #[error("V2 string reference is corrupt")]
    V2StringReference,
    #[error("V3 visual contract is invalid: {0}")]
    V3VisualContract(String),
    #[error("verified mention {start}..{end} no longer matches the active document")]
    StaleMention { start: u32, end: u32 },
    #[error("document contains {actual} bytes; scene references support at most {maximum}")]
    DocumentTooLarge { actual: usize, maximum: usize },
    #[error("chunk contains {actual} unique entities; maximum is {maximum}")]
    DenseChunk { actual: usize, maximum: usize },
    #[error("compiled edge count exceeds the maximum of {0}")]
    EdgeLimit(usize),
    #[error("stable {resource} identity collision")]
    IdentityCollision { resource: &'static str },
    #[error("integer range overflow while compiling {0}")]
    RangeOverflow(&'static str),
    #[error("highlight palette is invalid")]
    InvalidPalette,
    #[error("scene {resource} {id} has no graph palette key")]
    PaletteKeyMissing { resource: &'static str, id: u64 },
    #[error("CAPS node slot {slot} has reserved identity zero")]
    CapsZeroIdentity { slot: usize },
    #[error("CAPS node slot {slot} maps semantic kind {kind:?} into incompatible role {role:?}")]
    CapsSemanticRole {
        slot: usize,
        role: CapsRole,
        kind: VisualNodeKind,
    },
    #[error("CAPS node slot {slot} has parent slot {parent} outside the node page")]
    CapsParentOutOfRange { slot: usize, parent: u32 },
    #[error("CAPS node slot {slot} cannot descend from parent slot {parent}")]
    CapsParentRole { slot: usize, parent: u32 },
    #[error("CAPS node slot {slot} has sibling rank {rank} outside count {count}")]
    CapsSiblingRange { slot: usize, rank: u32, count: u32 },
    #[error("CAPS node slot {slot} produced a non-finite or out-of-ball projection")]
    CapsProjectionInvalid { slot: usize },
    #[error(transparent)]
    HybridLayout(#[from] phoenix_hybrid_space::HybridLayoutError),
    #[error(transparent)]
    HopfLayout(#[from] phoenix_hopf_space::HopfLayoutError),
    #[error(transparent)]
    V2Structural(#[from] crate::StructuralSourceError),
    #[error(transparent)]
    V2Topology(#[from] phoenix_graph_generation_v2::TopologyValidationError),
}
