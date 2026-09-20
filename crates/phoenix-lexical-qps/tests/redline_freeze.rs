//! REDLINE Phase 0: semantic freeze test.
//!
//! Pins exact serving outputs on a small deterministic corpus so structural
//! phases 1-4 can prove bit/order equivalence. Run with
//! `REDLINE_DUMP=1 cargo test -p phoenix-lexical-qps --test redline_freeze`
//! to print the current snapshot when intentionally updating expectations
//! (Phase 3+ work counters only, with report justification).
//!
//! Semantic freeze (NEVER changes under exact phases): hit ids, order,
//! score bits, tiers, evidence bits, pool id sets, group strengths.
//! Work freeze (valid through Phase 2; Phase 3 BMW changes rows/positions
//! by design): posting rows, position values, reranked counts.

use phoenix_lexical_qps::{
    DocumentInput, Expansion, FeatureNormalizationV3, FieldConfig, LinearRankerV3, QpsBuilder,
    QpsConfig, QueryGroup, RankEvidenceV3, SearchScratch,
};

const DOCS: usize = 500;
const TOKENS: usize = 96;
const TOP_K: usize = 5;

fn corpus() -> Vec<String> {
    let vocabulary = (0..512)
        .map(|index| format!("term{index}"))
        .collect::<Vec<_>>();
    (0..DOCS)
        .map(|document| {
            let mut state = (document as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15);
            let mut text = String::with_capacity(768);
            for token in 0..TOKENS {
                state ^= state >> 12;
                state ^= state << 25;
                state ^= state >> 27;
                let word = &vocabulary[(state as usize) & (vocabulary.len() - 1)];
                text.push_str(word);
                text.push(if token % 24 == 23 { '.' } else { ' ' });
            }
            if document % 97 == 0 {
                text.push_str(" graph memory retrieval.");
            }
            text
        })
        .collect()
}

fn receipt_line(label: &str, receipt: &phoenix_lexical_qps::SearchReceipt) -> String {
    // Structural receipt only: stage nanos are wall-clock and never frozen.
    format!(
        "receipt:{label} groups={} candidates={} covered={} reranked={} rows={} posvals={} selection={:?} alloc={}\n",
        receipt.query_groups,
        receipt.candidates,
        receipt.covered_candidates,
        receipt.reranked_candidates,
        receipt.posting_rows_visited,
        receipt.position_values_visited,
        receipt.selection,
        receipt.allocations_grew,
    )
}

fn bits(values: &[f32]) -> String {
    values
        .iter()
        .map(|v| format!("{:08x}", v.to_bits()))
        .collect::<Vec<_>>()
        .join("")
}

fn snapshot() -> String {
    let corpus = corpus();
    let fields = [FieldConfig::new("body", 1.0, 0.75, 0.0)];
    let mut builder =
        QpsBuilder::new(Vec::from(fields).into_boxed_slice(), QpsConfig::default()).unwrap();
    for (document, text) in corpus.iter().enumerate() {
        let values = [text.as_str()];
        builder
            .insert(DocumentInput {
                external_id: document as u64,
                fields: &values,
            })
            .unwrap();
    }
    let index = builder.build().unwrap();
    let mut out = String::new();
    out.push_str(&format!("stats={:?}\n", index.stats()));
    out.push_str(&format!(
        "sizeof_hit={}\n",
        std::mem::size_of::<phoenix_lexical_qps::SearchHit>()
    ));

    let mut scratch = SearchScratch::with_document_capacity(DOCS, 32);
    let mut hits = Vec::new();

    for query in [
        "graph memory retrieval",
        "term17 term203 term411",
        "term17",
        "neverindexedtoken",
    ] {
        hits.clear();
        let receipt = index
            .search_into(query, TOP_K, &mut scratch, &mut hits)
            .unwrap();
        dump_hits(&mut out, &format!("text:{query}"), &hits);
        out.push_str(&receipt_line(&format!("text:{query}"), &receipt));
    }

    let g0 = [
        Expansion {
            term: "graph",
            quality: 1.0,
        },
        Expansion {
            term: "term17",
            quality: 0.6,
        },
    ];
    let g1 = [Expansion {
        term: "memory",
        quality: 1.0,
    }];
    let g2 = [
        Expansion {
            term: "retrieval",
            quality: 1.0,
        },
        Expansion {
            term: "term411",
            quality: 0.6,
        },
    ];
    let groups = [
        QueryGroup { expansions: &g0 },
        QueryGroup { expansions: &g1 },
        QueryGroup { expansions: &g2 },
    ];
    hits.clear();
    let receipt = index
        .search_groups_into(&groups, TOP_K, &mut scratch, &mut hits)
        .unwrap();
    dump_hits(&mut out, "groups", &hits);
    out.push_str(&receipt_line("groups", &receipt));

    let mut strengths = phoenix_lexical_qps::GroupStrengthBatch::with_capacity(256, 32);
    hits.clear();
    index
        .search_groups_evidence_into(&groups, TOP_K, &mut scratch, &mut hits)
        .unwrap();
    index
        .capture_group_strengths_into(&scratch, &hits, &mut strengths)
        .unwrap();
    dump_hits(&mut out, "groups_evidence", &hits);
    for i in 0..strengths.hit_count() {
        out.push_str(&format!(
            "strengths:{i} {}\n",
            bits(strengths.strengths(i).unwrap())
        ));
    }

    hits.clear();
    index
        .search_exhaustive_into("graph memory retrieval", TOP_K, &mut scratch, &mut hits)
        .unwrap();
    dump_hits(&mut out, "exhaustive", &hits);

    let mut weights = [0.0; phoenix_lexical_qps::RANK_EVIDENCE_V3_FEATURE_COUNT];
    weights[RankEvidenceV3::BM25F_LEXICAL] = 8.0;
    let model = LinearRankerV3::from_weights(FeatureNormalizationV3::identity(), weights).unwrap();
    hits.clear();
    index
        .search_v3_into(
            "graph memory retrieval",
            TOP_K,
            &model,
            &mut scratch,
            &mut hits,
        )
        .unwrap();
    dump_hits(&mut out, "v3", &hits);

    out
}

fn dump_hits(out: &mut String, label: &str, hits: &[phoenix_lexical_qps::SearchHit]) {
    out.push_str(&format!("=={label}== len={}\n", hits.len()));
    for hit in hits {
        out.push_str(&format!(
            "{} s={:08x} v2={:08x} lex={:08x} tier={:?} ev={}\n",
            hit.external_id,
            hit.score.to_bits(),
            hit.v2_score.to_bits(),
            hit.lexical_score.to_bits(),
            hit.relevance_tier,
            bits(&hit.rank_evidence_v3.values),
        ));
    }
}

// EXPECTED is captured from the pre-REDLINE tree via REDLINE_DUMP=1.
// Semantic lines must never change under exact phases; work counters
// (receipt rows/positions) are frozen through Phase 2.
const EXPECTED: &str = include_str!("../redline/freeze_expected.snap");

#[test]
fn redline_semantic_freeze() {
    if std::env::var("REDLINE_DUMP").is_ok() {
        // Writes redline/freeze_expected.snap (crate root is the test CWD).
        // Re-run without the variable to verify.
        std::fs::create_dir_all("redline").unwrap();
        std::fs::write("redline/freeze_expected.snap", snapshot()).unwrap();
        return;
    }
    assert_eq!(snapshot(), EXPECTED);
}
