use phoenix_hopf_space::{HopfLayoutError, HopfNode as KernelNode};
use phoenix_scene_archive::PositionRecord;

pub type HopfNode = KernelNode;

pub fn layout(nodes: &[HopfNode]) -> Result<Vec<PositionRecord>, HopfLayoutError> {
    phoenix_hopf_space::layout(nodes).map(|points| {
        points
            .into_iter()
            .map(|point| PositionRecord {
                position: point.position,
            })
            .collect()
    })
}
