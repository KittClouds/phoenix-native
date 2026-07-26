use graph_model::{
    EdgeId, EdgeVisual, GraphDiff, GraphRevision, GraphSnapshot, ModelError, NodeId, NodeVisual,
};
use graph_render_wgpu::{RenderError, SceneState};
use phoenix_scene_archive::{
    EdgeRecord, ManifoldPageSet, NodeIdentityRecord, NodeStyleRecord, PositionRecord,
    TopologyRecord,
};

fn node(id: u64) -> NodeVisual {
    NodeVisual {
        id: NodeId(id),
        position: [id as f32, 0.0, 0.0],
        radius: 1.0,
        color: [0.2, 0.7, 0.9, 1.0],
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
        color: [0.4, 0.6, 0.8, 0.3],
        kind: 0,
        flags: 0,
    }
}

fn baseline() -> SceneState {
    let mut state = SceneState::default();
    let snapshot = GraphSnapshot::new(
        GraphRevision(1),
        vec![node(1), node(2), node(3)],
        vec![edge(10, 1, 2), edge(11, 2, 3)],
    );
    state
        .set_snapshot(&snapshot)
        .unwrap_or_else(|error| panic!("{error}"));
    state
}

#[test]
fn packed_manifold_updates_preserve_every_stable_slot() {
    let identities = [
        NodeIdentityRecord { id: 101 },
        NodeIdentityRecord { id: 205 },
        NodeIdentityRecord { id: 999 },
    ];
    let styles = [NodeStyleRecord {
        color: [0.1, 0.2, 0.3, 1.0],
        radius: 1.0,
        kind: 1,
        flags: 0,
    }; 3];
    let topology = [TopologyRecord {
        source_id: 101,
        target_id: 999,
    }];
    let edges = [EdgeRecord {
        id: 7,
        color: [0.4, 0.5, 0.6, 0.8],
        width: 1.0,
        kind: 2,
        flags: 0,
    }];
    let hybrid = [
        PositionRecord {
            position: [0.0, 1.0, 2.0],
        },
        PositionRecord {
            position: [3.0, 4.0, 5.0],
        },
        PositionRecord {
            position: [6.0, 7.0, 8.0],
        },
    ];
    let pages = ManifoldPageSet {
        identities: &identities,
        styles: &styles,
        topology: &topology,
        edges: &edges,
        positions: &hybrid,
    };
    let mut state = SceneState::default();
    state
        .set_archive_pages(GraphRevision(9), &pages)
        .unwrap_or_else(|error| panic!("{error}"));
    let slots = [
        state.node_slot(NodeId(101)),
        state.node_slot(NodeId(205)),
        state.node_slot(NodeId(999)),
    ];
    let node_capacity = state.node_capacity_slots();
    let edge_capacity = state.edge_capacity_slots();

    for switch in 0..200 {
        let offset = switch as f32;
        let positions = [
            PositionRecord {
                position: [offset, 1.0, 2.0],
            },
            PositionRecord {
                position: [3.0, offset, 5.0],
            },
            PositionRecord {
                position: [6.0, 7.0, offset],
            },
        ];
        state
            .update_packed_positions(&positions)
            .unwrap_or_else(|error| panic!("{error}"));
    }

    assert_eq!(state.node_capacity_slots(), node_capacity);
    assert_eq!(state.edge_capacity_slots(), edge_capacity);
    assert_eq!(
        [
            state.node_slot(NodeId(101)),
            state.node_slot(NodeId(205)),
            state.node_slot(NodeId(999)),
        ],
        slots
    );
    assert_eq!(state.edge_slot(EdgeId(7)), Some(0));
}

#[test]
fn borrowed_snapshot_projects_exact_ids_without_consuming_authority() {
    let snapshot = GraphSnapshot::new(
        GraphRevision(44),
        vec![node(101), node(205)],
        vec![edge(9001, 101, 205)],
    );
    let mut state = SceneState::default();
    state
        .set_snapshot(&snapshot)
        .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(snapshot.revision, GraphRevision(44));
    assert_eq!(
        snapshot
            .nodes
            .iter()
            .map(|value| value.id)
            .collect::<Vec<_>>(),
        vec![NodeId(101), NodeId(205)]
    );
    assert_eq!(snapshot.edges[0].id, EdgeId(9001));
    assert_eq!(state.node_count(), snapshot.nodes.len());
    assert_eq!(state.edge_count(), snapshot.edges.len());
    assert_eq!(state.node_slot(NodeId(101)), Some(0));
    assert_eq!(state.node_slot(NodeId(205)), Some(1));
    assert_eq!(state.edge_slot(EdgeId(9001)), Some(0));
}

fn model_error(result: Result<graph_render_wgpu::SceneChanges, RenderError>) -> ModelError {
    match result {
        Err(RenderError::Model(error)) => error,
        Err(error) => panic!("unexpected error: {error}"),
        Ok(_) => panic!("expected rejection"),
    }
}

#[test]
fn unchanged_ids_keep_slots_and_removed_slot_is_reused() {
    let mut state = baseline();
    let node_one_slot = state.node_slot(NodeId(1));
    let node_three_slot = state.node_slot(NodeId(3));
    let mut diff = GraphDiff::new(GraphRevision(2));
    diff.removed_edges.push(EdgeId(11));
    diff.removed_nodes.push(NodeId(3));
    diff.added_nodes.push(node(4));
    diff.added_edges.push(edge(12, 2, 4));
    let changes = state
        .apply_diff(diff)
        .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(state.node_slot(NodeId(1)), node_one_slot);
    assert_eq!(state.node_slot(NodeId(4)), node_three_slot);
    assert_eq!(state.node_slot(NodeId(3)), None);
    assert_eq!(state.edge_slot(EdgeId(11)), None);
    assert_eq!(changes.nodes_added, 1);
    assert_eq!(changes.nodes_removed, 1);
}

#[test]
fn node_removal_requires_explicit_incident_edge_action_and_is_atomic() {
    let mut state = baseline();
    let mut diff = GraphDiff::new(GraphRevision(2));
    diff.removed_nodes.push(NodeId(2));
    let error = model_error(state.apply_diff(diff));
    assert_eq!(
        error,
        ModelError::IncidentEdgeNotRemoved {
            node_id: NodeId(2),
            edge_id: EdgeId(10),
        }
    );
    assert_eq!(state.revision(), Some(GraphRevision(1)));
    assert_eq!(state.node_count(), 3);
    assert_eq!(state.edge_count(), 2);
}

#[test]
fn updated_edge_can_move_away_from_removed_node() {
    let mut state = baseline();
    let mut diff = GraphDiff::new(GraphRevision(2));
    diff.removed_nodes.push(NodeId(3));
    diff.updated_edges.push(edge(11, 2, 1));
    state
        .apply_diff(diff)
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(state.node_slot(NodeId(3)), None);
    assert_eq!(
        state
            .edge_at_slot(state.edge_slot(EdgeId(11)).unwrap_or_default())
            .map(|value| value.target),
        Some(NodeId(1))
    );
}

#[test]
fn conflicting_mutation_lanes_are_rejected() {
    let mut state = baseline();
    let mut diff = GraphDiff::new(GraphRevision(2));
    diff.updated_nodes.push(node(1));
    diff.removed_nodes.push(NodeId(1));
    assert_eq!(
        model_error(state.apply_diff(diff)),
        ModelError::ConflictingNodeMutation(NodeId(1))
    );
}

#[test]
fn stale_and_unknown_updates_are_rejected() {
    let mut state = baseline();
    let stale = GraphDiff::new(GraphRevision(1));
    assert_eq!(
        model_error(state.apply_diff(stale)),
        ModelError::StaleRevision {
            current: GraphRevision(1),
            incoming: GraphRevision(1),
        }
    );

    let mut unknown = GraphDiff::new(GraphRevision(2));
    unknown.updated_edges.push(edge(99, 1, 2));
    assert_eq!(
        model_error(state.apply_diff(unknown)),
        ModelError::EdgeNotFound(EdgeId(99))
    );
}

#[test]
fn newer_snapshot_replaces_state_but_stale_snapshot_does_not() {
    let mut state = baseline();
    let replacement = GraphSnapshot::new(GraphRevision(2), vec![node(8)], Vec::new());
    state
        .set_snapshot(&replacement)
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(state.node_count(), 1);

    let stale = GraphSnapshot::new(GraphRevision(2), vec![node(9)], Vec::new());
    assert!(matches!(
        state.set_snapshot(&stale),
        Err(RenderError::Model(ModelError::StaleRevision { .. }))
    ));
    assert_eq!(state.node_slot(NodeId(8)), Some(0));
}
