mod caps;

pub use caps::{layout as compile_caps_positions, CapsNode};
use phoenix_scene_archive::PositionRecord;

pub(crate) fn positions(
    stable_id: u64,
    ordinal: usize,
    count: usize,
    family_slot: u16,
    degree: u32,
) -> [PositionRecord; 5] {
    let digest = stable_digest(stable_id);
    let jitter = |offset: usize| unit(&digest[offset..offset + 4]);
    let count = count.max(1) as f32;
    let ordinal = ordinal as f32;
    let phase = std::f32::consts::TAU * (ordinal / count);
    let elevation = ((ordinal + 0.5) / count * 2.0 - 1.0).clamp(-1.0, 1.0);
    let radial = (1.0 - elevation * elevation).sqrt();
    let degree_scale = 18.0 + (degree as f32 + 1.0).ln() * 3.0;

    let hybrid = [
        phase.cos() * radial * degree_scale + jitter(0) * 1.5,
        elevation * degree_scale + jitter(4) * 1.5,
        phase.sin() * radial * degree_scale + jitter(8) * 1.5,
    ];

    let hopf_minor = 5.0 + family_slot as f32 * 0.35;
    let hopf_major = 15.0 + (degree as f32 + 1.0).ln();
    let hopf_theta = phase + jitter(0) * 0.25;
    let hopf_phi = phase * 2.0 + jitter(4) * std::f32::consts::PI;
    let hopf = [
        (hopf_major + hopf_minor * hopf_phi.cos()) * hopf_theta.cos(),
        hopf_minor * hopf_phi.sin(),
        (hopf_major + hopf_minor * hopf_phi.cos()) * hopf_theta.sin(),
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

    [hybrid, hopf, caps, transit, siegel].map(|position| PositionRecord { position })
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
