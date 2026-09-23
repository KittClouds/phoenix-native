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
mod edge_routes;
mod semantic;
use edge_routes::{bundled_point, curved_point, CURVE_SEGMENTS};
use semantic::semantic_guide_page;

const DEFAULT_GUIDE_STROKES: usize = 7;
const GUIDE_POINTS: usize = 65;
const MAX_CAP_BOUNDARIES: usize = 96;
const MAX_CURVED_PATH_SEGMENTS: usize = 400_000;
const CAPS_REFERENCE_ROLES: [CapsRole; 4] = [
    CapsRole::Document,
    CapsRole::Chapter,
    CapsRole::Paragraph,
    CapsRole::Sentence,
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
        return hybrid_guide_page(positions);
    }
    if manifold == ArchiveManifold::Caps {
        return caps_guide_page(positions, styles, caps_guides);
    }
    if manifold == ArchiveManifold::Hopf {
        return hopf_guide_page(positions.len());
    }
    if matches!(manifold, ArchiveManifold::Siegel | ArchiveManifold::Transit) {
        return semantic_guide_page(manifold);
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

fn hybrid_guide_page(
    positions: &[PositionRecord],
) -> Result<PreparedGuidePage, ScenePublicationError> {
    let mut occupied = [false; 12];
    for point in positions {
        let radius =
            Vec3::from_array(point.position).length() / phoenix_hybrid_space::HYBRID_WORLD_RADIUS;
        for role in CapsRole::ALL {
            if (radius - phoenix_hybrid_space::hierarchy_radius(role)).abs() < 0.001 {
                occupied[role as usize] = true;
            }
        }
    }
    let mut strokes = Vec::with_capacity(27);
    let mut points = Vec::with_capacity(27 * GUIDE_POINTS);
    for plane in 0..3 {
        append_hybrid_shell(&mut strokes, &mut points, plane)?;
    }
    for role in CapsRole::ALL {
        if !occupied[role as usize] {
            continue;
        }
        let radius = phoenix_hybrid_space::hierarchy_radius(role)
            * phoenix_hybrid_space::HYBRID_WORLD_RADIUS;
        for plane in 0..2 {
            let first_point = checked_u32(points.len(), "Hybrid role shell offset")?;
            for point in 0..GUIDE_POINTS {
                let angle = point as f32 * TAU / (GUIDE_POINTS - 1) as f32;
                let (sine, cosine) = angle.sin_cos();
                let position = if plane == 0 {
                    [cosine * radius, 0.0, sine * radius]
                } else {
                    [0.0, cosine * radius, sine * radius]
                };
                points.push(PositionRecord { position });
            }
            strokes.push(GuideStrokeRecord {
                first_point,
                point_count: GUIDE_POINTS as u32,
                rgba8: pack_rgba8([0.30, 0.63, 0.61, 0.075]),
                flags: GUIDE_FLAG_SHELL,
            });
        }
    }
    let bytes = variable_page(
        &GuidePageHeader {
            stroke_count: checked_u32(strokes.len(), "Hybrid shell count")?,
            point_count: checked_u32(points.len(), "Hybrid shell points")?,
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
            2 => [0.0, cosine * radius, sine * radius],
            _ => {
                let height = (plane as f32 - 5.5) / 3.5;
                let ring = (1.0 - height * height).sqrt() * radius;
                [cosine * ring, height * radius, sine * ring]
            }
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
            2 => [0.0, cosine * radius, sine * radius],
            _ => {
                let height = (plane as f32 - 5.5) / 3.5;
                let ring = (1.0 - height * height).sqrt() * radius;
                [cosine * ring, height * radius, sine * ring]
            }
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
    let shell_strokes = CAPS_REFERENCE_ROLES.len() * 9;
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
        for plane in 0..9 {
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
                match guide.role {
                    CapsRole::Document => CapsRole::Chapter.world_radius(),
                    CapsRole::Chapter => CapsRole::Paragraph.world_radius(),
                    CapsRole::Paragraph => CapsRole::Sentence.world_radius(),
                    _ => guide.radius,
                },
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
            2 => [0.0, cosine * radius, sine * radius],
            _ => {
                let height = (plane as f32 - 5.5) / 3.5;
                let ring = (1.0 - height * height).sqrt() * radius;
                [cosine * ring, height * radius, sine * ring]
            }
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
    let plane_alpha = if plane < 3 {
        [0.52, 0.42, 0.34][plane]
    } else {
        0.16
    };
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
    let alpha = 0.30 + (weight.ilog2().min(12) as f32 * 0.014);
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
    let segments_per_path = if style == 0 {
        1
    } else {
        CURVE_SEGMENTS.min((MAX_CURVED_PATH_SEGMENTS / publication.topology.len().max(1)).max(1))
    };
    let points_per_path = segments_per_path + 1;
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
        for step in 1..points_per_path - 1 {
            let progress = step as f32 / segments_per_path as f32;
            points.push(match style {
                1 => curved_point(source, target, edge_slot, progress),
                2 => bundled_point(source, target, progress),
                _ => unreachable!("only straight, curved, and bundled path styles are published"),
            });
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
    fn hybrid_guides_show_only_occupied_hierarchy_shells() {
        let radius = phoenix_hybrid_space::hierarchy_radius(CapsRole::Chapter)
            * phoenix_hybrid_space::HYBRID_WORLD_RADIUS;
        let page = hybrid_guide_page(&[PositionRecord {
            position: [radius, 0.0, 0.0],
        }])
        .unwrap();
        let header = bytemuck::pod_read_unaligned::<GuidePageHeader>(
            &page.bytes[..size_of::<GuidePageHeader>()],
        );
        assert_eq!(header.stroke_count, 5); // world + one occupied role, no confidence surfaces
        assert_eq!(hybrid_guide_page(&[]).unwrap().stroke_count, 3);
        let point_offset = size_of::<GuidePageHeader>() + 5 * size_of::<GuideStrokeRecord>();
        for point in page.bytes[point_offset..]
            .chunks_exact(size_of::<PositionRecord>())
            .skip(3 * GUIDE_POINTS)
        {
            let point = bytemuck::pod_read_unaligned::<PositionRecord>(point);
            assert!((Vec3::from_array(point.position).length() - radius).abs() < 0.0001);
        }
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
            CAPS_REFERENCE_ROLES.len() * 9 + caps_guides.len() + 1
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
            CAPS_REFERENCE_ROLES.len() * 9
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
