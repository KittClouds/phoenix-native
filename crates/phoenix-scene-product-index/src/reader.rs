use crate::format::{
    index_hash, ProductIndexBinding, ProductIndexHeader, SectionRange, FORMAT_VERSION, HEADER_SIZE,
    MAGIC, MAX_INDEX_BYTES, MAX_LABEL_BYTES, MAX_REFERENCE_RECORDS, NO_REFERENCE,
};
use crate::{
    EdgeProductRecord, EntityId, EntityNodeMappingRecord, NodeId, NodeProductRecord,
    ProductIndexError, ProductReferenceRecord,
};
use hashbrown::HashSet;
use memmap2::{Mmap, MmapOptions};
use phoenix_scene_archive::{ArchiveManifold, PhoenixSceneArchiveV1};
use std::fs::File;
use std::path::Path;

#[derive(Debug)]
pub struct PhoenixSceneProductIndexV1 {
    mmap: Mmap,
    header: ProductIndexHeader,
    sections: [SectionRange; 5],
}

impl PhoenixSceneProductIndexV1 {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ProductIndexError> {
        let path = path.as_ref();
        let file = File::open(path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                ProductIndexError::Missing(path.to_path_buf())
            } else {
                ProductIndexError::Io(error)
            }
        })?;
        let file_len = file.metadata()?.len();
        if file_len > MAX_INDEX_BYTES {
            return Err(ProductIndexError::Oversized {
                actual: file_len,
                limit: MAX_INDEX_BYTES,
            });
        }
        if file_len < HEADER_SIZE as u64 {
            return Err(ProductIndexError::CorruptHeader(
                "file is shorter than header",
            ));
        }
        // SAFETY: the file is opened read-only and the mapping is never exposed mutably.
        let mmap = unsafe { MmapOptions::new().map(&file)? };
        Self::from_mmap(mmap, file_len)
    }

    fn from_mmap(mmap: Mmap, actual_file_len: u64) -> Result<Self, ProductIndexError> {
        let (header, sections) = decode_header(&mmap[..HEADER_SIZE])?;
        if header.file_len != actual_file_len {
            return Err(ProductIndexError::CorruptHeader(
                "recorded file length mismatch",
            ));
        }
        validate_sections(&sections, header, actual_file_len)?;
        let body = &mmap[HEADER_SIZE..];
        if blake3::hash(body).as_bytes() != &header.body_hash {
            return Err(ProductIndexError::CorruptBody);
        }
        let expected_index_hash = index_hash(
            header.binding,
            header.node_count,
            header.edge_count,
            header.mapping_count,
            header.reference_count,
            header.label_bytes,
            header.body_hash,
        );
        if expected_index_hash != header.index_hash {
            return Err(ProductIndexError::CorruptIndexHash);
        }
        let index = Self {
            mmap,
            header,
            sections,
        };
        index.validate_internal()?;
        Ok(index)
    }

    #[must_use]
    pub const fn header(&self) -> ProductIndexHeader {
        self.header
    }

    pub fn nodes(&self) -> &[NodeProductRecord] {
        bytemuck::cast_slice(self.validated_section_bytes(0))
    }

    pub fn edges(&self) -> &[EdgeProductRecord] {
        bytemuck::cast_slice(self.validated_section_bytes(1))
    }

    pub fn mappings(&self) -> &[EntityNodeMappingRecord] {
        bytemuck::cast_slice(self.validated_section_bytes(2))
    }

    pub fn references(&self) -> &[ProductReferenceRecord] {
        bytemuck::cast_slice(self.validated_section_bytes(3))
    }

    pub fn label_slab(&self) -> &str {
        let bytes = self.validated_section_bytes(4);
        // The entire slab is checked during open.
        unsafe { std::str::from_utf8_unchecked(bytes) }
    }

    pub fn label(&self, node_slot: usize) -> Option<&str> {
        let record = self.nodes().get(node_slot)?;
        let start = record.label_offset as usize;
        let end = start.checked_add(record.label_len as usize)?;
        self.label_slab().get(start..end)
    }

    pub fn node_for_entity(&self, entity: EntityId) -> Option<NodeId> {
        self.mappings()
            .binary_search_by_key(&entity.0, |mapping| mapping.entity_id)
            .ok()
            .map(|slot| NodeId(self.mappings()[slot].node_id))
    }

    pub fn entity_for_node(&self, node: NodeId) -> Option<EntityId> {
        self.mappings()
            .iter()
            .find(|mapping| mapping.node_id == node.0)
            .map(|mapping| EntityId(mapping.entity_id))
    }

    pub fn bind_to_archive(
        &self,
        archive: &PhoenixSceneArchiveV1,
    ) -> Result<(), ProductIndexError> {
        let archive_header = archive.header();
        if self.header.binding.archive_generation != archive_header.generation_id {
            return Err(ProductIndexError::StaleGeneration {
                index: self.header.binding.archive_generation,
                archive: archive_header.generation_id,
            });
        }
        if self.header.binding.archive_cohort_hash != archive_header.cohort_hash {
            return Err(ProductIndexError::CohortMismatch);
        }
        let pages = archive
            .open_manifold(ArchiveManifold::Hybrid)
            .map_err(|_| ProductIndexError::InvalidSection("archive pages"))?;
        verify_identities(
            "node",
            self.nodes(),
            pages.identities,
            |record| record.node_id,
            |r| r.id,
        )?;
        verify_identities(
            "edge",
            self.edges(),
            pages.edges,
            |record| record.edge_id,
            |r| r.id,
        )?;
        let node_ids = pages
            .identities
            .iter()
            .map(|record| record.id)
            .collect::<HashSet<_>>();
        for mapping in self.mappings() {
            if !node_ids.contains(&mapping.node_id) {
                return Err(ProductIndexError::MissingMappedNode {
                    entity: mapping.entity_id,
                    node: mapping.node_id,
                });
            }
        }
        Ok(())
    }

    fn validate_internal(&self) -> Result<(), ProductIndexError> {
        let _: &[NodeProductRecord] = self.typed_section(0)?;
        let _: &[EdgeProductRecord] = self.typed_section(1)?;
        let mappings: &[EntityNodeMappingRecord] = self.typed_section(2)?;
        let references: &[ProductReferenceRecord] = self.typed_section(3)?;
        let label_bytes = self.section_bytes(4)?;
        std::str::from_utf8(label_bytes).map_err(|_| ProductIndexError::InvalidLabelSlab)?;
        let mut mapped_nodes = HashSet::with_capacity(mappings.len());
        for (slot, mapping) in mappings.iter().enumerate() {
            if mapping.entity_id == 0 {
                return Err(ProductIndexError::ZeroEntityIdentity);
            }
            if slot > 0 && mappings[slot - 1].entity_id >= mapping.entity_id {
                return Err(ProductIndexError::DuplicateEntity(mapping.entity_id));
            }
            if !mapped_nodes.insert(mapping.node_id) {
                return Err(ProductIndexError::DuplicateMappedNode(mapping.node_id));
            }
        }
        for (slot, node) in self.nodes().iter().enumerate() {
            let start = node.label_offset as usize;
            let end = start
                .checked_add(node.label_len as usize)
                .ok_or(ProductIndexError::InvalidLabelRange(slot))?;
            if label_bytes.get(start..end).is_none()
                || std::str::from_utf8(&label_bytes[start..end]).is_err()
            {
                return Err(ProductIndexError::InvalidLabelRange(slot));
            }
            validate_reference(node.inspector_ref, references.len(), "node", slot)?;
            validate_reference(node.provenance_ref, references.len(), "node", slot)?;
        }
        for (slot, edge) in self.edges().iter().enumerate() {
            validate_reference(edge.inspector_ref, references.len(), "edge", slot)?;
            validate_reference(edge.provenance_ref, references.len(), "edge", slot)?;
        }
        Ok(())
    }

    fn typed_section<T: bytemuck::Pod>(&self, index: usize) -> Result<&[T], ProductIndexError> {
        bytemuck::try_cast_slice(self.section_bytes(index)?)
            .map_err(|_| ProductIndexError::InvalidSection(section_name(index)))
    }

    fn section_bytes(&self, index: usize) -> Result<&[u8], ProductIndexError> {
        let range = self.sections[index];
        let start = usize::try_from(range.offset).map_err(|_| ProductIndexError::RangeOverflow)?;
        let len = usize::try_from(range.len).map_err(|_| ProductIndexError::RangeOverflow)?;
        let end = start
            .checked_add(len)
            .ok_or(ProductIndexError::RangeOverflow)?;
        self.mmap
            .get(start..end)
            .ok_or(ProductIndexError::InvalidSection(section_name(index)))
    }

    fn validated_section_bytes(&self, index: usize) -> &[u8] {
        let range = self.sections[index];
        let start = range.offset as usize;
        let end = start + range.len as usize;
        &self.mmap[start..end]
    }
}

fn validate_reference(
    reference: u32,
    reference_count: usize,
    resource: &'static str,
    slot: usize,
) -> Result<(), ProductIndexError> {
    if reference != NO_REFERENCE && reference as usize >= reference_count {
        return Err(ProductIndexError::InvalidReference {
            resource,
            slot,
            reference,
        });
    }
    Ok(())
}

fn verify_identities<A, B>(
    resource: &'static str,
    index: &[A],
    archive: &[B],
    index_id: impl Fn(&A) -> u64,
    archive_id: impl Fn(&B) -> u64,
) -> Result<(), ProductIndexError> {
    if index.len() != archive.len() {
        return Err(ProductIndexError::InventoryMismatch {
            resource,
            index: index.len(),
            archive: archive.len(),
        });
    }
    for (slot, (left, right)) in index.iter().zip(archive).enumerate() {
        let left = index_id(left);
        let right = archive_id(right);
        if left != right {
            return Err(ProductIndexError::IdentityMismatch {
                resource,
                slot,
                index_id: left,
                archive_id: right,
            });
        }
    }
    Ok(())
}

fn decode_header(
    bytes: &[u8],
) -> Result<(ProductIndexHeader, [SectionRange; 5]), ProductIndexError> {
    if bytes[0..8] != MAGIC {
        return Err(ProductIndexError::CorruptHeader("magic mismatch"));
    }
    let version = get_u32(bytes, 8);
    if version != FORMAT_VERSION {
        return Err(ProductIndexError::UnsupportedFormatVersion(version));
    }
    if get_u32(bytes, 12) != HEADER_SIZE as u32 {
        return Err(ProductIndexError::CorruptHeader("header size mismatch"));
    }
    let mut archive_cohort_hash = [0_u8; 32];
    archive_cohort_hash.copy_from_slice(&bytes[56..88]);
    let mut body_hash = [0_u8; 32];
    body_hash.copy_from_slice(&bytes[88..120]);
    let mut stored_index_hash = [0_u8; 32];
    stored_index_hash.copy_from_slice(&bytes[120..152]);
    let sections = std::array::from_fn(|index| SectionRange {
        offset: get_u64(bytes, 152 + index * 16),
        len: get_u64(bytes, 160 + index * 16),
    });
    Ok((
        ProductIndexHeader {
            binding: ProductIndexBinding {
                archive_generation: get_u64(bytes, 16),
                archive_cohort_hash,
            },
            file_len: get_u64(bytes, 24),
            node_count: get_u32(bytes, 32),
            edge_count: get_u32(bytes, 36),
            mapping_count: get_u32(bytes, 40),
            reference_count: get_u32(bytes, 44),
            label_bytes: get_u32(bytes, 48),
            body_hash,
            index_hash: stored_index_hash,
        },
        sections,
    ))
}

fn validate_sections(
    sections: &[SectionRange; 5],
    header: ProductIndexHeader,
    file_len: u64,
) -> Result<(), ProductIndexError> {
    let expected = [
        u64::from(header.node_count) * size_of::<NodeProductRecord>() as u64,
        u64::from(header.edge_count) * size_of::<EdgeProductRecord>() as u64,
        u64::from(header.mapping_count) * size_of::<EntityNodeMappingRecord>() as u64,
        u64::from(header.reference_count) * size_of::<ProductReferenceRecord>() as u64,
        u64::from(header.label_bytes),
    ];
    if u64::from(header.label_bytes) > MAX_LABEL_BYTES {
        return Err(ProductIndexError::OversizedLabels {
            actual: u64::from(header.label_bytes),
            limit: MAX_LABEL_BYTES,
        });
    }
    if header.reference_count > MAX_REFERENCE_RECORDS {
        return Err(ProductIndexError::Oversized {
            actual: u64::from(header.reference_count),
            limit: u64::from(MAX_REFERENCE_RECORDS),
        });
    }
    let mut cursor = HEADER_SIZE as u64;
    for (index, (section, expected_len)) in sections.iter().zip(expected).enumerate() {
        if section.offset != cursor || section.len != expected_len {
            return Err(ProductIndexError::InvalidSection(section_name(index)));
        }
        cursor = cursor
            .checked_add(section.len)
            .ok_or(ProductIndexError::RangeOverflow)?;
    }
    if cursor != file_len {
        return Err(ProductIndexError::CorruptHeader("section length mismatch"));
    }
    Ok(())
}

const fn section_name(index: usize) -> &'static str {
    ["nodes", "edges", "mappings", "references", "labels"][index]
}

fn get_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap_or([0; 4]))
}

fn get_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap_or([0; 8]))
}
