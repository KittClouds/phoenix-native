use super::*;

#[test]
fn frozen_mixed_baseline_captures_every_candidate_without_changing_v2() {
    let suite_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../memory-lock/qps-mixed-qualification-v1.json");
    let suite = load_suite(&suite_path).expect("load frozen mixed suite");
    let baseline = capture_mixed(&suite, 2).expect("capture mixed baseline");
    assert_eq!(baseline.queries.len(), 32);
    assert_eq!(baseline.metrics.hit_at_10, 1.0);
    assert_eq!(baseline.metrics.mean_reciprocal_rank, 1.0);
    assert_eq!(baseline.warm_allocation_growths, 0);
    assert_eq!(baseline.deterministic_ranking_failures, 0);
    assert!(baseline.maximum_candidate_pool <= CANDIDATE_CAP);
    assert!(baseline.queries.iter().all(|query| {
        query.candidate_pool.len() == query.execution.reranked_candidates as usize
            && query
                .candidate_pool
                .iter()
                .all(CandidateEvidenceV2::is_finite)
    }));
}

#[test]
fn frozen_configuration_hash_is_byte_stable() {
    let first = serde_json::to_vec(&frozen_configuration()).unwrap();
    let second = serde_json::to_vec(&frozen_configuration()).unwrap();
    assert_eq!(sha256_bytes(&first), sha256_bytes(&second));
    assert_eq!(v2_config().maximum_candidate_pool, CANDIDATE_CAP);
    assert!(!v2_config().learned_ranker.is_enabled());
}
