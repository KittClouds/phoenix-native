//! Deterministic comparison-cohort freezer used only by archive contract tests.

use crate::{
    ArchiveBuildReceipt, ArchiveError, ArchiveManifold, EdgeRecord, GuidePageHeader,
    GuideStrokeRecord, LabelPriorityRecord, NodeIdentityRecord, NodeStyleRecord, PageKey, PageKind,
    PaletteEntryRecord, PathPageHeader, PathRecord, PhoenixSceneArchiveBuilderV1, PositionRecord,
    RelationMaskRecord, TopologyRecord,
};
use std::path::Path;

pub const COHORT_NAME: &str = "phoenix-cleanroom-10k-50k-v1";
pub const GENERATION_ID: u64 = 0x2026_0725_0000_0003;
pub const NODE_COUNT: usize = 10_000;
pub const EDGE_COUNT: usize = 50_000;
pub const PAGE_COUNT: u32 = 32;

#[derive(Debug)]
pub struct FrozenCohort {
    pub identities: Vec<NodeIdentityRecord>,
    pub styles: Vec<NodeStyleRecord>,
    pub topology: Vec<TopologyRecord>,
    pub edges: Vec<EdgeRecord>,
    pub positions: [Vec<PositionRecord>; 5],
    pub labels: Vec<LabelPriorityRecord>,
    pub relations: Vec<RelationMaskRecord>,
    pub palette: Vec<PaletteEntryRecord>,
}

impl FrozenCohort {
    pub fn generate() -> Self {
        let identities = (0..NODE_COUNT)
            .map(|index| NodeIdentityRecord {
                id: index as u64 + 1,
            })
            .collect();
        let styles = (0..NODE_COUNT)
            .map(|index| {
                let tint = (index % 7) as f32 / 7.0;
                NodeStyleRecord {
                    color: [0.18 + tint * 0.35, 0.78 - tint * 0.25, 0.62, 0.82],
                    radius: 0.22 + (index % 5) as f32 * 0.025,
                    kind: (index % 8) as u16,
                    flags: 0,
                }
            })
            .collect();
        let topology = (0..EDGE_COUNT)
            .map(|index| {
                let source = index % NODE_COUNT;
                let lane = index / NODE_COUNT + 1;
                let target = (source + lane * 97) % NODE_COUNT;
                TopologyRecord {
                    source_id: source as u64 + 1,
                    target_id: target as u64 + 1,
                }
            })
            .collect();
        let edges = (0..EDGE_COUNT)
            .map(|index| {
                let lane = index / NODE_COUNT + 1;
                EdgeRecord {
                    id: index as u64 + 1,
                    color: [0.35, 0.56, 0.66, 0.16],
                    width: 0.8,
                    kind: (lane % 4) as u16,
                    flags: 0,
                }
            })
            .collect();
        let positions = ArchiveManifold::ALL.map(generate_positions);
        let labels = (0..NODE_COUNT)
            .map(|index| LabelPriorityRecord {
                node_slot: index as u32,
                rank: ((index * 37) % NODE_COUNT) as u32,
            })
            .collect();
        let relations = (0..8)
            .map(|kind| RelationMaskRecord {
                visible_mask: 1_u64 << kind,
                relation_kind: kind,
                flags: 1,
                reserved: 0,
            })
            .collect();
        let palette = (0..8)
            .map(|kind| PaletteEntryRecord {
                rgba8: palette_color(kind),
                kind,
                flags: 0,
            })
            .collect();
        Self {
            identities,
            styles,
            topology,
            edges,
            positions,
            labels,
            relations,
            palette,
        }
    }
}

pub fn freeze(path: impl AsRef<Path>) -> Result<ArchiveBuildReceipt, ArchiveError> {
    let cohort = FrozenCohort::generate();
    let mut builder = PhoenixSceneArchiveBuilderV1::new(GENERATION_ID)?;
    builder
        .add_records(PageKey::shared(PageKind::NodeIdentity), &cohort.identities)?
        .add_records(PageKey::shared(PageKind::NodeStyle), &cohort.styles)?
        .add_records(PageKey::shared(PageKind::Topology), &cohort.topology)?
        .add_records(PageKey::shared(PageKind::Edge), &cohort.edges)?
        .add_records(PageKey::shared(PageKind::LabelPriority), &cohort.labels)?
        .add_records(PageKey::shared(PageKind::RelationMasks), &cohort.relations)?
        .add_records(PageKey::shared(PageKind::PalettePolicy), &cohort.palette)?;

    for (manifold_index, manifold) in ArchiveManifold::ALL.into_iter().enumerate() {
        let positions = &cohort.positions[manifold_index];
        builder.add_records(PageKey::manifold(PageKind::Positions, manifold), positions)?;
        let guides = encode_guides(manifold, positions)?;
        builder.add_page(PageKey::manifold(PageKind::Guides, manifold), guides, 0, 0)?;
        for (kind, style) in [
            (PageKind::StraightPaths, 0),
            (PageKind::CurvedPaths, 1),
            (PageKind::BundledPaths, 2),
        ] {
            let paths = encode_paths(style, positions, &cohort.topology)?;
            builder.add_page(PageKey::manifold(kind, manifold), paths, 0, 0)?;
        }
    }
    builder.write_to_path(path)
}

fn generate_positions(manifold: ArchiveManifold) -> Vec<PositionRecord> {
    (0..NODE_COUNT)
        .map(|index| {
            let grid_x = (index % 100) as f32 - 49.5;
            let grid_y = ((index / 100) % 100) as f32 - 49.5;
            let phase = index as f32 * 0.017_453_292;
            let position = match manifold {
                ArchiveManifold::Hybrid => {
                    [grid_x, grid_y, ((index * 37) % 211) as f32 * 0.025 - 2.625]
                }
                ArchiveManifold::Hopf => {
                    let major = 34.0 + ((index / 100) % 8) as f32 * 1.4;
                    let minor = 4.0 + (index % 11) as f32 * 0.12;
                    let theta = phase;
                    let phi = phase * 7.0 + (index % 100) as f32 * 0.031;
                    [
                        (major + minor * phi.cos()) * theta.cos(),
                        (major + minor * phi.cos()) * theta.sin(),
                        minor * phi.sin(),
                    ]
                }
                ArchiveManifold::Caps => {
                    let cluster = (index % 16) as f32;
                    let angle = cluster * std::f32::consts::TAU / 16.0;
                    let radius = 18.0 + (index % 97) as f32 * 0.08;
                    [
                        angle.cos() * 32.0 + phase.cos() * radius,
                        angle.sin() * 32.0 + phase.sin() * radius,
                        ((index / 16) % 80) as f32 * 0.18 - 7.2,
                    ]
                }
                ArchiveManifold::Transit => {
                    let lane = (index % 12) as f32 - 5.5;
                    [
                        grid_x * 1.35,
                        lane * 5.5 + (phase * 3.0).sin() * 0.8,
                        grid_y * 0.22,
                    ]
                }
                ArchiveManifold::Siegel => {
                    let radius = 0.45 * (index as f32).sqrt();
                    [
                        radius * phase.cos(),
                        radius * phase.sin(),
                        ((index % 257) as f32 - 128.0) * 0.035,
                    ]
                }
            };
            PositionRecord { position }
        })
        .collect()
}

fn encode_guides(
    manifold: ArchiveManifold,
    positions: &[PositionRecord],
) -> Result<Vec<u8>, ArchiveError> {
    const STROKES: usize = 8;
    const POINTS_PER_STROKE: usize = 65;
    let mut strokes = Vec::with_capacity(STROKES);
    let mut points = Vec::with_capacity(STROKES * POINTS_PER_STROKE);
    for stroke in 0..STROKES {
        let first_point =
            u32::try_from(points.len()).map_err(|_| ArchiveError::ArchiveRangeOverflow)?;
        let radius = 10.0 + stroke as f32 * 5.5;
        for point in 0..POINTS_PER_STROKE {
            let angle = point as f32 * std::f32::consts::TAU / (POINTS_PER_STROKE - 1) as f32;
            points.push(PositionRecord {
                position: [
                    angle.cos() * radius,
                    angle.sin() * radius,
                    manifold as u8 as f32 * 0.25,
                ],
            });
        }
        strokes.push(GuideStrokeRecord {
            first_point,
            point_count: POINTS_PER_STROKE as u32,
            rgba8: palette_color(stroke as u16),
            flags: 0,
        });
    }
    debug_assert!(!positions.is_empty());
    encode_variable_page(
        &GuidePageHeader {
            stroke_count: STROKES as u32,
            point_count: points.len() as u32,
            reserved: [0; 2],
        },
        &strokes,
        &points,
    )
}

fn encode_paths(
    style: u32,
    positions: &[PositionRecord],
    topology: &[TopologyRecord],
) -> Result<Vec<u8>, ArchiveError> {
    let points_per_path = match style {
        0 => 2,
        1 => 3,
        2 => 4,
        _ => return Err(ArchiveError::CorruptDirectory("invalid path style")),
    };
    let mut paths = Vec::with_capacity(topology.len());
    let mut points = Vec::with_capacity(topology.len() * points_per_path);
    for (edge_slot, edge) in topology.iter().enumerate() {
        let source =
            usize::try_from(edge.source_id - 1).map_err(|_| ArchiveError::ArchiveRangeOverflow)?;
        let target =
            usize::try_from(edge.target_id - 1).map_err(|_| ArchiveError::ArchiveRangeOverflow)?;
        let first_point =
            u32::try_from(points.len()).map_err(|_| ArchiveError::ArchiveRangeOverflow)?;
        let source_position = positions[source];
        let target_position = positions[target];
        points.push(source_position);
        if style == 1 {
            points.push(curve_midpoint(source_position, target_position, edge_slot));
        } else if style == 2 {
            points.push(positions[(source / 100) * 100]);
            points.push(positions[(target / 100) * 100]);
        }
        points.push(target_position);
        paths.push(PathRecord {
            edge_slot: edge_slot as u32,
            first_point,
            point_count: points_per_path as u16,
            flags: 0,
            rgba8: 0x295F_8F38,
        });
    }
    encode_variable_page(
        &PathPageHeader {
            path_count: paths.len() as u32,
            point_count: points.len() as u32,
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
    let bend = (edge_slot % 17) as f32 * 0.035 - 0.28;
    PositionRecord {
        position: [
            (source.position[0] + target.position[0]) * 0.5 - bend,
            (source.position[1] + target.position[1]) * 0.5 + bend,
            (source.position[2] + target.position[2]) * 0.5 + bend * 0.5,
        ],
    }
}

fn encode_variable_page<H: bytemuck::Pod, R: bytemuck::Pod>(
    header: &H,
    records: &[R],
    points: &[PositionRecord],
) -> Result<Vec<u8>, ArchiveError> {
    let capacity = size_of::<H>()
        .checked_add(size_of_val(records))
        .and_then(|value| value.checked_add(size_of_val(points)))
        .ok_or(ArchiveError::ArchiveRangeOverflow)?;
    let mut bytes = Vec::with_capacity(capacity);
    bytes.extend_from_slice(bytemuck::bytes_of(header));
    bytes.extend_from_slice(bytemuck::cast_slice(records));
    bytes.extend_from_slice(bytemuck::cast_slice(points));
    Ok(bytes)
}

const fn palette_color(kind: u16) -> u32 {
    const COLORS: [u32; 8] = [
        0xFF33_BBAA,
        0xFF57_8FE8,
        0xFFDF_657A,
        0xFFFF_B347,
        0xFF62_D2A2,
        0xFFC0_7BE8,
        0xFFFF_D166,
        0xFF58_C7D9,
    ];
    COLORS[kind as usize % COLORS.len()]
}
