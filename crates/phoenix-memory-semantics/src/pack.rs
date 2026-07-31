use phoenix_memory_contract::VocabularyPackKindV3;
use phoenix_memory_coordinator::VocabularyPackDraft;
use std::sync::Arc;

pub const CORE_PACK_NAME: &str = "phoenix.core.memory";
pub const NARRATIVE_PACK_NAME: &str = "phoenix.lens.narrative";
pub const CONVERSATION_PACK_NAME: &str = "phoenix.lens.conversation";
pub const DOCUMENT_PACK_NAME: &str = "phoenix.lens.document";
const PACK_VERSION: &str = "1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PackDescriptor {
    pub id: u64,
    pub name: &'static str,
    pub version: &'static str,
    pub schema_hash: [u8; 32],
    pub producer_identity_hash: [u8; 32],
    pub kind: VocabularyPackKindV3,
}

impl PackDescriptor {
    pub fn draft(self) -> VocabularyPackDraft {
        VocabularyPackDraft {
            id: self.id,
            name: Arc::from(self.name),
            version: Arc::from(self.version),
            schema_hash: self.schema_hash,
            producer_identity_hash: self.producer_identity_hash,
            kind: self.kind,
            flags: 0,
        }
    }
}

pub fn core_pack() -> PackDescriptor {
    descriptor(CORE_PACK_NAME, VocabularyPackKindV3::Core)
}

pub fn narrative_pack() -> PackDescriptor {
    descriptor(NARRATIVE_PACK_NAME, VocabularyPackKindV3::Lens)
}

pub fn conversation_pack() -> PackDescriptor {
    descriptor(CONVERSATION_PACK_NAME, VocabularyPackKindV3::Lens)
}

pub fn document_pack() -> PackDescriptor {
    descriptor(DOCUMENT_PACK_NAME, VocabularyPackKindV3::Lens)
}

fn descriptor(name: &'static str, kind: VocabularyPackKindV3) -> PackDescriptor {
    let schema_hash = *blake3::hash(format!("{name}/{PACK_VERSION}/schema").as_bytes()).as_bytes();
    let producer_identity_hash =
        *blake3::hash(format!("phoenix-memory-semantics/{name}/{PACK_VERSION}").as_bytes())
            .as_bytes();
    PackDescriptor {
        id: nonzero_id(name.as_bytes()),
        name,
        version: PACK_VERSION,
        schema_hash,
        producer_identity_hash,
        kind,
    }
}

fn nonzero_id(bytes: &[u8]) -> u64 {
    let mut id = u64::from_le_bytes(
        blake3::hash(bytes).as_bytes()[..8]
            .try_into()
            .unwrap_or([0; 8]),
    );
    if id == 0 {
        id = 1;
    }
    id
}
