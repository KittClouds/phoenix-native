macro_rules! tagged_enum {
    (
        $(#[$meta:meta])*
        pub enum $name:ident {
            $($variant:ident = $value:expr),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        #[repr(u16)]
        pub enum $name {
            $($variant = $value),+
        }

        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            pub const fn from_raw(raw: u16) -> Option<Self> {
                match raw {
                    $($value => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

tagged_enum! {
    /// Authority is a property of a page, not of whether it is visible.
    pub enum AuthorityClass {
        SourceAuthoritative = 1,
        SemanticCandidate = 2,
        ContextualEvidenceOnly = 3,
        DecisionReceipt = 4,
        ProjectionOnly = 5,
        RuntimeReceipt = 6,
    }
}

tagged_enum! {
    pub enum CandidateStatus {
        Proposed = 1,
        Accepted = 2,
        Rejected = 3,
        Deferred = 4,
        Superseded = 5,
    }
}

tagged_enum! {
    pub enum DecisionAction {
        Accept = 1,
        Reject = 2,
        Defer = 3,
        Undo = 4,
    }
}

tagged_enum! {
    pub enum CapabilityState {
        Produced = 1,
        DurableVerified = 2,
        Unsupported = 3,
        Skipped = 4,
        Failed = 5,
        Cancelled = 6,
    }
}

tagged_enum! {
    pub enum CacheState {
        Computed = 1,
        DurableVerified = 2,
    }
}

tagged_enum! {
    pub enum ProducerProduct {
        DocumentStructure = 1,
        CanonicalEntities = 2,
        Identity = 3,
        Relationships = 4,
        Events = 5,
        Episodes = 6,
        Temporal = 7,
        Causal = 8,
        MemoryState = 9,
        ContextualEvidence = 10,
        NliAdjudication = 11,
    }
}

tagged_enum! {
    pub enum PublicationStatus {
        Prepared = 1,
        Published = 2,
        Replaced = 3,
        Rejected = 4,
    }
}

tagged_enum! {
    pub enum EpisodeMemberKind {
        Chunk = 1,
        Event = 2,
    }
}

tagged_enum! {
    pub enum SemanticFamily {
        Identity = 1,
        Alias = 2,
        Coreference = 3,
        Relationship = 4,
        Event = 5,
        Episode = 6,
        Temporal = 7,
        Causal = 8,
        MemoryState = 9,
        ContextualCoOccurrence = 10,
        GenericRelated = 11,
    }
}

tagged_enum! {
    pub enum EvidenceRole {
        Source = 1,
        Target = 2,
        Premise = 3,
        Hypothesis = 4,
        Subject = 5,
        State = 6,
        Cause = 7,
        Effect = 8,
        Membership = 9,
    }
}

tagged_enum! {
    pub enum CanonicalBindingKind {
        Direct = 1,
        CoordinatorDecision = 2,
    }
}

pub const STAGE_FLAG_TIMING_NOT_MEASURED: u32 = 1 << 0;
pub const STAGE_FLAG_ALLOCATION_NOT_MEASURED: u32 = 1 << 1;
pub const STAGE_FLAG_QUEUE_NOT_OBSERVED: u32 = 1 << 2;
