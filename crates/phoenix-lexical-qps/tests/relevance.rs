use phoenix_lexical_qps::{
    rerank_v3_in_place, CandidateSelection, DocumentInput, Expansion, FeatureNormalizationV3,
    FieldConfig, LinearRankerV3, QpsBuilder, QpsConfig, QueryGroup, RankEvidenceV3, RelevanceTier,
    SearchScratch, RANK_EVIDENCE_V3_FEATURE_COUNT,
};

#[test]
fn frozen_adversarial_and_ordinary_queries_keep_exact_first_rank() {
    let documents = [
        (
            101,
            "red dragon armor",
            "The artisan repaired the ceremonial plate before dawn.",
        ),
        (
            102,
            "inventory fragments",
            "red red red padding. dragon dragon dragon later. armor armor armor last.",
        ),
        (
            201,
            "memory correction policy",
            "Corrections retain old evidence and publish supersession.",
        ),
        (
            202,
            "policy notebook",
            "memory repeats here. correction repeats later. policy closes elsewhere.",
        ),
        (
            301,
            "camera controls",
            "The graph rotates around its central axis without orbit drift.",
        ),
        (
            302,
            "camera axis notes",
            "axis axis axis. unrelated filler. rotates later around scattered words graph.",
        ),
        (
            401,
            "conversation turn recall",
            "Recall reads committed history before the pending assistant turn is ingested.",
        ),
        (
            402,
            "conversation import",
            "Imported turns retain speaker roles, timestamps, and reply relationships.",
        ),
        (
            501,
            "dynamic chunk boundaries",
            "The compiler consumes exact chunk records instead of rebuilding paragraphs.",
        ),
        (
            502,
            "paragraph display",
            "The editor lays out paragraphs independently from retrieval chunk boundaries.",
        ),
    ];
    let fields = [
        FieldConfig::new("title", 3.0, 0.35, 0.45),
        FieldConfig::new("body", 1.0, 0.75, 0.15),
    ];
    let mut builder = QpsBuilder::new(Vec::from(fields).into_boxed_slice(), QpsConfig::default())
        .expect("valid QPS configuration");
    for (external_id, title, body) in documents {
        builder
            .insert(DocumentInput {
                external_id,
                fields: &[title, body],
            })
            .expect("insert fixture document");
    }
    let index = builder.build().expect("build frozen relevance index");
    let queries = [
        ("red dragon armor", 101),
        ("memory correction policy", 201),
        ("graph rotates central axis", 301),
        ("conversation turn recall", 401),
        ("dynamic chunk boundaries", 501),
    ];
    let mut scratch = SearchScratch::new();
    let mut oracle_scratch = SearchScratch::new();
    let mut hits = Vec::new();
    let mut oracle = Vec::new();
    for (query, expected) in queries {
        index
            .search_into(query, 10, &mut scratch, &mut hits)
            .expect("bounded query");
        index
            .search_exhaustive_into(query, 10, &mut oracle_scratch, &mut oracle)
            .expect("exhaustive query");
        assert_eq!(hits.first().map(|hit| hit.external_id), Some(expected));
        assert_eq!(oracle.first().map(|hit| hit.external_id), Some(expected));
        assert!(hits.iter().all(|hit| hit.rank_evidence_v3.is_valid()));
    }
}

#[test]
fn v3_evidence_is_primitive_complete_and_allocation_free_when_warm() {
    let fields = [
        FieldConfig::new("title", 2.5, 0.35, 0.35),
        FieldConfig::new("body", 1.0, 0.75, 0.10),
    ];
    let config = QpsConfig {
        maximum_candidate_pool: 160,
        maximum_query_groups: 128,
        ..QpsConfig::default()
    };
    let mut builder = QpsBuilder::new(Vec::from(fields).into_boxed_slice(), config).unwrap();
    for (external_id, title, body) in [
        (1, "alpha", "beta gamma"),
        (2, "alfa", "beta filler gamma"),
        (3, "alpha", "unrelated"),
    ] {
        builder
            .insert(DocumentInput {
                external_id,
                fields: &[title, body],
            })
            .unwrap();
    }
    let index = builder.build().unwrap();
    let alpha = [
        Expansion {
            term: "alpha",
            quality: 1.0,
        },
        Expansion {
            term: "alfa",
            quality: 0.7,
        },
    ];
    let beta = [Expansion {
        term: "beta",
        quality: 1.0,
    }];
    let groups = [
        QueryGroup { expansions: &alpha },
        QueryGroup { expansions: &beta },
    ];
    let mut scratch = SearchScratch::with_document_capacity(3, 128);
    let mut hits = Vec::with_capacity(160);
    index
        .search_groups_into(&groups, 10, &mut scratch, &mut hits)
        .expect("warm V3 primitive evidence");
    let receipt = index
        .search_groups_into(&groups, 10, &mut scratch, &mut hits)
        .expect("measure V3 primitive evidence");
    assert!(!receipt.allocations_grew);
    assert!(hits.iter().all(|hit| hit.rank_evidence_v3.is_valid()));

    let exact = hits.iter().find(|hit| hit.external_id == 1).unwrap();
    let fuzzy = hits.iter().find(|hit| hit.external_id == 2).unwrap();
    assert_eq!(exact.rank_evidence_v3.matched_groups, 2);
    assert_eq!(exact.rank_evidence_v3.missing_groups, 0);
    assert!(exact.rank_evidence_v3.values[RankEvidenceV3::FIELD_LEXICAL_0] > 0.0);
    assert!(exact.rank_evidence_v3.values[RankEvidenceV3::FIELD_LEXICAL_1] > 0.0);
    assert_eq!(
        exact.rank_evidence_v3.values[RankEvidenceV3::COMPLETE_COVERAGE],
        1.0
    );
    assert_eq!(
        exact.rank_evidence_v3.values[RankEvidenceV3::EXACT_GROUP_FRACTION],
        1.0
    );
    assert_eq!(
        fuzzy.rank_evidence_v3.values[RankEvidenceV3::MINIMUM_EXPANSION_QUALITY].to_bits(),
        0.7_f32.to_bits()
    );
    assert!(
        fuzzy.rank_evidence_v3.values[RankEvidenceV3::EXACT_GROUP_FRACTION]
            < exact.rank_evidence_v3.values[RankEvidenceV3::EXACT_GROUP_FRACTION]
    );
}

#[test]
fn constitutional_tiers_protect_identifiers_exact_coverage_and_expansions() {
    let fields = [
        FieldConfig::new("identifier", 1.0, 0.0, 0.0),
        FieldConfig::new("body", 1.0, 0.75, 0.0),
    ];
    let config = QpsConfig {
        maximum_candidate_pool: 160,
        maximum_query_groups: 128,
        v3_exact_identifier_fields: 1,
        ..QpsConfig::default()
    };
    let mut builder = QpsBuilder::new(Vec::from(fields).into_boxed_slice(), config).unwrap();
    for (external_id, identifier, body) in [
        (1, "px 9000", "primary record"),
        (2, "reference", "px px px 9000 9000 9000"),
        (3, "px 9001", "expanded identifier collision"),
        (4, "px", "partial repetition px px px px"),
    ] {
        builder
            .insert(DocumentInput {
                external_id,
                fields: &[identifier, body],
            })
            .unwrap();
    }
    let index = builder.build().unwrap();
    let px = [Expansion {
        term: "px",
        quality: 1.0,
    }];
    let number = [
        Expansion {
            term: "9000",
            quality: 1.0,
        },
        Expansion {
            term: "9001",
            quality: 0.8,
        },
    ];
    let groups = [
        QueryGroup { expansions: &px },
        QueryGroup {
            expansions: &number,
        },
    ];
    let mut scratch = SearchScratch::with_document_capacity(4, 128);
    let mut hits = Vec::with_capacity(160);
    index
        .search_groups_into(&groups, 10, &mut scratch, &mut hits)
        .unwrap();
    let tier = |identity| {
        hits.iter()
            .find(|hit| hit.external_id == identity)
            .unwrap()
            .relevance_tier
    };
    assert_eq!(tier(1), RelevanceTier::ConfiguredExactIdentifier);
    assert_eq!(tier(2), RelevanceTier::CompleteExactGroups);
    assert_eq!(tier(3), RelevanceTier::CompleteExpandedGroups);
    assert_eq!(tier(4), RelevanceTier::AdmissiblePartial);

    hits.sort_unstable_by(|left, right| {
        left.relevance_tier.compare_ranked(
            left.score,
            left.external_id,
            right.relevance_tier,
            right.score,
            right.external_id,
        )
    });
    assert_eq!(
        hits.iter().map(|hit| hit.external_id).collect::<Vec<_>>(),
        vec![1, 2, 3, 4]
    );
}

#[test]
fn v3_owns_final_order_without_changing_the_v2_candidate_substrate() {
    let config = QpsConfig {
        maximum_candidate_pool: 160,
        maximum_query_groups: 128,
        v3_exact_identifier_fields: 1,
        ..QpsConfig::default()
    };
    let mut builder = QpsBuilder::new(
        Vec::from([
            FieldConfig::new("identifier", 4.0, 0.25, 3.0),
            FieldConfig::new("body", 1.0, 0.75, 0.0),
        ])
        .into_boxed_slice(),
        config,
    )
    .unwrap();
    for (external_id, identifier, body) in [
        (1, "px 9000", "primary record"),
        (2, "reference", "px 9000"),
        (3, "px 9001", "expanded identifier collision"),
        (4, "px", "partial repetition px px px px px px"),
    ] {
        builder
            .insert(DocumentInput {
                external_id,
                fields: &[identifier, body],
            })
            .unwrap();
    }
    let index = builder.build().unwrap();
    let mut weights = [0.0; RANK_EVIDENCE_V3_FEATURE_COUNT];
    weights[RankEvidenceV3::BM25F_LEXICAL] = 8.0;
    let model = LinearRankerV3::from_weights(FeatureNormalizationV3::identity(), weights).unwrap();
    let px = [Expansion {
        term: "px",
        quality: 1.0,
    }];
    let number = [
        Expansion {
            term: "9000",
            quality: 1.0,
        },
        Expansion {
            term: "9001",
            quality: 0.8,
        },
    ];
    let groups = [
        QueryGroup { expansions: &px },
        QueryGroup {
            expansions: &number,
        },
    ];
    let mut v2_scratch = SearchScratch::with_document_capacity(4, 128);
    let mut v3_scratch = SearchScratch::with_document_capacity(4, 128);
    let mut v2 = Vec::with_capacity(160);
    let mut v3 = Vec::with_capacity(160);
    index
        .search_groups_evidence_into(&groups, 10, &mut v2_scratch, &mut v2)
        .unwrap();
    index
        .search_groups_v3_evidence_into(&groups, 10, &model, &mut v3_scratch, &mut v3)
        .unwrap();
    let mut v2_pool = v2.iter().map(|hit| hit.external_id).collect::<Vec<_>>();
    let mut v3_pool = v3.iter().map(|hit| hit.external_id).collect::<Vec<_>>();
    v2_pool.sort_unstable();
    v3_pool.sort_unstable();
    assert_eq!(v2_pool, v3_pool);
    for v3_hit in &v3 {
        let v2_hit = v2
            .iter()
            .find(|candidate| candidate.external_id == v3_hit.external_id)
            .unwrap();
        assert_eq!(v3_hit.v2_score.to_bits(), v2_hit.v2_score.to_bits());
        assert_eq!(v3_hit.rank_evidence_v3, v2_hit.rank_evidence_v3);
    }
    assert_eq!(
        v3.iter().map(|hit| hit.external_id).collect::<Vec<_>>(),
        vec![1, 2, 3, 4]
    );
    for hit in &mut v3 {
        hit.v2_score = if hit.external_id == 4 {
            1_000_000.0
        } else {
            0.0
        };
        hit.score = hit.v2_score;
    }
    rerank_v3_in_place(&model, &mut v3).unwrap();
    assert_eq!(
        v3.iter().map(|hit| hit.external_id).collect::<Vec<_>>(),
        vec![1, 2, 3, 4]
    );
}

#[test]
fn v3_exact_score_ties_use_stable_external_document_identity() {
    let mut builder = QpsBuilder::new(
        Vec::from([FieldConfig::new("body", 1.0, 0.75, 0.0)]).into_boxed_slice(),
        QpsConfig {
            maximum_candidate_pool: 160,
            ..QpsConfig::default()
        },
    )
    .unwrap();
    for external_id in [20, 10] {
        builder
            .insert(DocumentInput {
                external_id,
                fields: &["identical shared text"],
            })
            .unwrap();
    }
    let index = builder.build().unwrap();
    let mut weights = [0.0; RANK_EVIDENCE_V3_FEATURE_COUNT];
    weights[RankEvidenceV3::SINGLE_GROUP_FLAG] = 1.0;
    let model = LinearRankerV3::from_weights(FeatureNormalizationV3::identity(), weights).unwrap();
    let mut scratch = SearchScratch::with_document_capacity(2, 32);
    let mut hits = Vec::with_capacity(160);
    index
        .search_v3_into("shared", 10, &model, &mut scratch, &mut hits)
        .unwrap();
    assert_eq!(
        hits.iter().map(|hit| hit.external_id).collect::<Vec<_>>(),
        vec![10, 20]
    );
    let second = index
        .search_v3_into("shared", 10, &model, &mut scratch, &mut hits)
        .unwrap();
    assert!(!second.allocations_grew);
    assert_eq!(
        hits.iter().map(|hit| hit.external_id).collect::<Vec<_>>(),
        vec![10, 20]
    );
}

#[test]
fn dense_simd_lane_stays_bounded_and_warm_scratch_does_not_grow() {
    let config = QpsConfig {
        maximum_candidate_pool: 160,
        ..QpsConfig::default()
    };
    let mut builder = QpsBuilder::new(
        Vec::from([FieldConfig::new("body", 1.0, 0.75, 0.0)]).into_boxed_slice(),
        config,
    )
    .expect("valid dense configuration");
    for external_id in 0..1_000_u64 {
        let body = format!(
            "shared dense positional query document {external_id}. \
             extra token{} token{}.",
            external_id % 31,
            external_id % 47
        );
        builder
            .insert(DocumentInput {
                external_id,
                fields: &[body.as_str()],
            })
            .expect("insert dense document");
    }
    let index = builder.build().expect("build dense index");
    let mut scratch = SearchScratch::with_document_capacity(1_000, 32);
    let mut hits = Vec::with_capacity(10);
    index
        .search_into("shared dense positional query", 10, &mut scratch, &mut hits)
        .expect("warm dense query");
    let receipt = index
        .search_into("shared dense positional query", 10, &mut scratch, &mut hits)
        .expect("measured dense query");
    assert_eq!(receipt.selection, CandidateSelection::DenseSimd);
    assert_eq!(receipt.reranked_candidates, 160);
    assert!(!receipt.allocations_grew);
    assert!(receipt.stages.total > 0);

    let serving_order = hits.iter().map(|hit| hit.external_id).collect::<Vec<_>>();
    let mut evidence_scratch = SearchScratch::with_document_capacity(1_000, 32);
    let mut evidence = Vec::with_capacity(160);
    index
        .search_evidence_into(
            "shared dense positional query",
            10,
            &mut evidence_scratch,
            &mut evidence,
        )
        .expect("warm bounded candidate evidence capture");
    let evidence_receipt = index
        .search_evidence_into(
            "shared dense positional query",
            10,
            &mut evidence_scratch,
            &mut evidence,
        )
        .expect("capture bounded candidate evidence");
    assert_eq!(
        evidence_receipt.reranked_candidates,
        receipt.reranked_candidates
    );
    assert_eq!(evidence.len(), 160);
    assert_eq!(
        evidence
            .iter()
            .take(serving_order.len())
            .map(|hit| hit.external_id)
            .collect::<Vec<_>>(),
        serving_order
    );
    assert!(!evidence_receipt.allocations_grew);
}

#[test]
fn long_queries_cross_the_old_32_group_boundary_without_truncation() {
    let config = QpsConfig {
        maximum_query_groups: 128,
        ..QpsConfig::default()
    };
    let mut builder = QpsBuilder::new(
        Vec::from([FieldConfig::new("body", 1.0, 0.75, 0.0)]).into_boxed_slice(),
        config,
    )
    .expect("valid long-query configuration");
    let exact = (0..40)
        .map(|index| format!("term{index}"))
        .collect::<Vec<_>>()
        .join(" ");
    let scattered = (0..40)
        .map(|index| format!("term{index} filler"))
        .collect::<Vec<_>>()
        .join(" ");
    for (external_id, body) in [(1, exact.as_str()), (2, scattered.as_str())] {
        builder
            .insert(DocumentInput {
                external_id,
                fields: &[body],
            })
            .expect("insert long-query fixture");
    }
    let index = builder.build().expect("build long-query index");
    let mut scratch = SearchScratch::with_document_capacity(2, 128);
    let mut hits = Vec::with_capacity(2);
    let receipt = index
        .search_into(&exact, 2, &mut scratch, &mut hits)
        .expect("run 40-group query");
    assert_eq!(receipt.query_groups, 40);
    assert_eq!(hits.first().map(|hit| hit.external_id), Some(1));
}

#[test]
fn lexical_only_profile_never_opens_position_payloads() {
    let config = QpsConfig {
        proximity_weight: 0.0,
        order_weight: 0.0,
        phrase_weight: 0.0,
        segment_weight: 0.0,
        ..QpsConfig::default()
    };
    let mut builder = QpsBuilder::new(
        Vec::from([FieldConfig::new("body", 1.0, 0.75, 0.0)]).into_boxed_slice(),
        config,
    )
    .expect("valid lexical-only configuration");
    for (external_id, body) in [
        (1, "the exact compact sequence"),
        (2, "the sequence is scattered across an unrelated document"),
    ] {
        builder
            .insert(DocumentInput {
                external_id,
                fields: &[body],
            })
            .expect("insert lexical-only fixture");
    }
    let index = builder.build().expect("build lexical-only index");
    let mut scratch = SearchScratch::with_document_capacity(2, 8);
    let mut hits = Vec::with_capacity(2);
    let receipt = index
        .search_into("exact compact sequence", 2, &mut scratch, &mut hits)
        .expect("run lexical-only query");
    assert_eq!(receipt.reranked_candidates, 0);
    assert_eq!(receipt.position_values_visited, 0);
}

#[test]
fn scratch_reuse_across_different_index_sizes_is_grow_only_and_safe() {
    let fields = [FieldConfig::new("body", 1.0, 0.75, 0.0)];
    let config = QpsConfig::default();
    let mut large_builder =
        QpsBuilder::new(Vec::from(fields).into_boxed_slice(), config).expect("large builder");
    for external_id in 0..64_u64 {
        let text = format!("shared token document {external_id}");
        large_builder
            .insert(DocumentInput {
                external_id,
                fields: &[text.as_str()],
            })
            .expect("insert large document");
    }
    let large = large_builder.build().expect("build large index");
    let mut small_builder =
        QpsBuilder::new(Vec::from(fields).into_boxed_slice(), config).expect("small builder");
    for external_id in 0..2_u64 {
        let text = format!("small shared token {external_id}");
        small_builder
            .insert(DocumentInput {
                external_id,
                fields: &[text.as_str()],
            })
            .expect("insert small document");
    }
    let small = small_builder.build().expect("build small index");
    let mut scratch = SearchScratch::with_document_capacity(64, 16);
    let mut hits = Vec::with_capacity(10);

    large
        .search_into("shared token", 10, &mut scratch, &mut hits)
        .expect("search large index");
    small
        .search_into("shared token", 10, &mut scratch, &mut hits)
        .expect("search smaller index with reused scratch");
    let receipt = large
        .search_into("shared token", 10, &mut scratch, &mut hits)
        .expect("search large index again");

    assert!(!receipt.allocations_grew);
    assert_eq!(hits.len(), 10);
}
