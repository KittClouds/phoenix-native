//! Endpoint-relative routes: no world-origin attraction or slot-derived jitter.
use phoenix_scene_archive::PositionRecord;

/// Share an X/Z corridor between the endpoints and advance through semantic Y
/// bands. Every coordinate is a convex combination of the endpoint coordinates:
/// the polyline cannot overshoot their box, even for very short, distant edges.
/// Reversing an edge reverses its ports; translating nodes translates the route.
pub(super) fn bundle_ports(source: PositionRecord, target: PositionRecord) -> [PositionRecord; 2] {
    let a = source.position;
    let b = target.position;
    let x = a[0] * 0.5 + b[0] * 0.5;
    let z = a[2] * 0.5 + b[2] * 0.5;
    [
        PositionRecord {
            position: [x, a[1] * 0.75 + b[1] * 0.25, z],
        },
        PositionRecord {
            position: [x, a[1] * 0.25 + b[1] * 0.75, z],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundles_stay_between_endpoints_and_never_backtrack() {
        let cases = [
            ([100.0, 80.0, -90.0], [100.01, 80.02, -90.01]),
            ([-30.0, 21.0, 6.0], [40.0, -19.0, -8.0]),
            ([1.0, 1.0, 1.0], [1.0, 1.0, 1.0]),
            ([2.0, -3.0, 4.0], [2.0, 7.0, 4.0]),
        ];
        for (a, b) in cases {
            let ports = bundle_ports(
                PositionRecord { position: a },
                PositionRecord { position: b },
            );
            let route = [a, ports[0].position, ports[1].position, b];
            for point in route {
                for axis in 0..3 {
                    assert!(point[axis].is_finite());
                    assert!(point[axis] >= a[axis].min(b[axis]));
                    assert!(point[axis] <= a[axis].max(b[axis]));
                }
            }
            for pair in route.windows(2) {
                let progress: f32 = (0..3)
                    .map(|i| (pair[1][i] - pair[0][i]) * (b[i] - a[i]))
                    .sum();
                assert!(progress >= 0.0);
            }
            let reverse = bundle_ports(
                PositionRecord { position: b },
                PositionRecord { position: a },
            );
            assert_eq!(ports[0], reverse[1]);
            assert_eq!(ports[1], reverse[0]);
        }
    }

    #[test]
    fn bundles_translate_with_their_nodes() {
        let a = PositionRecord {
            position: [2.0, 4.0, 8.0],
        };
        let b = PositionRecord {
            position: [6.0, 12.0, 16.0],
        };
        let shift = [128.0, -256.0, 512.0];
        let translate = |p: PositionRecord| PositionRecord {
            position: std::array::from_fn(|i| p.position[i] + shift[i]),
        };
        assert_eq!(
            bundle_ports(translate(a), translate(b)),
            bundle_ports(a, b).map(translate)
        );
    }
}
