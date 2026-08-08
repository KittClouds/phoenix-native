//! Deterministic native compiler from canonical workspace evidence to a packed scene publication.

mod compile_v2;
mod compile_v3;
mod error;
mod layout;
mod structural_source;
mod v2_projection;
mod visual_v3;

#[cfg(test)]
mod v2_tests;

pub use compile_v2::{
    compile_graph_generation_v2, semantic_candidate_edge_id, CompiledNativeSceneV2,
    NativeSceneCompileReceiptV2, NativeSceneCompilerV2Input,
};
pub use compile_v3::{
    compile_graph_generation_v3, CompiledNativeSceneV3, NativeSceneCompilerV3Input,
};
pub use error::NativeSceneCompilerError;
pub use layout::project_node_positions;
pub use layout::{
    compile_caps_layout, compile_caps_positions, compile_hopf_positions, compile_hybrid_positions,
    CapsGuide, CapsLayout, CapsNode, HopfNode, HybridNode,
};
pub use phoenix_scene_contract::NATIVE_SCENE_COMPILER_CONTRACT;
pub use phoenix_scene_contract::NATIVE_SCENE_COMPILER_V2_CONTRACT;
pub use phoenix_scene_contract::NATIVE_SCENE_COMPILER_V3_CONTRACT;
pub use structural_source::{StructuralSourceError, VerifiedStructuralSource};
pub use visual_v3::{
    VisualContractDraftV3, VisualContractError, VisualContractReceiptV3, VisualLaneCount,
    VISUAL_EDGE_KIND_COUNT, VISUAL_NODE_KIND_COUNT,
};

/// The retired V1 compiler is not a feature and cannot enter the build graph.
///
/// ```compile_fail
/// use phoenix_scene_compiler::{compile_active_document, NativeSceneCompilerInput};
/// ```
pub const PRODUCTION_COMPILER_V2_ONLY: () = ();

/// Production registration point for the typed V3 visual contract.
pub const PRODUCTION_COMPILER_V3_ONLY: () = ();
