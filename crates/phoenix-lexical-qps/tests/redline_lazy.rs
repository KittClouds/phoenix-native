//! REDLINE Phase 2: lazy-evaluation differential tests.
//!
//! Serving paths prune positions/evidence for proven losers; evidence paths
//! evaluate everything. Top-k serving output must equal the evidence-pool
//! prefix bit-exactly, over randomized corpora/configs/queries. This is the
//! exactness proof for branch-and-bound (plus pool/rows preservation, which
//! Phases 3-4 extend).

use phoenix_lexical_qps::{
    DocumentInput, Expansion, FeatureNormalizationV3, FieldConfig, LinearRankerV1, LinearRankerV3,
    QpsBuilder, QpsConfig, QueryGroup, RankEvidenceV3, SearchHit, SearchScratch,
    RANK_EVIDENCE_V3_FEATURE_COUNT,
};

fn corpus(seed: u64, documents: usize) -> Vec<String> {
    let vocabulary = (0..256)
        .map(|index| format!("term{index}"))
        .collect::<Vec<_>>();
    (0..documents)
        .map(|document| {
            let mut state = (document as u64 + 1)
                .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                .wrapping_add(seed.wrapping_mul(0xBF58_476D_1CE4_E5B9));
            let mut text = String::with_capacity(512);
            for token in 0..64 {
                state ^= state >> 12;
                state ^= state << 25;
                state ^= state >> 27;
                let word = &vocabulary[(state as usize) & (vocabulary.len() - 1)];
                text.push_str(word);
                text.push(if token % 16 == 15 { '.' } else { ' ' });
            }
            if document % 11 == 0 {
                text.push_str(" alpha beta gamma.");
            }
            text
        })
        .collect()
}

fn v2_key(hit: &SearchHit, with_tier: bool) -> String {
    // V2-identical fields only. Primitive V3 evidence (spans, locality,
    // rarity mass, exact identifier flag) is collected on evidence paths but
    // skipped on plain serving paths BY DESIGN (pre-existing); tier follows
    // the identifier flag, so it is pinned only without identifier fields.
    let mut floats = Vec::new();
    for value in [
        hit.score,
        hit.v2_score,
        hit.lexical_score,
        hit.coverage,
        hit.proximity,
        hit.order,
        hit.phrase,
        hit.segment,
        hit.exact_field,
    ] {
        floats.push(format!("{:08x}", value.to_bits()));
    }
    for value in hit.rank_features.0 {
        floats.push(format!("{:08x}", value.to_bits()));
    }
    format!(
        "{}:{}:{}{}",
        hit.document,
        hit.external_id,
        floats.join(""),
        if with_tier {
            format!(":{:?}", hit.relevance_tier)
        } else {
            String::new()
        },
    )
}

fn v3_key(hit: &SearchHit) -> String {
    // V3 serving collects full primitives, so evidence is bit-identical.
    let mut floats = Vec::new();
    for value in hit.rank_evidence_v3.values {
        floats.push(format!("{:08x}", value.to_bits()));
    }
    format!(
        "{}:{}:{}:{}:{}",
        v2_key(hit, true),
        floats.join(""),
        hit.rank_evidence_v3.query_groups,
        hit.rank_evidence_v3.matched_groups,
        hit.rank_evidence_v3.missing_groups,
    )
}

fn hit_key(hit: &SearchHit) -> String {
    v3_key(hit)
}

fn check_prefix(
    serving: &[SearchHit],
    evidence: &[SearchHit],
    serving_rows: u32,
    evidence_rows: u32,
    top_k: usize,
    key: &dyn Fn(&SearchHit) -> String,
    context: &str,
) {
    assert_eq!(
        serving.len(),
        serving.len().min(top_k),
        "{context}: serving length"
    );
    assert!(
        evidence.len() >= serving.len(),
        "{context}: evidence pool covers serving"
    );
    // Accumulation is untouched by lazy evaluation: identical row counts.
    assert_eq!(serving_rows, evidence_rows, "{context}: posting rows");
    for (index, (got, want)) in serving.iter().zip(evidence.iter()).enumerate() {
        let (g, w) = (key(got), key(want));
        if g != w {
            eprintln!("MISMATCH {context} hit {index}\n got {g}\nwant {w}");
        }
        assert_eq!(g, w, "{context} hit {index}");
    }
}

#[test]
fn lazy_serving_matches_evidence_prefix() {
    let queries = [
        "alpha beta gamma",
        "term3 term44 term200",
        "term3",
        "alpha",
        "neverindexedtoken",
        "term3 term3 term44",
    ];
    for seed in [1_u64, 7, 42] {
        let corpus = corpus(seed, 300);
        // Config A: default single field.
        // Config B: identifier field + exact bonuses.
        // Config C: enabled V1 learned ranker (exercises the V1 bound path).
        let mut v1_weights = [0.0; 12];
        v1_weights[0] = 1.0;
        v1_weights[4] = 0.5;
        let v1 = LinearRankerV1::from_weights(v1_weights).unwrap();
        let configs: Vec<(&str, Vec<FieldConfig>, QpsConfig)> = vec![
            (
                "default",
                vec![FieldConfig::new("body", 1.0, 0.75, 0.0)],
                QpsConfig::default(),
            ),
            (
                "ident",
                vec![
                    FieldConfig::new("identifier", 4.0, 0.25, 3.0),
                    FieldConfig::new("body", 1.0, 0.75, 0.0),
                ],
                QpsConfig {
                    v3_exact_identifier_fields: 1,
                    ..QpsConfig::default()
                },
            ),
            (
                "v1rank",
                vec![FieldConfig::new("body", 1.0, 0.75, 0.15)],
                QpsConfig {
                    learned_ranker: v1,
                    ..QpsConfig::default()
                },
            ),
        ];
        for (name, fields, config) in configs {
            let mut builder =
                QpsBuilder::new(Vec::from(fields).into_boxed_slice(), config).unwrap();
            // Config B uses two fields: identifier carries every 5th title token.
            let two_fields = name == "ident";
            for (document, text) in corpus.iter().enumerate() {
                if two_fields {
                    let identifier = if document % 5 == 0 {
                        "px9000"
                    } else {
                        "reference"
                    };
                    let values = [identifier, text.as_str()];
                    builder
                        .insert(DocumentInput {
                            external_id: document as u64,
                            fields: &values,
                        })
                        .unwrap();
                } else {
                    let values = [text.as_str()];
                    builder
                        .insert(DocumentInput {
                            external_id: document as u64,
                            fields: &values,
                        })
                        .unwrap();
                }
            }
            let index = builder.build().unwrap();
            let mut model_weights = [0.0; RANK_EVIDENCE_V3_FEATURE_COUNT];
            model_weights[RankEvidenceV3::BM25F_LEXICAL] = 8.0;
            model_weights[RankEvidenceV3::MATCHED_GROUP_FRACTION] = 2.0;
            let model =
                LinearRankerV3::from_weights(FeatureNormalizationV3::identity(), model_weights)
                    .unwrap();
            let mut serving_scratch = SearchScratch::with_document_capacity(300, 32);
            let mut evidence_scratch = SearchScratch::with_document_capacity(300, 32);
            let mut serving = Vec::new();
            let mut evidence = Vec::new();
            for query in queries {
                for top_k in [1_usize, 3, 10] {
                    let context = format!("{name}/seed{seed}/{query}/k{top_k}");
                    // Tiers pin only without identifier fields (flag differs
                    // between plain serving and evidence paths by design).
                    let tier_ok = name != "ident";
                    let serving_receipt = index
                        .search_into(query, top_k, &mut serving_scratch, &mut serving)
                        .unwrap();
                    let evidence_receipt = index
                        .search_evidence_into(query, top_k, &mut evidence_scratch, &mut evidence)
                        .unwrap();
                    check_prefix(
                        &serving,
                        &evidence,
                        serving_receipt.posting_rows_visited,
                        evidence_receipt.posting_rows_visited,
                        top_k,
                        &|hit| v2_key(hit, tier_ok),
                        &context,
                    );
                    let serving_receipt = index
                        .search_v3_into(query, top_k, &model, &mut serving_scratch, &mut serving)
                        .unwrap();
                    let evidence_receipt = index
                        .search_v3_evidence_into(
                            query,
                            top_k,
                            &model,
                            &mut evidence_scratch,
                            &mut evidence,
                        )
                        .unwrap();
                    check_prefix(
                        &serving,
                        &evidence,
                        serving_receipt.posting_rows_visited,
                        evidence_receipt.posting_rows_visited,
                        top_k,
                        &v3_key,
                        &context,
                    );
                }
            }
            // Explicit transport groups (one multi-expansion group).
            let alpha = [
                Expansion {
                    term: "alpha",
                    quality: 1.0,
                },
                Expansion {
                    term: "term3",
                    quality: 0.7,
                },
                Expansion {
                    term: "missingterm",
                    quality: 0.5,
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
            for top_k in [1_usize, 5, 25] {
                let context = format!("{name}/seed{seed}/groups/k{top_k}");
                let tier_ok = name != "ident";
                let serving_receipt = index
                    .search_groups_into(&groups, top_k, &mut serving_scratch, &mut serving)
                    .unwrap();
                let evidence_receipt = index
                    .search_groups_evidence_into(
                        &groups,
                        top_k,
                        &mut evidence_scratch,
                        &mut evidence,
                    )
                    .unwrap();
                check_prefix(
                    &serving,
                    &evidence,
                    serving_receipt.posting_rows_visited,
                    evidence_receipt.posting_rows_visited,
                    top_k,
                    &|hit| v2_key(hit, tier_ok),
                    &context,
                );
                let serving_receipt = index
                    .search_groups_v3_into(
                        &groups,
                        top_k,
                        &model,
                        &mut serving_scratch,
                        &mut serving,
                    )
                    .unwrap();
                let evidence_receipt = index
                    .search_groups_v3_evidence_into(
                        &groups,
                        top_k,
                        &model,
                        &mut evidence_scratch,
                        &mut evidence,
                    )
                    .unwrap();
                check_prefix(
                    &serving,
                    &evidence,
                    serving_receipt.posting_rows_visited,
                    evidence_receipt.posting_rows_visited,
                    top_k,
                    &v3_key,
                    &context,
                );
            }
        }
    }
}
