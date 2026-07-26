use graph_model::{
    EdgeId, EdgeVisual, GraphRevision, GraphSnapshot, ModelError, NodeId, NodeVisual,
};

fn node(id: u64) -> NodeVisual {
    NodeVisual {
        id: NodeId(id),
        position: [id as f32, 0.0, 0.0],
        radius: 1.0,
        color: [0.2, 0.4, 0.8, 1.0],
        kind: 0,
        flags: 0,
    }
}

fn edge(id: u64, source: u64, target: u64) -> EdgeVisual {
    EdgeVisual {
        id: EdgeId(id),
        source: NodeId(source),
        target: NodeId(target),
        width: 1.0,
        color: [0.5, 0.5, 0.5, 0.5],
        kind: 0,
        flags: 0,
    }
}

#[test]
fn valid_snapshot_returns_exact_inventory() {
    let snapshot = GraphSnapshot::new(
        GraphRevision(1),
        vec![node(1), node(2)],
        vec![edge(10, 1, 2)],
    );
    let inventory = snapshot
        .validate()
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(inventory.node_ids.len(), 2);
    assert_eq!(inventory.edge_ids.len(), 1);
}

#[test]
fn duplicate_identity_is_rejected() {
    let snapshot = GraphSnapshot::new(GraphRevision(1), vec![node(1), node(1)], Vec::new());
    assert_eq!(
        snapshot.validate().err(),
        Some(ModelError::DuplicateNode(NodeId(1)))
    );
}

#[test]
fn missing_endpoint_is_rejected() {
    let snapshot = GraphSnapshot::new(GraphRevision(1), vec![node(1)], vec![edge(10, 1, 9)]);
    assert_eq!(
        snapshot.validate().err(),
        Some(ModelError::MissingTargetNode {
            edge_id: EdgeId(10),
            target_id: NodeId(9),
        })
    );
}

#[test]
fn non_finite_visual_is_rejected() {
    let mut invalid = node(1);
    invalid.position[0] = f32::NAN;
    let snapshot = GraphSnapshot::new(GraphRevision(1), vec![invalid], Vec::new());
    assert_eq!(
        snapshot.validate().err(),
        Some(ModelError::InvalidNodeVisual(NodeId(1)))
    );
}
