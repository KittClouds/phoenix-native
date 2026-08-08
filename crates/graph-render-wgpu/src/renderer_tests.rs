use crate::renderer::{
    graph_view_change_requires_fit, preferred_present_mode, validate_view_authority,
};
use graph_model::GraphRevision;
use phoenix_scene_contract::{FamilyMask, GraphGeneration, GraphViewState, Manifold};

#[test]
fn mailbox_is_the_low_latency_first_choice() {
    assert_eq!(
        preferred_present_mode(&[
            wgpu::PresentMode::Fifo,
            wgpu::PresentMode::Immediate,
            wgpu::PresentMode::Mailbox,
        ]),
        Some(wgpu::PresentMode::Mailbox)
    );
}

#[test]
fn fifo_remains_the_portable_fail_closed_floor() {
    assert_eq!(
        preferred_present_mode(&[wgpu::PresentMode::Fifo]),
        Some(wgpu::PresentMode::Fifo)
    );
    assert_eq!(preferred_present_mode(&[]), None);
}

#[test]
fn graph_view_authority_fails_closed_on_every_mismatch() {
    let mut view = GraphViewState::for_archive(GraphGeneration(8), [3; 32], Some([4; 32]));
    assert!(
        validate_view_authority(view, Some(GraphRevision(8)), Some([3; 32]), Some([4; 32])).is_ok()
    );
    assert!(
        validate_view_authority(view, Some(GraphRevision(9)), Some([3; 32]), Some([4; 32]))
            .is_err()
    );
    assert!(
        validate_view_authority(view, Some(GraphRevision(8)), Some([5; 32]), Some([4; 32]))
            .is_err()
    );
    assert!(
        validate_view_authority(view, Some(GraphRevision(8)), Some([3; 32]), Some([6; 32]))
            .is_err()
    );
    view = GraphViewState::for_archive(GraphGeneration(8), [3; 32], None);
    assert!(validate_view_authority(view, Some(GraphRevision(8)), Some([3; 32]), None).is_ok());
}

#[test]
fn visibility_only_view_changes_preserve_camera_framing() {
    let current = GraphViewState::default();
    let mut next = current;
    next.families = FamilyMask::ENTITIES;
    next.topology_families = FamilyMask::EVENT_FACTS;
    assert!(!graph_view_change_requires_fit(current, next));

    next.manifold = Manifold::Caps;
    assert!(graph_view_change_requires_fit(current, next));
}

#[test]
fn v3_screen_space_node_and_pick_contracts_are_locked() {
    let nodes = include_str!("../shaders/nodes.wgsl");
    let picking = include_str!("../shaders/picking.wgsl");

    assert!(nodes.contains("const NODE_SCREEN_SCALE: f32 = 1.3662"));
    assert!(nodes.contains("const NODE_DIAMETER_SCALE: f32 = 2.02"));
    assert!(nodes.contains("fn visual_role(flags: u32) -> u32"));
    assert!(nodes.contains("role_scale(role)"));
    assert!(nodes.contains("role_aura_strength(role)"));
    assert!(nodes.contains("* NODE_SCREEN_SCALE"));
    assert!(nodes.contains("let sphere_normal = normalize(vec3<f32>(sphere_xy, sphere_z))"));
    assert!(nodes.contains("let specular = pow(max(dot(sphere_normal, half_direction), 0.0)"));
    assert!(nodes.contains("view_depth * 0.8284271 / max(camera.viewport_size.y, 1.0)"));
    assert!(picking.contains("const NODE_SCREEN_SCALE: f32 = 1.3662"));
    assert!(picking.contains("const NODE_DIAMETER_SCALE: f32 = 2.02"));
    assert!(picking.contains("fn visual_role(flags: u32) -> u32"));
    assert!(picking.contains("role_scale(visual_role(flags))"));
    assert!(picking.contains("* NODE_SCREEN_SCALE"));
    assert!(picking.contains("clamp(visual_diameter * 0.7 + 6.0, 7.0, 18.0)"));
}

#[test]
fn every_edge_path_uses_the_stable_width_and_density_contract() {
    let edges = include_str!("../shaders/edges.wgsl");
    let paths = include_str!("../shaders/paths.wgsl");

    for shader in [edges, paths] {
        assert!(shader.contains("const BASE_NODE_DIAMETER_PX: f32 = 2.0493"));
        assert!(shader.contains("const EDGE_WIDTH_SCALE: f32 = 0.90"));
        assert!(shader.contains("const MIN_EDGE_WIDTH_PX: f32 = 0.64"));
        assert!(shader.contains("const MAX_EDGE_TO_BASE_NODE_RATIO: f32 = 0.45"));
        assert!(shader.contains("BASE_NODE_DIAMETER_PX * MAX_EDGE_TO_BASE_NODE_RATIO"));
        assert!(shader.contains("color.a = min(color.a, camera.edge_opacity)"));
    }
}
