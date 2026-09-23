use super::*;
use phoenix_scene_contract::{
    siegel_band_center, transit_layer_radius, transit_layer_y, SIEGEL_BAND_COUNT,
};

pub(super) fn semantic_guide_page(
    manifold: ArchiveManifold,
) -> Result<PreparedGuidePage, ScenePublicationError> {
    let mut strokes = Vec::with_capacity(24);
    let mut points = Vec::with_capacity(24 * GUIDE_POINTS);
    match manifold {
        ArchiveManifold::Siegel => {
            for band in 0..SIEGEL_BAND_COUNT {
                let center = siegel_band_center(band);
                let first_point = checked_u32(points.len(), "Siegel guide offset")?;
                for x in [-34.0, 34.0] {
                    points.push(PositionRecord {
                        position: [x, center[1], -14.0],
                    });
                }
                strokes.push(GuideStrokeRecord {
                    first_point,
                    point_count: 2,
                    rgba8: caps_shell_color(CapsRole::ALL[band], 1),
                    flags: GUIDE_FLAG_SHELL,
                });
            }
        }
        ArchiveManifold::Transit => {
            for role in CapsRole::ALL {
                for fraction in [1.0, 0.72] {
                    let radius = transit_layer_radius(role) * fraction;
                    let first_point = checked_u32(points.len(), "Transit guide offset")?;
                    for point in 0..GUIDE_POINTS {
                        let angle = point as f32 * TAU / (GUIDE_POINTS - 1) as f32;
                        points.push(PositionRecord {
                            position: [
                                radius * angle.cos(),
                                transit_layer_y(role),
                                radius * angle.sin(),
                            ],
                        });
                    }
                    strokes.push(GuideStrokeRecord {
                        first_point,
                        point_count: GUIDE_POINTS as u32,
                        rgba8: caps_shell_color(role, 0),
                        flags: GUIDE_FLAG_SHELL,
                    });
                }
            }
        }
        _ => unreachable!("semantic guides only serve Siegel and Transit"),
    }
    let bytes = variable_page(
        &GuidePageHeader {
            stroke_count: checked_u32(strokes.len(), "semantic guide strokes")?,
            point_count: checked_u32(points.len(), "semantic guide points")?,
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
