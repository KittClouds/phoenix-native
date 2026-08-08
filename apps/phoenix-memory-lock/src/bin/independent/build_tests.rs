use super::*;

fn evidence(query_groups: u16, values: &[(usize, f32)]) -> RankEvidenceV3 {
    let mut evidence = RankEvidenceV3 {
        schema_version: 3,
        query_groups,
        matched_groups: query_groups,
        missing_groups: 0,
        query_flags: 0,
        field_count: 2,
        values: [0.0; 30],
    };
    for (index, value) in values {
        evidence.values[*index] = *value;
    }
    evidence
}

fn hit(evidence: RankEvidenceV3) -> SearchHit {
    SearchHit {
        document: phoenix_lexical_qps::DocumentId(0),
        external_id: 1,
        score: 0.0,
        v2_score: 0.0,
        lexical_score: 0.0,
        coverage: 0.0,
        proximity: 0.0,
        order: 0.0,
        phrase: 0.0,
        segment: 0.0,
        exact_field: 0.0,
        rank_features: phoenix_lexical_qps::RankFeatureVector::default(),
        rank_evidence_v3: evidence,
        relevance_tier: RelevanceTier::CompleteExactGroups,
    }
}

fn query(kind: QueryKind, text: &str) -> SourceQuery {
    SourceQuery {
        id: "q".to_owned(),
        text: text.to_owned(),
        relevant: HashMap::new(),
        family: "f".to_owned(),
        entity_family: "e".to_owned(),
        collection_cohort: "c".to_owned(),
        collected_at: 1,
        kind,
    }
}

#[test]
fn frozen_candidate_cap_and_no_learned_ranker() {
    let config = v2_config();
    assert_eq!(config.maximum_candidate_pool, 160);
    assert_eq!(config.maximum_query_groups, MAXIMUM_QUERY_GROUPS);
    assert!(!config.learned_ranker.is_enabled());
}

#[test]
fn long_query_reason_is_invariant() {
    assert_eq!(
        classify_reason(
            &query(QueryKind::ScientificClaim, "a deliberately long query"),
            hit(evidence(17, &[])),
            hit(evidence(17, &[])),
        ),
        JudgmentReasonV3::LongQueryFailure
    );
}

#[test]
fn phrase_delta_is_classified_without_v2_score() {
    let positive = evidence(
        2,
        &[
            (RankEvidenceV3::EXACT_PHRASE, 1.0),
            (RankEvidenceV3::ORDERED_FRACTION, 1.0),
        ],
    );
    assert_eq!(
        classify_reason(
            &query(QueryKind::ScientificClaim, "ordered phrase"),
            hit(positive),
            hit(evidence(2, &[])),
        ),
        JudgmentReasonV3::PhraseOrderFailure
    );
}
