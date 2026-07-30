use bytemuck::{Pod, Zeroable};

macro_rules! typed_u64_id {
    ($($name:ident),+ $(,)?) => {
        $(
            #[derive(
                Clone,
                Copy,
                Debug,
                Default,
                Eq,
                Hash,
                Ord,
                PartialEq,
                PartialOrd,
                Pod,
                Zeroable,
            )]
            #[repr(transparent)]
            pub struct $name(pub u64);
        )+
    };
}

typed_u64_id!(
    GenerationId,
    DocumentId,
    ChapterId,
    ParagraphId,
    SentenceId,
    ChunkId,
    SpanId,
    EntityId,
    MentionId,
    EvidenceId,
    StructuralEdgeId,
    EventId,
    EpisodeId,
    TemporalId,
    CausalId,
    MemoryStateId,
    DecisionId,
    ReceiptId,
);

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Pod, Zeroable)]
#[repr(transparent)]
pub struct CandidateId(pub [u8; 32]);

impl CandidateId {
    pub const ZERO: Self = Self([0; 32]);

    pub fn is_zero(self) -> bool {
        self.0.iter().all(|byte| *byte == 0)
    }
}
