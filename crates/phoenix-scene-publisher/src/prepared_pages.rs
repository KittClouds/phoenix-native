use crate::{NativeScenePublication, SceneCapsGuide, ScenePublicationError};
use glam::Vec3;
use hashbrown::HashMap;
use phoenix_scene_archive::{
    ArchiveManifold, GuidePageHeader, GuideStrokeRecord, LabelPriorityRecord, PageKey, PageKind,
    PaletteEntryRecord, PathPageHeader, PathRecord, PhoenixSceneArchiveBuilderV1, PositionRecord,
    RelationMaskRecord,
};
use phoenix_scene_contract::{
    visual_role, CapsRole, CHUNK_NODE_KIND, EPISODE_NODE_KIND, GUIDE_FLAG_CAP_BOUNDARY,
    GUIDE_FLAG_CONCENTRATION_AXIS, GUIDE_FLAG_HOPF_BASE_LINK, GUIDE_FLAG_HOPF_BASE_SPHERE,
    GUIDE_FLAG_HOPF_FIBER, GUIDE_FLAG_SHELL,
};
use std::f32::consts::TAU;

const DEFAULT_GUIDE_STROKES: usize = 7;
const GUIDE_POINTS: usize = 65;
const MAX_CAP_BOUNDARIES: usize = 96;
const CAPS_REFERENCE_ROLES: [CapsRole; 5] = [
    CapsRole::Document,
    CapsRole::Episode,
    CapsRole::Fact,
    CapsRole::Discourse,
    CapsRole::Memory,
];

struct PreparedGuidePage {
    bytes: Vec<u8>,
    stroke_count: usize,
}

pub(super) fn add_prepared_pages(
    archive: &mut PhoenixSceneArchiveBuilderV1,
    publication: &NativeScenePublication,
) -> Result<(), ScenePublicationError> {
    let labels = label_priorities(publication)?;
    archive.add_records(PageKey::shared(PageKind::LabelPriority), &labels)?;

    let relations = publication
        .edge_products
        .iter()
        .map(|edge| RelationMaskRecord {
            visible_mask: edge.relation_mask,
            relation_kind: edge.relation_mask.trailing_zeros().min(u32::from(u16::MAX)) as u16,
            flags: 0,
            reserved: 0,
        })
        .collect::<Vec<_>>();
    archive.add_records(PageKey::shared(PageKind::RelationMasks), &relations)?;

    let palette = palette_entries(publication);
    archive.add_records(PageKey::shared(PageKind::PalettePolicy), &palette)?;

    let slots = node_slots(publication)?;
    for (manifold, positions) in ArchiveManifold::ALL.into_iter().zip(&publication.positions) {
        let guides = guide_page(
            manifold,
            positions,
            &publication.styles,
            &publication.caps_guides,
        )?;
        archive.add_page(
            PageKey::manifold(PageKind::Guides, manifold),
            guides.bytes,
            guides.stroke_count as u64,
            std::mem::size_of::<GuideStrokeRecord>() as u32,
        )?;
        for (kind, style) in [
            (PageKind::StraightPaths, 0),
            (PageKind::CurvedPaths, 1),
            (PageKind::BundledPaths, 2),
        ] {
            let bytes = path_page(publication, positions, &slots, style)?;
            archive.add_page(
                PageKey::manifold(kind, manifold),
                bytes,
                publication.edges.len() as u64,
                std::mem::size_of::<PathRecord>() as u32,
            )?;
        }
    }
    Ok(())
}

fn label_priorities(
    publication: &NativeScenePublication,
) -> Result<Vec<LabelPriorityRecord>, ScenePublicationError> {
    let mut slots = (0..publication.styles.len()).collect::<Vec<_>>();
    slots.sort_unstable_by(|left, right| {
        visual_role(publication.styles[*right].flags)
            .priority()
            .cmp(&visual_role(publication.styles[*left].flags).priority())
            .then_with(|| {
                publication.styles[*right]
                    .radius
                    .total_cmp(&publication.styles[*left].radius)
            })
            .then_with(|| {
                publication.identities[*left]
                    .id
                    .cmp(&publication.identities[*right].id)
            })
    });
    slots
        .into_iter()
        .enumerate()
        .map(|(rank, slot)| {
            Ok(LabelPriorityRecord {
                node_slot: u32::try_from(slot)
                    .map_err(|_| ScenePublicationError::InventoryMismatch("label slot"))?,
                rank: u32::try_from(rank)
                    .map_err(|_| ScenePublicationError::InventoryMismatch("label rank"))?,
            })
        })
        .collect()
}

fn palette_entries(publication: &NativeScenePublication) -> Vec<PaletteEntryRecord> {
    let mut by_kind = HashMap::<u16, PaletteEntryRecord>::new();
    for style in &publication.styles {
        by_kind.entry(style.kind).or_insert(PaletteEntryRecord {
            rgba8: pack_rgba8(style.color),
            kind: style.kind,
            flags: 0,
        });
    }
    let mut entries = by_kind.into_values().collect::<Vec<_>>();
    entries.sort_unstable_by_key(|entry| entry.kind);
    entries
}

fn node_slots(
    publication: &NativeScenePublication,
) -> Result<HashMap<u64, usize>, ScenePublicationError> {
    let mut slots = HashMap::with_capacity(publication.identities.len());
    for (slot, identity) in publication.identities.iter().enumerate() {
        if slots.insert(identity.id, slot).is_some() {
            return Err(ScenePublicationError::IdentityMismatch {
                resource: "prepared node",
                slot,
            });
        }
    }
    Ok(slots)
}

fn guide_page(
    manifold: ArchiveManifold,
    positions: &[PositionRecord],
    styles: &[phoenix_scene_archive::NodeStyleRecord],
    caps_guides: &[SceneCapsGuide],
) -> Result<PreparedGuidePage, ScenePublicationError> {
    if manifold == ArchiveManifold::Hybrid {
        return hybrid_guide_page();
    }
    if manifold == ArchiveManifold::Caps {
        return caps_guide_page(positions, styles, caps_guides);
    }
    if manifold == ArchiveManifold::Hopf {
        return hopf_guide_page(positions.len());
    }
    let radius = positions
        .iter()
        .map(|point| {
            point.position[0]
                .hypot(point.position[1])
                .hypot(point.position[2])
        })
        .fold(12.0_f32, f32::max)
        .max(12.0);
    let mut strokes = Vec::with_capacity(DEFAULT_GUIDE_STROKES);
    let mut points = Vec::with_capacity(DEFAULT_GUIDE_STROKES * GUIDE_POINTS);
    for stroke in 0..DEFAULT_GUIDE_STROKES {
        let first_point = checked_u32(points.len(), "guide point offset")?;
        let ring = radius * (0.28 + stroke as f32 * 0.12);
        let phase = manifold as u8 as f32 * 0.17;
        for point in 0..GUIDE_POINTS {
            let angle = point as f32 * TAU / (GUIDE_POINTS - 1) as f32 + phase;
            let (x, y, z) = guide_position(manifold, ring, angle, stroke);
            points.push(PositionRecord {
                position: [x, y, z],
            });
        }
        strokes.push(GuideStrokeRecord {
            first_point,
            point_count: GUIDE_POINTS as u32,
            rgba8: guide_color(stroke),
            flags: 0,
        });
    }
    let bytes = variable_page(
        &GuidePageHeader {
            stroke_count: DEFAULT_GUIDE_STROKES as u32,
            point_count: checked_u32(points.len(), "guide point count")?,
            reserved: [0; 2],
        },
        &strokes,
        &points,
    )?;
    Ok(PreparedGuidePage {
        bytes,
        stroke_count: strokes.len(),
    })
}

fn hybrid_guide_page() -> Result<PreparedGuidePage, ScenePublicationError> {
    const TAU_LEVELS: [f32; 4] = [-3.0, -2.0, -1.0, 0.0];
    const RINGS_PER_HOROSPHERE: usize = 2;
    let horosphere_strokes =
        phoenix_hybrid_space::HybridLane::ALL.len() * TAU_LEVELS.len() * RINGS_PER_HOROSPHERE;
    let mut strokes = Vec::with_capacity(3 + horosphere_strokes);
    let mut points = Vec::with_capacity((3 + horosphere_strokes) * GUIDE_POINTS);

    for plane in 0..3 {
        append_hybrid_shell(&mut strokes, &mut points, plane)?;
    }
    for lane in phoenix_hybrid_space::HybridLane::ALL {
        for tau in TAU_LEVELS {
            let sphere = phoenix_hybrid_space::horosphere(lane, tau);
            for plane in 0..RINGS_PER_HOROSPHERE {
                append_horosphere_ring(&mut strokes, &mut points, sphere, plane)?;
            }
        }
    }

    let bytes = variable_page(
        &GuidePageHeader {
            stroke_count: checked_u32(strokes.len(), "Hybrid guide stroke count")?,
            point_count: checked_u32(points.len(), "Hybrid guide point count")?,
            reserved: [0; 2],
        },
        &strokes,
        &points,
    )?;
    Ok(PreparedGuidePage {
        bytes,
        stroke_count: strokes.len(),
    })
}

fn hopf_guide_page(node_count: usize) -> Result<PreparedGuidePage, ScenePublicationError> {
    const SPHERE_PLANES: usize = 3;
    const LATITUDE_RINGS: usize = 5;
    let fibers = phoenix_hopf_space::fiber_count(node_count);
    let stroke_count = SPHERE_PLANES + LATITUDE_RINGS + fibers * 2;
    let point_count = (SPHERE_PLANES + LATITUDE_RINGS + fibers)
        .checked_mul(GUIDE_POINTS)
        .and_then(|count| count.checked_add(fibers * 2))
        .ok_or(ScenePublicationError::InventoryMismatch(
            "Hopf guide point count",
        ))?;
    let mut strokes = Vec::with_capacity(stroke_count);
    let mut points = Vec::with_capacity(point_count);

    for plane in 0..SPHERE_PLANES {
        append_hopf_sphere_plane(&mut strokes, &mut points, plane)?;
    }
    for latitude in [-60.0_f32, -30.0, 0.0, 30.0, 60.0] {
        append_hopf_latitude(&mut strokes, &mut points, latitude.to_radians())?;
    }
    for fiber in 0..fibers {
        append_hopf_fiber(&mut strokes, &mut points, fiber, fibers)?;
        append_hopf_base_link(&mut strokes, &mut points, fiber, fibers)?;
    }

    let bytes = variable_page(
        &GuidePageHeader {
            stroke_count: checked_u32(strokes.len(), "Hopf guide stroke count")?,
            point_count: checked_u32(points.len(), "Hopf guide point count")?,
            reserved: [0; 2],
        },
        &strokes,
        &points,
    )?;
    Ok(PreparedGuidePage {
        bytes,
        stroke_count: strokes.len(),
    })
}

fn append_hopf_sphere_plane(
    strokes: &mut Vec<GuideStrokeRecord>,
    points: &mut Vec<PositionRecord>,
    plane: usize,
) -> Result<(), ScenePublicationError> {
    let first_point = checked_u32(points.len(), "Hopf sphere plane offset")?;
    let radius = phoenix_hopf_space::HOPF_BASE_SPHERE_RADIUS;
    for point in 0..GUIDE_POINTS {
        let angle = point as f32 * TAU / (GUIDE_POINTS - 1) as f32;
        let (sine, cosine) = angle.sin_cos();
        let position = match plane {
            0 => [cosine * radius, sine * radius, 0.0],
            1 => [cosine * radius, 0.0, sine * radius],
            _ => [0.0, cosine * radius, sine * radius],
        };
        points.push(PositionRecord { position });
    }
    strokes.push(GuideStrokeRecord {
        first_point,
        point_count: GUIDE_POINTS as u32,
        rgba8: pack_rgba8([0.26, 0.64, 0.92, 0.18]),
        flags: GUIDE_FLAG_HOPF_BASE_SPHERE,
    });
    Ok(())
}

fn append_hopf_latitude(
    strokes: &mut Vec<GuideStrokeRecord>,
    points: &mut Vec<PositionRecord>,
    latitude: f32,
) -> Result<(), ScenePublicationError> {
    let first_point = checked_u32(points.len(), "Hopf sphere latitude offset")?;
    for point in 0..GUIDE_POINTS {
        let longitude = point as f32 * TAU / (GUIDE_POINTS - 1) as f32;
        points.push(PositionRecord {
            position: phoenix_hopf_space::sphere_point(latitude, longitude),
        });
    }
    strokes.push(GuideStrokeRecord {
        first_point,
        point_count: GUIDE_POINTS as u32,
        rgba8: pack_rgba8([0.20, 0.52, 0.82, 0.11]),
        flags: GUIDE_FLAG_HOPF_BASE_SPHERE,
    });
    Ok(())
}

fn append_hopf_fiber(
    strokes: &mut Vec<GuideStrokeRecord>,
    points: &mut Vec<PositionRecord>,
    fiber: usize,
    fiber_count: usize,
) -> Result<(), ScenePublicationError> {
    let first_point = checked_u32(points.len(), "Hopf fiber offset")?;
    let base = phoenix_hopf_space::base_direction(fiber, fiber_count);
    for point in 0..GUIDE_POINTS {
        let phase = point as f32 * TAU / (GUIDE_POINTS - 1) as f32;
        points.push(PositionRecord {
            position: phoenix_hopf_space::fiber_point(base, phase),
        });
    }
    strokes.push(GuideStrokeRecord {
        first_point,
        point_count: GUIDE_POINTS as u32,
        rgba8: hopf_fiber_color(fiber),
        flags: GUIDE_FLAG_HOPF_FIBER,
    });
    Ok(())
}

fn append_hopf_base_link(
    strokes: &mut Vec<GuideStrokeRecord>,
    points: &mut Vec<PositionRecord>,
    fiber: usize,
    fiber_count: usize,
) -> Result<(), ScenePublicationError> {
    let first_point = checked_u32(points.len(), "Hopf base link offset")?;
    let base = phoenix_hopf_space::base_direction(fiber, fiber_count);
    let radius = phoenix_hopf_space::HOPF_BASE_SPHERE_RADIUS;
    points.push(PositionRecord {
        position: [base[0] * radius, base[2] * radius, base[1] * radius],
    });
    points.push(PositionRecord {
        position: phoenix_hopf_space::fiber_point(base, 0.0),
    });
    strokes.push(GuideStrokeRecord {
        first_point,
        point_count: 2,
        rgba8: pack_rgba8([0.44, 0.76, 0.96, 0.13]),
        flags: GUIDE_FLAG_HOPF_BASE_LINK,
    });
    Ok(())
}

fn hopf_fiber_color(fiber: usize) -> u32 {
    const COLORS: [[f32; 3]; 8] = [
        [0.20, 0.82, 0.96],
        [0.18, 0.88, 0.68],
        [0.54, 0.36, 0.96],
        [0.96, 0.32, 0.66],
        [0.98, 0.64, 0.12],
        [0.92, 0.88, 0.18],
        [0.28, 0.54, 0.98],
        [0.72, 0.42, 0.92],
    ];
    let color = COLORS[fiber % COLORS.len()];
    pack_rgba8([color[0], color[1], color[2], 0.27])
}

fn append_hybrid_shell(
    strokes: &mut Vec<GuideStrokeRecord>,
    points: &mut Vec<PositionRecord>,
    plane: usize,
) -> Result<(), ScenePublicationError> {
    let first_point = checked_u32(points.len(), "Hybrid shell offset")?;
    let radius = phoenix_hybrid_space::HYBRID_WORLD_RADIUS;
    for point in 0..GUIDE_POINTS {
        let angle = point as f32 * TAU / (GUIDE_POINTS - 1) as f32;
        let (sine, cosine) = angle.sin_cos();
        let position = match plane {
            0 => [cosine * radius, sine * radius, 0.0],
            1 => [cosine * radius, 0.0, sine * radius],
            _ => [0.0, cosine * radius, sine * radius],
        };
        points.push(PositionRecord { position });
    }
    strokes.push(GuideStrokeRecord {
        first_point,
        point_count: GUIDE_POINTS as u32,
        rgba8: pack_rgba8([0.22, 0.58, 0.66, 0.055]),
        flags: GUIDE_FLAG_SHELL,
    });
    Ok(())
}

fn append_horosphere_ring(
    strokes: &mut Vec<GuideStrokeRecord>,
    points: &mut Vec<PositionRecord>,
    sphere: phoenix_hybrid_space::Horosphere,
    plane: usize,
) -> Result<(), ScenePublicationError> {
    let first_point = checked_u32(points.len(), "Busemann horosphere offset")?;
    let center = Vec3::from_array(sphere.center);
    let prototype = sphere.lane.prototype();
    let (tangent_u, tangent_v) = tangent_basis(prototype);
    let (axis_u, axis_v) = if plane == 0 {
        (tangent_u, tangent_v)
    } else {
        (prototype, tangent_u)
    };
    for point in 0..GUIDE_POINTS {
        let angle = point as f32 * TAU / (GUIDE_POINTS - 1) as f32;
        let position = center + (axis_u * angle.cos() + axis_v * angle.sin()) * sphere.radius;
        points.push(PositionRecord {
            position: position.to_array(),
        });
    }
    strokes.push(GuideStrokeRecord {
        first_point,
        point_count: GUIDE_POINTS as u32,
        rgba8: hybrid_horosphere_color(sphere.lane, sphere.tau),
        flags: GUIDE_FLAG_CAP_BOUNDARY,
    });
    Ok(())
}

fn hybrid_horosphere_color(lane: phoenix_hybrid_space::HybridLane, tau: f32) -> u32 {
    let alpha = 0.040 + (tau.abs().min(3.0) * 0.008);
    let rgb = match lane {
        phoenix_hybrid_space::HybridLane::Structure => [0.98, 0.70, 0.08],
        phoenix_hybrid_space::HybridLane::Facts => [0.96, 0.34, 0.62],
        phoenix_hybrid_space::HybridLane::Discourse => [0.42, 0.82, 0.98],
        phoenix_hybrid_space::HybridLane::Entities => [0.12, 0.86, 0.62],
    };
    pack_rgba8([rgb[0], rgb[1], rgb[2], alpha])
}

fn caps_guide_page(
    positions: &[PositionRecord],
    styles: &[phoenix_scene_archive::NodeStyleRecord],
    caps_guides: &[SceneCapsGuide],
) -> Result<PreparedGuidePage, ScenePublicationError> {
    if positions.len() != styles.len() {
        return Err(ScenePublicationError::InventoryMismatch(
            "CAPS guide node pages",
        ));
    }
    let shell_strokes = CAPS_REFERENCE_ROLES.len() * 3;
    let cap_count = if caps_guides.is_empty() {
        styles
            .iter()
            .filter(|style| style.kind == CHUNK_NODE_KIND)
            .count()
            .min(MAX_CAP_BOUNDARIES)
    } else {
        caps_guides.len().min(MAX_CAP_BOUNDARIES)
    };
    let mut strokes = Vec::with_capacity(shell_strokes + cap_count + 1);
    let mut points = Vec::with_capacity((shell_strokes + cap_count) * GUIDE_POINTS + 2);

    for role in CAPS_REFERENCE_ROLES {
        for plane in 0..3 {
            append_caps_shell(&mut strokes, &mut points, role, plane)?;
        }
    }

    if caps_guides.is_empty() {
        for (position, _) in positions
            .iter()
            .zip(styles)
            .filter(|(_, style)| style.kind == CHUNK_NODE_KIND)
            .take(MAX_CAP_BOUNDARIES)
        {
            append_cap_boundary(
                &mut strokes,
                &mut points,
                Vec3::from_array(position.position).normalize_or_zero(),
                CapsRole::Chunk.world_radius(),
                0.32,
                CapsRole::Chunk,
                1,
            )?;
        }
    } else {
        for guide in caps_guides.iter().take(MAX_CAP_BOUNDARIES) {
            append_cap_boundary(
                &mut strokes,
                &mut points,
                Vec3::from_array(guide.center),
                guide.radius,
                guide.aperture,
                guide.role,
                guide.weight,
            )?;
        }
    }

    let concentration_axis = caps_guides.first().map_or_else(
        || {
            positions
                .iter()
                .zip(styles)
                .find(|(_, style)| style.kind == EPISODE_NODE_KIND)
                .map_or(Vec3::new(0.24, 0.31, 0.92).normalize(), |(position, _)| {
                    Vec3::from_array(position.position).normalize_or_zero()
                })
        },
        |guide| Vec3::from_array(guide.center),
    );
    append_concentration_axis(&mut strokes, &mut points, concentration_axis)?;

    let bytes = variable_page(
        &GuidePageHeader {
            stroke_count: checked_u32(strokes.len(), "CAPS guide stroke count")?,
            point_count: checked_u32(points.len(), "CAPS guide point count")?,
            reserved: [0; 2],
        },
        &strokes,
        &points,
    )?;
    Ok(PreparedGuidePage {
        bytes,
        stroke_count: strokes.len(),
    })
}

fn append_caps_shell(
    strokes: &mut Vec<GuideStrokeRecord>,
    points: &mut Vec<PositionRecord>,
    role: CapsRole,
    plane: usize,
) -> Result<(), ScenePublicationError> {
    let first_point = checked_u32(points.len(), "CAPS shell offset")?;
    let radius = role.world_radius();
    for point in 0..GUIDE_POINTS {
        let angle = point as f32 * TAU / (GUIDE_POINTS - 1) as f32;
        let (sine, cosine) = angle.sin_cos();
        let position = match plane {
            0 => [cosine * radius, sine * radius, 0.0],
            1 => [cosine * radius, 0.0, sine * radius],
            _ => [0.0, cosine * radius, sine * radius],
        };
        points.push(PositionRecord { position });
    }
    strokes.push(GuideStrokeRecord {
        first_point,
        point_count: GUIDE_POINTS as u32,
        rgba8: caps_shell_color(role, plane),
        flags: GUIDE_FLAG_SHELL,
    });
    Ok(())
}

fn append_cap_boundary(
    strokes: &mut Vec<GuideStrokeRecord>,
    points: &mut Vec<PositionRecord>,
    center: Vec3,
    radius: f32,
    aperture: f32,
    role: CapsRole,
    weight: u32,
) -> Result<(), ScenePublicationError> {
    if center.length_squared() < 0.5 {
        return Ok(());
    }
    let first_point = checked_u32(points.len(), "CAPS boundary offset")?;
    let (u, v) = tangent_basis(center);
    for point in 0..GUIDE_POINTS {
        let angle = point as f32 * TAU / (GUIDE_POINTS - 1) as f32;
        let tangent = u * angle.cos() + v * angle.sin();
        let direction = (center * aperture.cos() + tangent * aperture.sin()).normalize_or_zero();
        points.push(PositionRecord {
            position: (direction * radius).to_array(),
        });
    }
    strokes.push(GuideStrokeRecord {
        first_point,
        point_count: GUIDE_POINTS as u32,
        rgba8: caps_boundary_color(role, weight),
        flags: GUIDE_FLAG_CAP_BOUNDARY,
    });
    Ok(())
}

fn append_concentration_axis(
    strokes: &mut Vec<GuideStrokeRecord>,
    points: &mut Vec<PositionRecord>,
    axis: Vec3,
) -> Result<(), ScenePublicationError> {
    let first_point = checked_u32(points.len(), "CAPS concentration axis offset")?;
    let extent = CapsRole::Document.world_radius() * 1.04;
    points.push(PositionRecord {
        position: (-axis * extent).to_array(),
    });
    points.push(PositionRecord {
        position: (axis * extent).to_array(),
    });
    strokes.push(GuideStrokeRecord {
        first_point,
        point_count: 2,
        rgba8: pack_rgba8([0.24, 0.88, 0.70, 0.34]),
        flags: GUIDE_FLAG_CONCENTRATION_AXIS,
    });
    Ok(())
}

fn tangent_basis(direction: Vec3) -> (Vec3, Vec3) {
    let reference = if direction.y.abs() < 0.88 {
        Vec3::Y
    } else {
        Vec3::X
    };
    let u = direction.cross(reference).normalize();
    let v = u.cross(direction).normalize();
    (u, v)
}

fn caps_shell_color(role: CapsRole, plane: usize) -> u32 {
    const COLORS: [[f32; 3]; 12] = [
        [0.92, 0.18, 0.25],
        [0.34, 0.65, 0.95],
        [0.42, 0.76, 0.96],
        [0.50, 0.84, 0.80],
        [0.86, 0.32, 0.65],
        [0.96, 0.60, 0.10],
        [0.34, 0.78, 0.49],
        [0.63, 0.53, 0.92],
        [0.84, 0.36, 0.72],
        [0.72, 0.46, 0.96],
        [0.12, 0.78, 0.64],
        [0.18, 0.58, 0.92],
    ];
    let rgb = COLORS[role as usize];
    let plane_alpha = [0.10, 0.075, 0.055][plane.min(2)];
    pack_rgba8([rgb[0], rgb[1], rgb[2], plane_alpha])
}

fn caps_boundary_color(role: CapsRole, weight: u32) -> u32 {
    const COLORS: [[f32; 3]; 12] = [
        [0.92, 0.18, 0.25],
        [0.34, 0.65, 0.95],
        [0.42, 0.76, 0.96],
        [0.50, 0.84, 0.80],
        [0.86, 0.32, 0.65],
        [0.96, 0.60, 0.10],
        [0.34, 0.78, 0.49],
        [0.63, 0.53, 0.92],
        [0.84, 0.36, 0.72],
        [0.72, 0.46, 0.96],
        [0.12, 0.78, 0.64],
        [0.18, 0.58, 0.92],
    ];
    let rgb = COLORS[role as usize];
    let alpha = 0.12 + (weight.ilog2().min(12) as f32 * 0.012);
    pack_rgba8([rgb[0], rgb[1], rgb[2], alpha])
}

fn guide_position(
    manifold: ArchiveManifold,
    radius: f32,
    angle: f32,
    stroke: usize,
) -> (f32, f32, f32) {
    match manifold {
        ArchiveManifold::Hybrid => (
            angle.cos() * radius,
            angle.sin() * radius,
            (angle * 2.0).sin() * radius * 0.08,
        ),
        ArchiveManifold::Torus => (
            angle.cos() * radius,
            angle.sin() * radius,
            (angle + stroke as f32 * 0.31).sin() * radius * 0.32,
        ),
        ArchiveManifold::Hopf => unreachable!("Hopf uses its exact prepared guide page"),
        ArchiveManifold::Caps => (
            angle.cos() * radius,
            angle.sin() * radius * 0.52,
            (stroke as f32 - 3.0) * radius * 0.12,
        ),
        ArchiveManifold::Transit => (
            (angle.cos() * 0.18 + stroke as f32 - 3.0) * radius * 0.38,
            angle.sin() * radius,
            angle.cos() * radius * 0.12,
        ),
        ArchiveManifold::Siegel => (
            angle.cos() * radius,
            angle.sin() * radius,
            angle.sin() * angle.cos() * radius * 0.28,
        ),
    }
}

fn path_page(
    publication: &NativeScenePublication,
    positions: &[PositionRecord],
    slots: &HashMap<u64, usize>,
    style: u32,
) -> Result<Vec<u8>, ScenePublicationError> {
    let points_per_path = 2 + style as usize;
    let mut paths = Vec::with_capacity(publication.topology.len());
    let mut points = Vec::with_capacity(publication.topology.len() * points_per_path);
    for (edge_slot, edge) in publication.topology.iter().enumerate() {
        let source_slot =
            *slots
                .get(&edge.source_id)
                .ok_or(ScenePublicationError::IdentityMismatch {
                    resource: "prepared edge source",
                    slot: edge_slot,
                })?;
        let target_slot =
            *slots
                .get(&edge.target_id)
                .ok_or(ScenePublicationError::IdentityMismatch {
                    resource: "prepared edge target",
                    slot: edge_slot,
                })?;
        let source = positions[source_slot];
        let target = positions[target_slot];
        let first_point = checked_u32(points.len(), "path point offset")?;
        points.push(source);
        if style == 1 {
            points.push(curve_midpoint(source, target, edge_slot));
        } else if style == 2 {
            points.push(bundle_port(source, source_slot));
            points.push(bundle_port(target, target_slot));
        }
        points.push(target);
        paths.push(PathRecord {
            edge_slot: checked_u32(edge_slot, "path edge slot")?,
            first_point,
            point_count: points_per_path as u16,
            flags: 0,
            rgba8: edge_rgba(publication.edges[edge_slot].color),
        });
    }
    variable_page(
        &PathPageHeader {
            path_count: checked_u32(paths.len(), "path count")?,
            point_count: checked_u32(points.len(), "path point count")?,
            path_style: style,
            reserved: 0,
        },
        &paths,
        &points,
    )
}

fn curve_midpoint(
    source: PositionRecord,
    target: PositionRecord,
    edge_slot: usize,
) -> PositionRecord {
    let delta_x = target.position[0] - source.position[0];
    let delta_y = target.position[1] - source.position[1];
    let length = delta_x.hypot(delta_y).max(1.0);
    let bend = (((edge_slot * 37) % 19) as f32 / 18.0 - 0.5) * length * 0.22;
    PositionRecord {
        position: [
            (source.position[0] + target.position[0]) * 0.5 - delta_y / length * bend,
            (source.position[1] + target.position[1]) * 0.5 + delta_x / length * bend,
            (source.position[2] + target.position[2]) * 0.5 + bend * 0.18,
        ],
    }
}

fn bundle_port(position: PositionRecord, slot: usize) -> PositionRecord {
    let lane = ((slot * 29) % 17) as f32 / 16.0 - 0.5;
    PositionRecord {
        position: [
            position.position[0] * 0.58,
            position.position[1] * 0.58,
            position.position[2] * 0.58 + lane * 5.0,
        ],
    }
}

fn variable_page<H: bytemuck::Pod, R: bytemuck::Pod>(
    header: &H,
    records: &[R],
    points: &[PositionRecord],
) -> Result<Vec<u8>, ScenePublicationError> {
    let capacity = size_of_val(header)
        .checked_add(size_of_val(records))
        .and_then(|value| value.checked_add(size_of_val(points)))
        .ok_or(ScenePublicationError::InventoryMismatch(
            "prepared page size",
        ))?;
    let mut bytes = Vec::with_capacity(capacity);
    bytes.extend_from_slice(bytemuck::bytes_of(header));
    bytes.extend_from_slice(bytemuck::cast_slice(records));
    bytes.extend_from_slice(bytemuck::cast_slice(points));
    Ok(bytes)
}

fn checked_u32(value: usize, resource: &'static str) -> Result<u32, ScenePublicationError> {
    u32::try_from(value).map_err(|_| ScenePublicationError::InventoryMismatch(resource))
}

fn pack_rgba8(color: [f32; 4]) -> u32 {
    color
        .into_iter()
        .enumerate()
        .fold(0_u32, |packed, (shift, channel)| {
            packed | ((channel.clamp(0.0, 1.0) * 255.0).round() as u32) << (shift * 8)
        })
}

fn edge_rgba(mut color: [f32; 4]) -> u32 {
    color[3] = color[3].clamp(0.12, 0.58);
    pack_rgba8(color)
}

fn guide_color(stroke: usize) -> u32 {
    const COLORS: [u32; DEFAULT_GUIDE_STROKES] = [
        0x282E_7550,
        0x2435_AA72,
        0x213E_C5A4,
        0x1F4D_D8C7,
        0x213E_C5A4,
        0x2435_AA72,
        0x282E_7550,
    ];
    COLORS[stroke]
}

#[cfg(test)]
mod tests {
    use super::*;
    use phoenix_scene_archive::NodeStyleRecord;

    #[test]
    fn hybrid_guides_encode_shell_and_busemann_horospheres() {
        let page = hybrid_guide_page().unwrap_or_else(|error| panic!("Hybrid guides: {error}"));
        assert!(page.bytes.len() <= 256 * 1024);
        let header = bytemuck::pod_read_unaligned::<GuidePageHeader>(
            &page.bytes[..std::mem::size_of::<GuidePageHeader>()],
        );
        let expected_horospheres = phoenix_hybrid_space::HybridLane::ALL.len() * 4 * 2;
        assert_eq!(header.stroke_count as usize, 3 + expected_horospheres);
        let record_start = std::mem::size_of::<GuidePageHeader>();
        let record_end =
            record_start + header.stroke_count as usize * std::mem::size_of::<GuideStrokeRecord>();
        let records = page.bytes[record_start..record_end]
            .chunks_exact(std::mem::size_of::<GuideStrokeRecord>())
            .map(bytemuck::pod_read_unaligned::<GuideStrokeRecord>)
            .collect::<Vec<_>>();
        assert_eq!(
            records
                .iter()
                .filter(|record| record.flags == GUIDE_FLAG_SHELL)
                .count(),
            3
        );
        assert_eq!(
            records
                .iter()
                .filter(|record| record.flags == GUIDE_FLAG_CAP_BOUNDARY)
                .count(),
            expected_horospheres
        );
    }

    #[test]
    fn hopf_guides_bind_exact_fibers_to_a_visible_base_sphere() {
        let page = hopf_guide_page(6_867).unwrap_or_else(|error| panic!("Hopf guides: {error}"));
        assert!(page.bytes.len() <= 128 * 1024);
        let header = bytemuck::pod_read_unaligned::<GuidePageHeader>(
            &page.bytes[..std::mem::size_of::<GuidePageHeader>()],
        );
        let fibers = phoenix_hopf_space::MAX_HOPF_FIBERS;
        assert_eq!(header.stroke_count as usize, 3 + 5 + fibers * 2);
        let record_start = std::mem::size_of::<GuidePageHeader>();
        let record_end =
            record_start + header.stroke_count as usize * std::mem::size_of::<GuideStrokeRecord>();
        let records = page.bytes[record_start..record_end]
            .chunks_exact(std::mem::size_of::<GuideStrokeRecord>())
            .map(bytemuck::pod_read_unaligned::<GuideStrokeRecord>)
            .collect::<Vec<_>>();
        assert_eq!(
            records
                .iter()
                .filter(|record| record.flags == GUIDE_FLAG_HOPF_BASE_SPHERE)
                .count(),
            8
        );
        assert_eq!(
            records
                .iter()
                .filter(|record| record.flags == GUIDE_FLAG_HOPF_FIBER)
                .count(),
            fibers
        );
        assert_eq!(
            records
                .iter()
                .filter(|record| record.flags == GUIDE_FLAG_HOPF_BASE_LINK)
                .count(),
            fibers
        );
    }

    #[test]
    fn caps_guides_encode_orthogonal_shells_caps_and_axis_without_schema_growth() {
        let positions = [
            position(Vec3::new(0.0, 0.0, CapsRole::Episode.world_radius())),
            position(Vec3::new(CapsRole::Chunk.world_radius(), 0.0, 0.0)),
            position(Vec3::new(0.0, CapsRole::Chunk.world_radius(), 0.0)),
            position(Vec3::new(CapsRole::Entity.world_radius(), 0.0, 0.0)),
        ];
        let styles = [
            style(EPISODE_NODE_KIND),
            style(CHUNK_NODE_KIND),
            style(CHUNK_NODE_KIND),
            style(1),
        ];
        let caps_guides = [
            SceneCapsGuide {
                stable_id: 7,
                center: Vec3::Z.to_array(),
                aperture: 0.72,
                radius: CapsRole::Episode.world_radius(),
                role: CapsRole::Episode,
                weight: 18,
            },
            SceneCapsGuide {
                stable_id: 11,
                center: Vec3::X.to_array(),
                aperture: 0.24,
                radius: CapsRole::Chunk.world_radius(),
                role: CapsRole::Chunk,
                weight: 4,
            },
        ];
        let page = caps_guide_page(&positions, &styles, &caps_guides)
            .unwrap_or_else(|error| panic!("CAPS guide page: {error}"));
        assert!(page.bytes.len() <= 512 * 1024);
        let header = bytemuck::pod_read_unaligned::<GuidePageHeader>(
            &page.bytes[..std::mem::size_of::<GuidePageHeader>()],
        );
        assert_eq!(
            header.stroke_count as usize,
            CAPS_REFERENCE_ROLES.len() * 3 + caps_guides.len() + 1
        );
        assert_eq!(page.stroke_count, header.stroke_count as usize);
        let record_start = std::mem::size_of::<GuidePageHeader>();
        let record_end =
            record_start + header.stroke_count as usize * std::mem::size_of::<GuideStrokeRecord>();
        let records = page.bytes[record_start..record_end]
            .chunks_exact(std::mem::size_of::<GuideStrokeRecord>())
            .map(bytemuck::pod_read_unaligned::<GuideStrokeRecord>)
            .collect::<Vec<_>>();
        assert_eq!(
            records
                .iter()
                .filter(|record| record.flags == GUIDE_FLAG_SHELL)
                .count(),
            CAPS_REFERENCE_ROLES.len() * 3
        );
        assert_eq!(
            records
                .iter()
                .filter(|record| record.flags == GUIDE_FLAG_CAP_BOUNDARY)
                .count(),
            2
        );
        assert_eq!(
            records
                .iter()
                .filter(|record| record.flags == GUIDE_FLAG_CONCENTRATION_AXIS)
                .count(),
            1
        );
    }

    #[test]
    fn caps_guides_fail_closed_on_mismatched_node_pages() {
        let error = caps_guide_page(&[position(Vec3::X)], &[], &[])
            .err()
            .unwrap_or_else(|| panic!("mismatched CAPS pages must fail"));
        assert!(matches!(
            error,
            ScenePublicationError::InventoryMismatch("CAPS guide node pages")
        ));
    }

    fn position(value: Vec3) -> PositionRecord {
        PositionRecord {
            position: value.to_array(),
        }
    }

    fn style(kind: u16) -> NodeStyleRecord {
        NodeStyleRecord {
            color: [0.2, 0.8, 0.6, 1.0],
            radius: 1.0,
            kind,
            flags: 0,
        }
    }
}
