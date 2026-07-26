use crate::renderer::{preferred_present_mode, validate_view_authority};
use graph_model::GraphRevision;
use phoenix_scene_contract::{FamilyMask, GraphGeneration, GraphViewState};

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
    view = GraphViewState {
        families: FamilyMask(1),
        ..GraphViewState::for_archive(GraphGeneration(8), [3; 32], None)
    };
    assert!(validate_view_authority(view, Some(GraphRevision(8)), Some([3; 32]), None).is_err());
}
