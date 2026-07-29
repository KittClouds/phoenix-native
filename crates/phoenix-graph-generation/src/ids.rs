macro_rules! typed_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        #[repr(transparent)]
        pub struct $name(pub u64);
    };
}

typed_id!(DocumentId);
typed_id!(ChunkId);
typed_id!(SentenceId);
typed_id!(SpanId);
typed_id!(EntityId);
typed_id!(MentionId);
typed_id!(EvidenceId);
typed_id!(AcceptedEdgeId);
typed_id!(DecisionId);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct CandidateEdgeId(pub [u8; 32]);
