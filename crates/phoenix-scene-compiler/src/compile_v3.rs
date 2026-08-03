//! V3 visual-contract compiler boundary.
//!
//! The projection kernel remains the proven V2 implementation for this
//! additive cut.  V3 wraps its result with typed visual lanes and a receipt so
//! production can validate the exact topology/style interpretation without
//! changing the archive bytes or creating a second graph model.

use crate::{
    compile_graph_generation_v2, NativeSceneCompileReceiptV2, NativeSceneCompilerError,
    NativeSceneCompilerV2Input, VisualContractDraftV3,
};
use phoenix_graph_generation_v2::VerifiedGraphGenerationV2;
use phoenix_scene_contract::HighlightPalette;
use phoenix_scene_publisher::NativeScenePublication;
use phoenix_semantic_review::ReviewCatalog;

pub struct NativeSceneCompilerV3Input<'a> {
    pub scene_generation_id: u64,
    pub generation: &'a VerifiedGraphGenerationV2,
    pub review_catalog: &'a ReviewCatalog,
    pub palette: HighlightPalette,
}

pub struct CompiledNativeSceneV3 {
    pub publication: NativeScenePublication,
    pub receipt: NativeSceneCompileReceiptV2,
    pub visual: VisualContractDraftV3,
}

pub fn compile_graph_generation_v3(
    input: NativeSceneCompilerV3Input<'_>,
) -> Result<CompiledNativeSceneV3, NativeSceneCompilerError> {
    let compiled = compile_graph_generation_v2(NativeSceneCompilerV2Input {
        scene_generation_id: input.scene_generation_id,
        generation: input.generation,
        review_catalog: input.review_catalog,
        palette: input.palette,
    })?;
    let visual = VisualContractDraftV3::from_publication(&compiled.publication)
        .map_err(|error| NativeSceneCompilerError::V3VisualContract(error.to_string()))?;
    Ok(CompiledNativeSceneV3 {
        publication: compiled.publication,
        receipt: compiled.receipt,
        visual,
    })
}
