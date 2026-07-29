//! Deterministic native compiler from canonical workspace evidence to a packed scene publication.

mod compile;
mod error;
mod layout;
mod scene_build;

#[cfg(test)]
mod tests;

pub use compile::{
    compile_active_document, compile_graph_generation, proposed_nli_edge_id, CompiledNativeScene,
    NativeSceneCompileReceipt, NativeSceneCompilerInput,
};
pub use error::NativeSceneCompilerError;
pub use layout::project_node_positions;
pub use layout::{compile_caps_positions, CapsNode};
pub use phoenix_scene_contract::NATIVE_SCENE_COMPILER_CONTRACT;
