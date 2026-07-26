//! Atomic native scene publication.
//!
//! A publication consists of two immutable files (`.psa` and `.pspi`) and one
//! small binary manifest. The manifest is replaced last and is the sole commit
//! point. Readers therefore observe either the previous coherent generation or
//! the new coherent generation, never a mixed pair.

mod error;
mod manifest;
mod model;
mod store;

pub use error::ScenePublicationError;
pub use manifest::{ScenePublicationKind, ScenePublicationReceipt, MANIFEST_CONTRACT};
pub use model::{NativeScenePublication, SceneEdgeProduct, SceneNodeProduct};
pub use store::{PublishedScene, ScenePublicationStore};

pub const SCENE_PUBLISHER_CONTRACT: &str = "phoenix.native.scene-publisher/v1";

#[cfg(test)]
mod tests;
