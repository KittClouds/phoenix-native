use phoenix_graph_generation_v2::{expected_authority, AuthorityClass, PageKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OracleDisposition {
    AuthoritativeSource,
    CandidateOnly,
    ContextualEvidenceOnly,
}

#[derive(Clone, Copy, Debug)]
struct ExpectedTopologyFamily {
    legacy_rule: &'static str,
    native_meaning: &'static str,
    disposition: OracleDisposition,
    pages: &'static [PageKind],
}

const EXPECTED_TOPOLOGY: &[ExpectedTopologyFamily] = &[
    ExpectedTopologyFamily {
        legacy_rule: "documentSpine",
        native_meaning: "document, chapter, paragraph, sentence, and span hierarchy",
        disposition: OracleDisposition::AuthoritativeSource,
        pages: &[
            PageKind::Documents,
            PageKind::Chapters,
            PageKind::Paragraphs,
            PageKind::Sentences,
            PageKind::Spans,
            PageKind::StructuralEdges,
        ],
    },
    ExpectedTopologyFamily {
        legacy_rule: "chunkSpine",
        native_meaning: "dynamic chunks carried forward without reconstruction",
        disposition: OracleDisposition::AuthoritativeSource,
        pages: &[PageKind::Chunks, PageKind::StructuralEdges],
    },
    ExpectedTopologyFamily {
        legacy_rule: "entityAnchors",
        native_meaning: "canonical entities and exact source mentions",
        disposition: OracleDisposition::AuthoritativeSource,
        pages: &[
            PageKind::Entities,
            PageKind::Mentions,
            PageKind::CanonicalEntityBindings,
        ],
    },
    ExpectedTopologyFamily {
        legacy_rule: "anchorEvidence",
        native_meaning: "independently addressable evidence bindings",
        disposition: OracleDisposition::AuthoritativeSource,
        pages: &[PageKind::Evidence],
    },
    ExpectedTopologyFamily {
        legacy_rule: "entityLinker",
        native_meaning: "identity, alias, and coreference proposals",
        disposition: OracleDisposition::CandidateOnly,
        pages: &[
            PageKind::IdentityCandidates,
            PageKind::CandidateEvidenceBindings,
        ],
    },
    ExpectedTopologyFamily {
        legacy_rule: "relationshipFacts",
        native_meaning: "typed relationship proposals",
        disposition: OracleDisposition::CandidateOnly,
        pages: &[PageKind::TypedRelationshipCandidates],
    },
    ExpectedTopologyFamily {
        legacy_rule: "eventIdentity",
        native_meaning: "evidence-bound event proposals",
        disposition: OracleDisposition::CandidateOnly,
        pages: &[PageKind::Events],
    },
    ExpectedTopologyFamily {
        legacy_rule: "episodeProjection",
        native_meaning: "evidence-bound episodes with typed chunk or event membership",
        disposition: OracleDisposition::CandidateOnly,
        pages: &[PageKind::Episodes, PageKind::EpisodeMemberships],
    },
    ExpectedTopologyFamily {
        legacy_rule: "temporalFacts",
        native_meaning: "evidence-bound directional temporal proposals",
        disposition: OracleDisposition::CandidateOnly,
        pages: &[PageKind::TemporalCandidates],
    },
    ExpectedTopologyFamily {
        legacy_rule: "causalFacts",
        native_meaning: "evidence-bound directional causal proposals",
        disposition: OracleDisposition::CandidateOnly,
        pages: &[PageKind::CausalCandidates],
    },
    ExpectedTopologyFamily {
        legacy_rule: "memoryStates",
        native_meaning: "subject, context, key, and value state proposals",
        disposition: OracleDisposition::CandidateOnly,
        pages: &[PageKind::MemoryStateCandidates],
    },
    ExpectedTopologyFamily {
        legacy_rule: "coOccurrenceWeak",
        native_meaning: "contextual proximity evidence without semantic promotion",
        disposition: OracleDisposition::ContextualEvidenceOnly,
        pages: &[PageKind::ContextualEvidence],
    },
];

#[test]
fn static_expected_topology_has_an_unambiguous_v2_home() {
    assert_eq!(EXPECTED_TOPOLOGY.len(), 12);

    for family in EXPECTED_TOPOLOGY {
        assert!(!family.legacy_rule.is_empty());
        assert!(!family.native_meaning.is_empty());
        assert!(!family.pages.is_empty());

        let expected = match family.disposition {
            OracleDisposition::AuthoritativeSource => AuthorityClass::SourceAuthoritative,
            OracleDisposition::CandidateOnly => AuthorityClass::SemanticCandidate,
            OracleDisposition::ContextualEvidenceOnly => AuthorityClass::ContextualEvidenceOnly,
        };

        for page in family.pages {
            assert_eq!(
                expected_authority(*page),
                expected,
                "{} mapped {:?} to the wrong authority",
                family.legacy_rule,
                page
            );
        }
    }
}

#[test]
fn every_expected_topology_page_is_covered_by_the_static_oracle() {
    const EXPECTED_PAGES: &[PageKind] = &[
        PageKind::Documents,
        PageKind::Chapters,
        PageKind::Paragraphs,
        PageKind::Sentences,
        PageKind::Chunks,
        PageKind::Spans,
        PageKind::Entities,
        PageKind::Mentions,
        PageKind::Evidence,
        PageKind::StructuralEdges,
        PageKind::TypedRelationshipCandidates,
        PageKind::IdentityCandidates,
        PageKind::CandidateEvidenceBindings,
        PageKind::CanonicalEntityBindings,
        PageKind::Events,
        PageKind::Episodes,
        PageKind::EpisodeMemberships,
        PageKind::TemporalCandidates,
        PageKind::CausalCandidates,
        PageKind::MemoryStateCandidates,
        PageKind::ContextualEvidence,
    ];

    for expected in EXPECTED_PAGES {
        assert!(
            EXPECTED_TOPOLOGY
                .iter()
                .any(|family| family.pages.contains(expected)),
            "{expected:?} is missing from the static expected-topology oracle"
        );
    }

    assert!(EXPECTED_TOPOLOGY.iter().all(|family| family
        .pages
        .iter()
        .all(|page| expected_authority(*page) != AuthorityClass::ProjectionOnly)));
}
