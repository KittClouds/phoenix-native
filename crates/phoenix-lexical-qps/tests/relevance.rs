use phoenix_lexical_qps::{
    CandidateSelection, DocumentInput, FieldConfig, QpsBuilder, QpsConfig, SearchScratch,
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
    }
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
