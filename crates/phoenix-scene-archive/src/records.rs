use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct NodeIdentityRecord {
    pub id: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct NodeStyleRecord {
    pub color: [f32; 4],
    pub radius: f32,
    pub kind: u16,
    pub flags: u16,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct TopologyRecord {
    pub source_id: u64,
    pub target_id: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct EdgeRecord {
    pub id: u64,
    pub color: [f32; 4],
    pub width: f32,
    pub kind: u16,
    pub flags: u16,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct PositionRecord {
    pub position: [f32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct LabelPriorityRecord {
    pub node_slot: u32,
    pub rank: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct RelationMaskRecord {
    pub visible_mask: u64,
    pub relation_kind: u16,
    pub flags: u16,
    pub reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct PaletteEntryRecord {
    pub rgba8: u32,
    pub kind: u16,
    pub flags: u16,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct GuidePageHeader {
    pub stroke_count: u32,
    pub point_count: u32,
    pub reserved: [u32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct GuideStrokeRecord {
    pub first_point: u32,
    pub point_count: u32,
    pub rgba8: u32,
    pub flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct PathPageHeader {
    pub path_count: u32,
    pub point_count: u32,
    pub path_style: u32,
    pub reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct PathRecord {
    pub edge_slot: u32,
    pub first_point: u32,
    pub point_count: u16,
    pub flags: u16,
    pub rgba8: u32,
}

const _: () = {
    assert!(size_of::<NodeIdentityRecord>() == 8);
    assert!(size_of::<NodeStyleRecord>() == 24);
    assert!(size_of::<TopologyRecord>() == 16);
    assert!(size_of::<EdgeRecord>() == 32);
    assert!(size_of::<PositionRecord>() == 12);
    assert!(size_of::<LabelPriorityRecord>() == 8);
    assert!(size_of::<RelationMaskRecord>() == 16);
    assert!(size_of::<PaletteEntryRecord>() == 8);
    assert!(size_of::<GuidePageHeader>() == 16);
    assert!(size_of::<GuideStrokeRecord>() == 16);
    assert!(size_of::<PathPageHeader>() == 16);
    assert!(size_of::<PathRecord>() == 16);
};
