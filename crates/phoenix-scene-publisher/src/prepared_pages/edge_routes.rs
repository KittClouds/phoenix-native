//! Smooth endpoint-relative routes shared by every published manifold.
use phoenix_scene_archive::PositionRecord;

pub(super) const CURVE_SEGMENTS: usize = 12;

/// Quadratic curve through the former curved path's midpoint.
pub(super) fn curved_point(
    source: PositionRecord,
    target: PositionRecord,
    edge_slot: usize,
    progress: f32,
) -> PositionRecord {
    let a = source.position;
    let b = target.position;
    let delta_x = b[0] - a[0];
    let delta_y = b[1] - a[1];
    let length = delta_x.hypot(delta_y).max(1.0);
    let bend = (((edge_slot * 37) % 19) as f32 / 18.0 - 0.5) * length * 0.22;
    let midpoint = [
        (a[0] + b[0]) * 0.5 - delta_y / length * bend,
        (a[1] + b[1]) * 0.5 + delta_x / length * bend,
        (a[2] + b[2]) * 0.5 + bend * 0.18,
    ];
    let control =
        std::array::from_fn::<_, 3, _>(|axis| 2.0 * midpoint[axis] - (a[axis] + b[axis]) * 0.5);
    let t = progress.clamp(0.0, 1.0);
    let inverse = 1.0 - t;
    PositionRecord {
        position: std::array::from_fn(|axis| {
            inverse * inverse * a[axis] + 2.0 * inverse * t * control[axis] + t * t * b[axis]
        }),
    }
}

/// A cubic Bézier bends toward a shared X/Z corridor and eases out of it.
/// Both control points stay inside the endpoint box, so the route cannot
/// overshoot; reversing the edge reverses exactly the same curve.
pub(super) fn bundled_point(
    source: PositionRecord,
    target: PositionRecord,
    progress: f32,
) -> PositionRecord {
    let t = progress.clamp(0.0, 1.0);
    let inverse = 1.0 - t;
    let a = source.position;
    let b = target.position;
    let first = [
        a[0] * 0.32 + b[0] * 0.68,
        a[1] * 0.75 + b[1] * 0.25,
        a[2] * 0.32 + b[2] * 0.68,
    ];
    let second = [
        a[0] * 0.68 + b[0] * 0.32,
        a[1] * 0.25 + b[1] * 0.75,
        a[2] * 0.68 + b[2] * 0.32,
    ];
    PositionRecord {
        position: std::array::from_fn(|axis| {
            inverse * inverse * inverse * a[axis]
                + 3.0 * inverse * inverse * t * first[axis]
                + 3.0 * inverse * t * t * second[axis]
                + t * t * t * b[axis]
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    #[test]
    fn bundled_route_is_bounded_reversible_and_has_no_knee() {
        let source = PositionRecord {
            position: [-30.0, 21.0, 6.0],
        };
        let target = PositionRecord {
            position: [40.0, -19.0, -8.0],
        };
        let points: Vec<_> = (0..=CURVE_SEGMENTS)
            .map(|step| bundled_point(source, target, step as f32 / CURVE_SEGMENTS as f32))
            .collect();
        assert_eq!(points.first(), Some(&source));
        assert_eq!(points.last(), Some(&target));
        for (step, point) in points.iter().enumerate() {
            let reverse = bundled_point(target, source, 1.0 - step as f32 / CURVE_SEGMENTS as f32);
            for axis in 0..3 {
                assert!((point.position[axis] - reverse.position[axis]).abs() < 0.0001);
                assert!(point.position[axis] >= source.position[axis].min(target.position[axis]));
                assert!(point.position[axis] <= source.position[axis].max(target.position[axis]));
            }
        }
        for triple in points.windows(3) {
            let incoming =
                Vec3::from_slice(&triple[1].position) - Vec3::from_slice(&triple[0].position);
            let outgoing =
                Vec3::from_slice(&triple[2].position) - Vec3::from_slice(&triple[1].position);
            assert!(incoming.normalize().dot(outgoing.normalize()) > 0.9);
        }
    }

    #[test]
    fn curved_route_passes_through_both_nodes_without_a_midpoint_corner() {
        let source = PositionRecord {
            position: [0.0, 0.0, 0.0],
        };
        let target = PositionRecord {
            position: [20.0, 10.0, 4.0],
        };
        assert_eq!(curved_point(source, target, 3, 0.0), source);
        assert_eq!(curved_point(source, target, 3, 1.0), target);
        let before = curved_point(source, target, 3, 5.0 / 12.0);
        let middle = curved_point(source, target, 3, 0.5);
        let after = curved_point(source, target, 3, 7.0 / 12.0);
        let incoming = Vec3::from_slice(&middle.position) - Vec3::from_slice(&before.position);
        let outgoing = Vec3::from_slice(&after.position) - Vec3::from_slice(&middle.position);
        assert!(incoming.normalize().dot(outgoing.normalize()) > 0.99);
    }
}
