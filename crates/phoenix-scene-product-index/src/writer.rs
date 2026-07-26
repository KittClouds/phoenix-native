use crate::format::{
    index_hash, ProductIndexBinding, SectionRange, FORMAT_VERSION, HEADER_SIZE, MAGIC,
    MAX_INDEX_BYTES, MAX_LABEL_BYTES, MAX_REFERENCE_RECORDS,
};
use crate::{
    EdgeProductRecord, EntityNodeMappingRecord, NodeProductRecord, ProductIndexError,
    ProductReferenceRecord,
};
use std::fs::OpenOptions;
use std::io::{BufWriter, Write};
use std::path::Path;

#[derive(Debug)]
pub struct PhoenixSceneProductIndexBuilderV1 {
    binding: ProductIndexBinding,
    nodes: Vec<NodeProductRecord>,
    edges: Vec<EdgeProductRecord>,
    mappings: Vec<EntityNodeMappingRecord>,
    references: Vec<ProductReferenceRecord>,
    labels: Vec<u8>,
}

impl PhoenixSceneProductIndexBuilderV1 {
    #[must_use]
    pub fn new(binding: ProductIndexBinding) -> Self {
        Self {
            binding,
            nodes: Vec::new(),
            edges: Vec::new(),
            mappings: Vec::new(),
            references: Vec::new(),
            labels: Vec::new(),
        }
    }

    pub fn push_node(
        &mut self,
        mut record: NodeProductRecord,
        label: &str,
    ) -> Result<&mut Self, ProductIndexError> {
        let offset =
            u32::try_from(self.labels.len()).map_err(|_| ProductIndexError::RangeOverflow)?;
        let len = u32::try_from(label.len()).map_err(|_| ProductIndexError::RangeOverflow)?;
        let new_len = self
            .labels
            .len()
            .checked_add(label.len())
            .ok_or(ProductIndexError::RangeOverflow)?;
        if new_len as u64 > MAX_LABEL_BYTES {
            return Err(ProductIndexError::OversizedLabels {
                actual: new_len as u64,
                limit: MAX_LABEL_BYTES,
            });
        }
        record.label_offset = offset;
        record.label_len = len;
        self.labels.extend_from_slice(label.as_bytes());
        self.nodes.push(record);
        Ok(self)
    }

    pub fn push_edge(&mut self, record: EdgeProductRecord) -> &mut Self {
        self.edges.push(record);
        self
    }

    pub fn push_mapping(&mut self, record: EntityNodeMappingRecord) -> &mut Self {
        self.mappings.push(record);
        self
    }

    pub fn push_reference(
        &mut self,
        record: ProductReferenceRecord,
    ) -> Result<&mut Self, ProductIndexError> {
        if self.references.len() >= MAX_REFERENCE_RECORDS as usize {
            return Err(ProductIndexError::Oversized {
                actual: self.references.len() as u64 + 1,
                limit: u64::from(MAX_REFERENCE_RECORDS),
            });
        }
        self.references.push(record);
        Ok(self)
    }

    pub fn write_to_path(
        mut self,
        path: impl AsRef<Path>,
    ) -> Result<ProductIndexBuildReceipt, ProductIndexError> {
        self.mappings
            .sort_unstable_by_key(|mapping| mapping.entity_id);
        validate_mapping_order(&self.mappings)?;

        let node_count =
            u32::try_from(self.nodes.len()).map_err(|_| ProductIndexError::RangeOverflow)?;
        let edge_count =
            u32::try_from(self.edges.len()).map_err(|_| ProductIndexError::RangeOverflow)?;
        let mapping_count =
            u32::try_from(self.mappings.len()).map_err(|_| ProductIndexError::RangeOverflow)?;
        let reference_count =
            u32::try_from(self.references.len()).map_err(|_| ProductIndexError::RangeOverflow)?;
        let label_bytes =
            u32::try_from(self.labels.len()).map_err(|_| ProductIndexError::RangeOverflow)?;

        let mut body = Vec::new();
        let nodes = append_records(&mut body, &self.nodes)?;
        let edges = append_records(&mut body, &self.edges)?;
        let mappings = append_records(&mut body, &self.mappings)?;
        let references = append_records(&mut body, &self.references)?;
        let labels = append_bytes(&mut body, &self.labels)?;
        let file_len = (HEADER_SIZE as u64)
            .checked_add(body.len() as u64)
            .ok_or(ProductIndexError::RangeOverflow)?;
        if file_len > MAX_INDEX_BYTES {
            return Err(ProductIndexError::Oversized {
                actual: file_len,
                limit: MAX_INDEX_BYTES,
            });
        }

        let body_hash = *blake3::hash(&body).as_bytes();
        let index_hash = index_hash(
            self.binding,
            node_count,
            edge_count,
            mapping_count,
            reference_count,
            label_bytes,
            body_hash,
        );
        let header = encode_header(
            self.binding,
            file_len,
            [nodes, edges, mappings, references, labels],
            [
                node_count,
                edge_count,
                mapping_count,
                reference_count,
                label_bytes,
            ],
            body_hash,
            index_hash,
        );
        let path = path.as_ref();
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    ProductIndexError::AlreadyExists(path.to_path_buf())
                } else {
                    ProductIndexError::Io(error)
                }
            })?;
        let mut writer = BufWriter::new(file);
        writer.write_all(&header)?;
        writer.write_all(&body)?;
        writer.flush()?;
        writer.get_ref().sync_all()?;
        Ok(ProductIndexBuildReceipt {
            binding: self.binding,
            file_len,
            node_count,
            edge_count,
            mapping_count,
            reference_count,
            label_bytes,
            index_hash,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProductIndexBuildReceipt {
    pub binding: ProductIndexBinding,
    pub file_len: u64,
    pub node_count: u32,
    pub edge_count: u32,
    pub mapping_count: u32,
    pub reference_count: u32,
    pub label_bytes: u32,
    pub index_hash: [u8; 32],
}

fn append_records<T: bytemuck::Pod>(
    body: &mut Vec<u8>,
    records: &[T],
) -> Result<SectionRange, ProductIndexError> {
    append_bytes(body, bytemuck::cast_slice(records))
}

fn append_bytes(body: &mut Vec<u8>, bytes: &[u8]) -> Result<SectionRange, ProductIndexError> {
    let offset = (HEADER_SIZE as u64)
        .checked_add(body.len() as u64)
        .ok_or(ProductIndexError::RangeOverflow)?;
    body.extend_from_slice(bytes);
    Ok(SectionRange {
        offset,
        len: bytes.len() as u64,
    })
}

fn validate_mapping_order(mappings: &[EntityNodeMappingRecord]) -> Result<(), ProductIndexError> {
    let mut nodes = hashbrown::HashSet::with_capacity(mappings.len());
    for (slot, mapping) in mappings.iter().enumerate() {
        if mapping.entity_id == 0 {
            return Err(ProductIndexError::ZeroEntityIdentity);
        }
        if slot > 0 && mappings[slot - 1].entity_id == mapping.entity_id {
            return Err(ProductIndexError::DuplicateEntity(mapping.entity_id));
        }
        if !nodes.insert(mapping.node_id) {
            return Err(ProductIndexError::DuplicateMappedNode(mapping.node_id));
        }
    }
    Ok(())
}

fn encode_header(
    binding: ProductIndexBinding,
    file_len: u64,
    sections: [SectionRange; 5],
    counts: [u32; 5],
    body_hash: [u8; 32],
    index_hash: [u8; 32],
) -> [u8; HEADER_SIZE] {
    let mut bytes = [0_u8; HEADER_SIZE];
    bytes[0..8].copy_from_slice(&MAGIC);
    put_u32(&mut bytes, 8, FORMAT_VERSION);
    put_u32(&mut bytes, 12, HEADER_SIZE as u32);
    put_u64(&mut bytes, 16, binding.archive_generation);
    put_u64(&mut bytes, 24, file_len);
    for (index, count) in counts.into_iter().enumerate() {
        put_u32(&mut bytes, 32 + index * 4, count);
    }
    bytes[56..88].copy_from_slice(&binding.archive_cohort_hash);
    bytes[88..120].copy_from_slice(&body_hash);
    bytes[120..152].copy_from_slice(&index_hash);
    for (index, section) in sections.into_iter().enumerate() {
        put_u64(&mut bytes, 152 + index * 16, section.offset);
        put_u64(&mut bytes, 160 + index * 16, section.len);
    }
    bytes
}

fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}
