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

typed_u64_id!(NamespaceId, SourceId, ConversationId, TurnId, ContentUnitId,);

pub fn temporal_u64_subject_id(kind: crate::TemporalSubjectKindV1, id: u64) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"phoenix.temporal-subject/v1\0");
    hasher.update(&(kind as u16).to_le_bytes());
    hasher.update(&id.to_le_bytes());
    *hasher.finalize().as_bytes()
}

pub const fn temporal_candidate_subject_id(candidate_id: [u8; 32]) -> [u8; 32] {
    candidate_id
}
