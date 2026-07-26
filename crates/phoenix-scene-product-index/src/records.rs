use bytemuck::{Pod, Zeroable};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct EntityId(pub u64);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct NodeId(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ReviewState {
    Accepted = 1,
    Proposed = 2,
    Rejected = 4,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Pod, Zeroable)]
pub struct NodeProductRecord {
    pub node_id: u64,
    pub family_mask: u64,
    pub scope_mask: u64,
    pub review_mask: u32,
    pub label_offset: u32,
    pub label_len: u32,
    pub inspector_ref: u32,
    pub provenance_ref: u32,
    pub reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Pod, Zeroable)]
pub struct EdgeProductRecord {
    pub edge_id: u64,
    pub family_mask: u64,
    pub scope_mask: u64,
    pub relation_mask: u64,
    pub review_mask: u32,
    pub inspector_ref: u32,
    pub provenance_ref: u32,
    pub reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Pod, Zeroable)]
pub struct EntityNodeMappingRecord {
    pub entity_id: u64,
    pub node_id: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Pod, Zeroable)]
pub struct ProductReferenceRecord {
    pub stable_ref: u64,
    pub source_offset: u64,
    pub source_len: u32,
    pub kind: u16,
    pub flags: u16,
}

const _: () = {
    assert!(size_of::<NodeProductRecord>() == 48);
    assert!(size_of::<EdgeProductRecord>() == 48);
    assert!(size_of::<EntityNodeMappingRecord>() == 16);
    assert!(size_of::<ProductReferenceRecord>() == 24);
};
