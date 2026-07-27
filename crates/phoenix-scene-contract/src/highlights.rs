use crate::{DocumentId, GraphGeneration};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;

pub const HIGHLIGHT_CONTRACT: &str = "phoenix.native.document-highlights/v1";
pub const MAX_DOCUMENT_ANCHORS: usize = 100_000;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HighlightMode {
    Off,
    #[default]
    Subtle,
    Vivid,
}

impl HighlightMode {
    pub const ALL: [Self; 3] = [Self::Off, Self::Subtle, Self::Vivid];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Subtle => "Subtle",
            Self::Vivid => "Vivid",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[repr(u16)]
#[serde(rename_all = "snake_case")]
pub enum EntityFamily {
    Character = 1,
    Location = 2,
    Organization = 3,
    Item = 4,
    Concept = 5,
    Event = 6,
    Structure = 7,
    Other = 255,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct FamilyPalette {
    pub primary: [f32; 4],
    pub secondary: [f32; 4],
}

impl FamilyPalette {
    pub fn validate(self) -> Result<(), HighlightContractError> {
        if self
            .primary
            .into_iter()
            .chain(self.secondary)
            .any(|channel| !channel.is_finite() || !(0.0..=1.0).contains(&channel))
        {
            return Err(HighlightContractError::InvalidPalette);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct HighlightPalette {
    pub character: FamilyPalette,
    pub location: FamilyPalette,
    pub organization: FamilyPalette,
    pub item: FamilyPalette,
    pub concept: FamilyPalette,
    pub event: FamilyPalette,
    pub structure: FamilyPalette,
    pub other: FamilyPalette,
}

impl Default for HighlightPalette {
    fn default() -> Self {
        Self {
            character: family(0x2450e6, 0x7c3aed),
            location: family(0x00a95c, 0x00c48c),
            organization: family(0x0077b6, 0x2563eb),
            item: family(0xe39b00, 0xe4572e),
            concept: family(0x00a896, 0x0891b2),
            event: family(0xe35216, 0xd7a000),
            structure: family(0xc02667, 0x7c3aed),
            other: family(0x71817b, 0x4b6b61),
        }
    }
}

impl HighlightPalette {
    pub fn for_family(self, family: EntityFamily) -> FamilyPalette {
        match family {
            EntityFamily::Character => self.character,
            EntityFamily::Location => self.location,
            EntityFamily::Organization => self.organization,
            EntityFamily::Item => self.item,
            EntityFamily::Concept => self.concept,
            EntityFamily::Event => self.event,
            EntityFamily::Structure => self.structure,
            EntityFamily::Other => self.other,
        }
    }

    pub fn validate(self) -> Result<(), HighlightContractError> {
        for palette in [
            self.character,
            self.location,
            self.organization,
            self.item,
            self.concept,
            self.event,
            self.structure,
            self.other,
        ] {
            palette.validate()?;
        }
        Ok(())
    }
}

const fn family(primary: u32, secondary: u32) -> FamilyPalette {
    FamilyPalette {
        primary: rgb(primary),
        secondary: rgb(secondary),
    }
}

const fn rgb(value: u32) -> [f32; 4] {
    [
        ((value >> 16) & 0xff) as f32 / 255.0,
        ((value >> 8) & 0xff) as f32 / 255.0,
        (value & 0xff) as f32 / 255.0,
        1.0,
    ]
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AnchorSource {
    ResidentGraph,
    ManualRegistry,
    VerificationFixture,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnchorCandidate {
    pub start: u32,
    pub end: u32,
    pub node_id: u64,
    pub entity_slot: u32,
    pub family: EntityFamily,
    pub surface: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DocumentAnchor {
    pub start: u32,
    pub end: u32,
    pub node_id: u64,
    pub entity_slot: u32,
    pub family: EntityFamily,
}

#[derive(Clone, Debug)]
pub struct VerifiedDocumentAnchors {
    document: DocumentId,
    document_revision: u64,
    content_hash: [u8; 32],
    graph_generation: Option<GraphGeneration>,
    source: AnchorSource,
    anchors: Arc<[DocumentAnchor]>,
}

impl VerifiedDocumentAnchors {
    pub fn verify(
        document: DocumentId,
        document_revision: u64,
        expected_content_hash: [u8; 32],
        graph_generation: Option<GraphGeneration>,
        source: AnchorSource,
        content: &str,
        mut candidates: Vec<AnchorCandidate>,
    ) -> Result<Self, HighlightContractError> {
        if source == AnchorSource::ResidentGraph && graph_generation.is_none() {
            return Err(HighlightContractError::MissingGraphGeneration);
        }
        if *blake3::hash(content.as_bytes()).as_bytes() != expected_content_hash {
            return Err(HighlightContractError::ContentHashMismatch);
        }
        if candidates.len() > MAX_DOCUMENT_ANCHORS {
            return Err(HighlightContractError::Oversized {
                actual: candidates.len(),
                maximum: MAX_DOCUMENT_ANCHORS,
            });
        }
        candidates.sort_unstable_by_key(|anchor| (anchor.start, anchor.end, anchor.node_id));
        let mut anchors = Vec::with_capacity(candidates.len());
        let mut previous_end = 0u32;
        for (index, candidate) in candidates.into_iter().enumerate() {
            let start = usize::try_from(candidate.start)
                .map_err(|_| HighlightContractError::RangeOutOfBounds)?;
            let end = usize::try_from(candidate.end)
                .map_err(|_| HighlightContractError::RangeOutOfBounds)?;
            if start >= end
                || end > content.len()
                || !content.is_char_boundary(start)
                || !content.is_char_boundary(end)
            {
                return Err(HighlightContractError::RangeOutOfBounds);
            }
            if index > 0 && candidate.start < previous_end {
                return Err(HighlightContractError::OverlappingAnchors);
            }
            if content[start..end] != candidate.surface {
                return Err(HighlightContractError::SurfaceMismatch {
                    start: candidate.start,
                    end: candidate.end,
                });
            }
            previous_end = candidate.end;
            anchors.push(DocumentAnchor {
                start: candidate.start,
                end: candidate.end,
                node_id: candidate.node_id,
                entity_slot: candidate.entity_slot,
                family: candidate.family,
            });
        }
        Ok(Self {
            document,
            document_revision,
            content_hash: expected_content_hash,
            graph_generation,
            source,
            anchors: anchors.into(),
        })
    }

    pub fn document(&self) -> DocumentId {
        self.document
    }

    pub fn document_revision(&self) -> u64 {
        self.document_revision
    }

    pub fn content_hash(&self) -> [u8; 32] {
        self.content_hash
    }

    pub fn graph_generation(&self) -> Option<GraphGeneration> {
        self.graph_generation
    }

    pub fn source(&self) -> AnchorSource {
        self.source
    }

    pub fn anchors(&self) -> &[DocumentAnchor] {
        &self.anchors
    }
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum HighlightContractError {
    #[error("resident graph anchors require a graph generation")]
    MissingGraphGeneration,
    #[error("document content hash does not match the anchor snapshot")]
    ContentHashMismatch,
    #[error("document anchor range is invalid or not a UTF-8 boundary")]
    RangeOutOfBounds,
    #[error("document anchors overlap")]
    OverlappingAnchors,
    #[error("anchor surface at {start}..{end} does not match document content")]
    SurfaceMismatch { start: u32, end: u32 },
    #[error("anchor snapshot has {actual} records; maximum is {maximum}")]
    Oversized { actual: usize, maximum: usize },
    #[error("highlight palette contains an invalid channel")]
    InvalidPalette,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verification_accepts_unicode_boundaries_and_sorts() {
        let content = "Ryan met 新羅 in New Rome.";
        let hash = *blake3::hash(content.as_bytes()).as_bytes();
        let new_rome = content.find("New Rome").expect("fixture");
        let unicode = content.find("新羅").expect("fixture");
        let verified = VerifiedDocumentAnchors::verify(
            DocumentId(7),
            3,
            hash,
            Some(GraphGeneration(9)),
            AnchorSource::ResidentGraph,
            content,
            vec![
                AnchorCandidate {
                    start: new_rome as u32,
                    end: (new_rome + "New Rome".len()) as u32,
                    node_id: 2,
                    entity_slot: 2,
                    family: EntityFamily::Location,
                    surface: "New Rome".into(),
                },
                AnchorCandidate {
                    start: unicode as u32,
                    end: (unicode + "新羅".len()) as u32,
                    node_id: 1,
                    entity_slot: 1,
                    family: EntityFamily::Organization,
                    surface: "新羅".into(),
                },
            ],
        )
        .expect("verified");
        assert_eq!(verified.anchors()[0].node_id, 1);
        assert_eq!(verified.anchors()[1].node_id, 2);
    }

    #[test]
    fn verification_fails_closed_on_overlap_and_surface_drift() {
        let content = "New Rome";
        let hash = *blake3::hash(content.as_bytes()).as_bytes();
        let candidate = |start, end, surface: &str| AnchorCandidate {
            start,
            end,
            node_id: u64::from(start) + 1,
            entity_slot: start + 1,
            family: EntityFamily::Location,
            surface: surface.into(),
        };
        assert_eq!(
            VerifiedDocumentAnchors::verify(
                DocumentId(1),
                1,
                hash,
                None,
                AnchorSource::VerificationFixture,
                content,
                vec![candidate(0, 8, "New Rome"), candidate(4, 8, "Rome")],
            )
            .expect_err("overlap"),
            HighlightContractError::OverlappingAnchors
        );
        assert!(matches!(
            VerifiedDocumentAnchors::verify(
                DocumentId(1),
                1,
                hash,
                None,
                AnchorSource::VerificationFixture,
                content,
                vec![candidate(0, 8, "Old Rome")],
            ),
            Err(HighlightContractError::SurfaceMismatch { .. })
        ));
    }
}
