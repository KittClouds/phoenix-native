use crate::NativeSceneCompilerError;
pub use phoenix_hybrid_space::HybridNode;
use phoenix_scene_archive::PositionRecord;

pub fn layout(nodes: &[HybridNode]) -> Result<Vec<PositionRecord>, NativeSceneCompilerError> {
    Ok(phoenix_hybrid_space::layout(nodes)?
        .into_iter()
        .map(|point| PositionRecord {
            position: point.position,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use phoenix_hybrid_space::HybridLane;

    #[test]
    fn adapter_keeps_native_archive_positions_bounded() {
        let positions = layout(&[
            HybridNode {
                stable_id: 1,
                lane: HybridLane::Structure,
                role: phoenix_scene_contract::CapsRole::Document,
                parent_slot: None,
                sibling_rank: 0,
                sibling_count: 1,
                degree: 4,
            },
            HybridNode {
                stable_id: 2,
                lane: HybridLane::Structure,
                role: phoenix_scene_contract::CapsRole::Chapter,
                parent_slot: Some(0),
                sibling_rank: 0,
                sibling_count: 1,
                degree: 2,
            },
        ])
        .unwrap_or_else(|error| panic!("Hybrid adapter: {error}"));
        assert_eq!(positions.len(), 2);
        assert!(positions.iter().all(|position| position
            .position
            .iter()
            .all(|coordinate| coordinate.is_finite())));
    }
}
