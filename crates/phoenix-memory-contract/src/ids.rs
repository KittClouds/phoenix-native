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
