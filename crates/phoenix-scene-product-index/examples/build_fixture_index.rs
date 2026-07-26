use phoenix_scene_archive::{ArchiveManifold, PhoenixSceneArchiveV1};
use phoenix_scene_product_index::{
    EdgeProductRecord, NodeProductRecord, PhoenixSceneProductIndexBuilderV1, ProductIndexBinding,
    ReviewState,
};
use std::error::Error;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn Error>> {
    let mut arguments = std::env::args_os().skip(1);
    let archive_path = arguments
        .next()
        .map(PathBuf::from)
        .ok_or("usage: build_fixture_index <archive.psa> <output.pspi>")?;
    let output_path = arguments
        .next()
        .map(PathBuf::from)
        .ok_or("usage: build_fixture_index <archive.psa> <output.pspi>")?;
    if arguments.next().is_some() {
        return Err("usage: build_fixture_index <archive.psa> <output.pspi>".into());
    }

    let archive = PhoenixSceneArchiveV1::open(&archive_path)?;
    let pages = archive.open_manifold(ArchiveManifold::Hybrid)?;
    let mut builder =
        PhoenixSceneProductIndexBuilderV1::new(ProductIndexBinding::from_archive(&archive));
    for (identity, style) in pages.identities.iter().zip(pages.styles) {
        builder.push_node(
            NodeProductRecord {
                node_id: identity.id,
                family_mask: bit(style.kind)?,
                scope_mask: u64::MAX,
                review_mask: ReviewState::Accepted as u32,
                label_offset: 0,
                label_len: 0,
                inspector_ref: u32::MAX,
                provenance_ref: u32::MAX,
                reserved: 0,
            },
            "",
        )?;
    }
    for edge in pages.edges {
        builder.push_edge(EdgeProductRecord {
            edge_id: edge.id,
            family_mask: bit(edge.kind)?,
            scope_mask: u64::MAX,
            relation_mask: bit(edge.kind)?,
            review_mask: ReviewState::Accepted as u32,
            inspector_ref: u32::MAX,
            provenance_ref: u32::MAX,
            reserved: 0,
        });
    }
    let receipt = builder.write_to_path(&output_path)?;
    println!(
        "fixture product index generation={} nodes={} edges={} mappings=0 hash={}",
        receipt.binding.archive_generation,
        receipt.node_count,
        receipt.edge_count,
        hex(&receipt.index_hash)
    );
    Ok(())
}

fn bit(kind: u16) -> Result<u64, Box<dyn Error>> {
    1_u64
        .checked_shl(u32::from(kind))
        .ok_or_else(|| format!("family/relation kind {kind} exceeds the 64-bit mask").into())
}

fn hex(bytes: &[u8; 32]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(64);
    for byte in bytes {
        value.push(DIGITS[(byte >> 4) as usize] as char);
        value.push(DIGITS[(byte & 0xf) as usize] as char);
    }
    value
}
