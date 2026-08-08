mod caps;
mod hopf;
mod hybrid;

pub use caps::{
    layout as compile_caps_positions, layout_with_guides as compile_caps_layout, CapsGuide,
    CapsLayout, CapsNode,
};
pub use hopf::{layout as compile_hopf_positions, HopfNode};
pub use hybrid::{layout as compile_hybrid_positions, HybridNode};
use phoenix_scene_archive::PositionRecord;

/// Projects one stable node into every non-authoritative manifold lane.
///
/// Backend adapters use this only while compiling a packed generation. The
/// renderer never invokes layout code or reconstructs graph products.
#[must_use]
pub fn project_node_positions(
    stable_id: u64,
    ordinal: usize,
    count: usize,
    family_slot: u16,
    degree: u32,
) -> [PositionRecord; 6] {
    positions(stable_id, ordinal, count, family_slot, degree)
}

pub(crate) fn positions(
    stable_id: u64,
    ordinal: usize,
    count: usize,
    family_slot: u16,
    degree: u32,
) -> [PositionRecord; 6] {
    let digest = stable_digest(stable_id);
    let jitter = |offset: usize| unit(&digest[offset..offset + 4]);
    let count = count.max(1) as f32;
    let ordinal = ordinal as f32;
    let phase = std::f32::consts::TAU * (ordinal / count);
    // Hybrid is a batch containment layout. The compiler overwrites this
    // reserved slot after every explicit parent relationship is known.
    let hybrid = [0.0, 0.0, 0.0];

    let torus_minor = 5.0 + family_slot as f32 * 0.35;
    let torus_major = 15.0 + (degree as f32 + 1.0).ln();
    let torus_theta = phase + jitter(0) * 0.25;
    let torus_phi = phase * 2.0 + jitter(4) * std::f32::consts::PI;
    let torus = [
        (torus_major + torus_minor * torus_phi.cos()) * torus_theta.cos(),
        torus_minor * torus_phi.sin(),
        (torus_major + torus_minor * torus_phi.cos()) * torus_theta.sin(),
    ];

    // CAPS is a batch hierarchy layout. The compiler overwrites this reserved
    // slot after every node and explicit parent relationship are known.
    let caps = [0.0; 3];

    let transit_lane = family_slot as f32 - 3.5;
    let transit = [
        (ordinal - count * 0.5) * 1.25,
        transit_lane * 5.0,
        jitter(8) * 2.0 + degree as f32 * 0.08,
    ];

    let spiral = (ordinal + 1.0).sqrt() * 2.1;
    let siegel = [
        phase.cos() * spiral,
        phase.sin() * spiral,
        (degree as f32 + 1.0).ln() * 2.0 + jitter(8),
    ];

    // Hopf is a hierarchy batch layout. The compiler overwrites this reserved
    // slot after authoritative parents and sibling ranks are complete.
    let hopf = [0.0; 3];

    [hybrid, torus, caps, transit, siegel, hopf].map(|position| PositionRecord { position })
}

fn stable_digest(stable_id: u64) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"phoenix.native.scene-layout/v1\0");
    hasher.update(&stable_id.to_le_bytes());
    *hasher.finalize().as_bytes()
}

fn unit(bytes: &[u8]) -> f32 {
    let mut encoded = [0_u8; 4];
    encoded.copy_from_slice(bytes);
    (u32::from_le_bytes(encoded) as f64 / u32::MAX as f64 * 2.0 - 1.0) as f32
}
