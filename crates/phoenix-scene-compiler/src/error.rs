use thiserror::Error;

#[derive(Debug, Error, Eq, PartialEq)]
pub enum NativeSceneCompilerError {
    #[error("scene generation zero is reserved")]
    ZeroGeneration,
    #[error("registry revision {provided} does not match canonical revision {canonical}")]
    RegistryRevisionMismatch { provided: u64, canonical: u64 },
    #[error("active document has no verified entity mentions")]
    NoVerifiedMentions,
    #[error("verified mention {start}..{end} no longer matches the active document")]
    StaleMention { start: u32, end: u32 },
    #[error("document contains {actual} bytes; scene references support at most {maximum}")]
    DocumentTooLarge { actual: usize, maximum: usize },
    #[error("chunk contains {actual} unique entities; maximum is {maximum}")]
    DenseChunk { actual: usize, maximum: usize },
    #[error("compiled edge count exceeds the maximum of {0}")]
    EdgeLimit(usize),
    #[error("stable {resource} identity collision")]
    IdentityCollision { resource: &'static str },
    #[error("integer range overflow while compiling {0}")]
    RangeOverflow(&'static str),
    #[error("highlight palette is invalid")]
    InvalidPalette,
    #[error("CAPS node slot {slot} has reserved identity zero")]
    CapsZeroIdentity { slot: usize },
    #[error("CAPS node slot {slot} has parent slot {parent} outside the node page")]
    CapsParentOutOfRange { slot: usize, parent: u32 },
    #[error("CAPS node slot {slot} cannot descend from parent slot {parent}")]
    CapsParentRole { slot: usize, parent: u32 },
    #[error("CAPS node slot {slot} has sibling rank {rank} outside count {count}")]
    CapsSiblingRange { slot: usize, rank: u32, count: u32 },
    #[error("CAPS node slot {slot} produced a non-finite or out-of-ball projection")]
    CapsProjectionInvalid { slot: usize },
}
