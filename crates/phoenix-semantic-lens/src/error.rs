use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SemanticLensError {
    #[error("semantic lens namespace is empty, oversized, or contains a NUL byte")]
    InvalidNamespace,
    #[error("semantic lens definition has no codes or exceeds the code budget")]
    InvalidCodeCount,
    #[error("semantic lens code {0} is zero or duplicated")]
    DuplicateCode(u32),
    #[error("semantic lens code name is empty, oversized, duplicated, or contains a NUL byte")]
    InvalidCodeName,
    #[error("semantic lens code {0} has an invalid semantic class")]
    InvalidSemanticClass(u32),
    #[error("semantic lens code {0} has an invalid endpoint mask")]
    InvalidEndpointMask(u32),
    #[error("semantic lens pack generation binding is missing")]
    MissingGenerationBinding,
    #[error("semantic lens pack already exists: {0}")]
    AlreadyExists(PathBuf),
    #[error("semantic lens pack I/O failed for {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("semantic lens pack is truncated")]
    Truncated,
    #[error("semantic lens pack is oversized")]
    Oversized,
    #[error("semantic lens pack magic is invalid")]
    InvalidMagic,
    #[error("semantic lens pack version {0} is unsupported")]
    UnsupportedVersion(u16),
    #[error("semantic lens pack header is invalid")]
    InvalidHeader,
    #[error("semantic lens pack header hash does not match")]
    HeaderHashMismatch,
    #[error("semantic lens pack payload hash does not match")]
    PayloadHashMismatch,
    #[error("semantic lens pack identity does not match its contents")]
    LensIdentityMismatch,
    #[error("semantic lens pack vocabulary hash does not match its code table")]
    VocabularyHashMismatch,
    #[error("semantic lens string reference is invalid UTF-8 or out of bounds")]
    InvalidStringReference,
    #[error("semantic lens code {0} is unknown")]
    UnknownCode(u32),
    #[error("semantic candidate namespace or domain is invalid")]
    InvalidCandidateNamespace,
    #[error("semantic review binding is incomplete or inconsistent")]
    InvalidReviewBinding,
}
