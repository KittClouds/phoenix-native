use super::*;

#[test]
fn candidate_term_occurrences_are_removed_only_from_their_marker_bits() {
    let candidate = CANDIDATES
        .iter()
        .position(|c| c.id == "bank_to_water")
        .unwrap();
    let event = Event {
        doc: 0,
        candidate,
        features: Features {
            family: [
                FamilyFeatures {
                    distinct_mask: (1 << 2) | (1 << 3),
                    raw_count: 3,
                    marker_occurrences: {
                        let mut values = [0; MAX_MARKERS];
                        values[2] = 1;
                        values[3] = 2;
                        values
                    },
                    ..FamilyFeatures::default()
                },
                FamilyFeatures {
                    distinct_mask: (1 << 4) | (1 << 5),
                    raw_count: 2,
                    marker_occurrences: {
                        let mut values = [0; MAX_MARKERS];
                        values[4] = 1;
                        values[5] = 1;
                        values
                    },
                    ..FamilyFeatures::default()
                },
                FamilyFeatures::default(),
            ],
            ..Features::default()
        },
    };
    let masks = context_only_masks(event);
    assert_eq!(masks[0] & (1 << 2), 0);
    assert_eq!(masks[0] & (1 << 3), 1 << 3);
    assert_eq!(masks[1] & (1 << 4), 0);
    assert_eq!(masks[1] & (1 << 5), 1 << 5);
}

#[test]
fn context_only_pair_route_requires_unique_matching_endpoint_winners() {
    let event = |doc: u64, candidate: usize, masks: [u32; 3]| {
        let mut families = [FamilyFeatures::default(); 3];
        for family in 0..3 {
            families[family].distinct_mask = masks[family];
            for bit in 0..marker_names(family).len() {
                if masks[family] & (1 << bit) != 0 {
                    families[family].marker_occurrences[bit] = 1;
                }
            }
        }
        Event {
            doc,
            candidate,
            features: Features {
                family: families,
                ..Features::default()
            },
        }
    };
    let candidate = CANDIDATES
        .iter()
        .position(|c| c.id == "repair_to_fix")
        .unwrap();
    let left = event(0, candidate, [1, 0, 0]);
    let right = event(1, candidate, [1, 0, 0]);
    assert_eq!(
        context_only_pair_route(Episode {
            candidate,
            nomination: left,
            witness: right,
        }),
        Some(0)
    );
    let tied = event(1, candidate, [1, 1, 0]);
    assert_eq!(
        context_only_pair_route(Episode {
            candidate,
            nomination: left,
            witness: tied,
        }),
        None
    );
}
