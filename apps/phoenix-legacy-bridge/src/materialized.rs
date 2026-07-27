use anyhow::{bail, Context, Result};
use hashbrown::{HashMap, HashSet};
use phoenix_scene_archive::{
    ArchiveManifold, EdgeRecord, NodeIdentityRecord, NodeStyleRecord, PositionRecord,
    TopologyRecord,
};
use phoenix_scene_compiler::project_node_positions;
use phoenix_scene_contract::{
    EntityKind, FamilyMask, RelationFamily, ReviewMask, ScopeMask, CAPS_WORLD_SCALE,
    CHUNK_NODE_KIND, DOCUMENT_NODE_KIND, EPISODE_NODE_KIND,
};
use phoenix_scene_product_index::EntityNodeMappingRecord;
use phoenix_scene_publisher::{
    NativeScenePublication, SceneEdgeProduct, SceneNodeProduct, ScenePublicationKind,
    ScenePublicationStore,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::sync::Arc;

const MATERIALIZED_CONTRACT: &str = "PhoenixAngularMaterializedSceneV1";
const NO_REFERENCE: u32 = u32::MAX;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MaterializedScene {
    format: String,
    key: String,
    cohort: Cohort,
    source_mode: String,
    manifold_mode: String,
    nodes: Vec<LegacyNode>,
    edges: Vec<LegacyEdge>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Cohort {
    note_id: String,
    note_sha256: String,
    snapshot_id: String,
    authority_hash: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyNode {
    id: String,
    label: String,
    kind: String,
    #[serde(default)]
    total_mentions: u32,
    position: [f32; 3],
    color_hsl: String,
    source_id: Option<String>,
    family: Option<String>,
    style_key: Option<String>,
    review: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyEdge {
    id: String,
    source_id: String,
    target_id: String,
    #[serde(rename = "type")]
    edge_type: String,
    confidence: f32,
    family: Option<String>,
    review: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PublishReceipt {
    contract: &'static str,
    note_id: String,
    note_sha256: String,
    snapshot_id: String,
    authority_hash: String,
    generation_id: u64,
    node_count: usize,
    edge_count: usize,
    entity_mapping_count: usize,
    archive_cohort_hash: String,
    product_index_hash: String,
    publication_root: String,
}

pub(crate) fn publish(scene_path: &Path, root: &Path) -> Result<PublishReceipt> {
    let bytes =
        fs::read(scene_path).with_context(|| format!("read scene {}", scene_path.display()))?;
    let scene: MaterializedScene = serde_json::from_slice(&bytes)?;
    validate_scene(&scene)?;
    let store = ScenePublicationStore::at_root(root);
    let generation_id = store.next_generation()?;
    let publication = compile(&scene, generation_id)?;
    let entity_mapping_count = publication.entity_mappings.len();
    let published = store.publish(publication)?;
    Ok(PublishReceipt {
        contract: "phoenix.native.legacy-materialized-bridge/v1",
        note_id: scene.cohort.note_id,
        note_sha256: scene.cohort.note_sha256,
        snapshot_id: scene.cohort.snapshot_id,
        authority_hash: scene.cohort.authority_hash,
        generation_id,
        node_count: scene.nodes.len(),
        edge_count: scene.edges.len(),
        entity_mapping_count,
        archive_cohort_hash: hex(published.receipt.archive_cohort_hash),
        product_index_hash: hex(published.receipt.product_index_hash),
        publication_root: root.display().to_string(),
    })
}

fn validate_scene(scene: &MaterializedScene) -> Result<()> {
    if scene.format != MATERIALIZED_CONTRACT {
        bail!("unsupported materialized scene format {}", scene.format);
    }
    if scene.key != "caps" || scene.source_mode != "embeddings" || scene.manifold_mode != "lorentz"
    {
        bail!("materialized bridge requires the verified Caps embeddings page");
    }
    if scene.cohort.note_id.is_empty()
        || scene.cohort.note_sha256.len() != 64
        || scene.cohort.snapshot_id.is_empty()
        || scene.cohort.authority_hash.is_empty()
    {
        bail!("materialized scene cohort identity is incomplete");
    }
    if scene.nodes.is_empty() || scene.edges.is_empty() {
        bail!("materialized scene has no graph geometry");
    }
    Ok(())
}

fn compile(scene: &MaterializedScene, generation_id: u64) -> Result<NativeScenePublication> {
    let mut node_ids = HashMap::with_capacity(scene.nodes.len());
    let mut node_slots = HashMap::with_capacity(scene.nodes.len());
    let mut stable_nodes = HashMap::<u64, &str>::with_capacity(scene.nodes.len());
    for (slot, node) in scene.nodes.iter().enumerate() {
        let stable = stable_id(b"phoenix.legacy.node/v1\0", &node.id);
        if let Some(existing) = stable_nodes.insert(stable, &node.id) {
            bail!(
                "node identity collision between {existing:?} and {:?}",
                node.id
            );
        }
        if node_ids.insert(node.id.as_str(), stable).is_some() {
            bail!("duplicate node identity {:?}", node.id);
        }
        node_slots.insert(node.id.as_str(), slot);
    }
    let mut degrees = HashMap::<u64, u32>::with_capacity(node_ids.len());
    for edge in &scene.edges {
        let source = *node_ids
            .get(edge.source_id.as_str())
            .with_context(|| format!("edge {:?} source is missing", edge.id))?;
        let target = *node_ids
            .get(edge.target_id.as_str())
            .with_context(|| format!("edge {:?} target is missing", edge.id))?;
        let source_degree = degrees.entry(source).or_default();
        *source_degree = source_degree.saturating_add(1);
        let target_degree = degrees.entry(target).or_default();
        *target_degree = target_degree.saturating_add(1);
    }

    let mut identities = Vec::with_capacity(scene.nodes.len());
    let mut styles = Vec::with_capacity(scene.nodes.len());
    let mut node_products = Vec::with_capacity(scene.nodes.len());
    let mut mappings = Vec::new();
    let mut positions: [Vec<PositionRecord>; 5] =
        std::array::from_fn(|_| Vec::with_capacity(scene.nodes.len()));
    for (ordinal, node) in scene.nodes.iter().enumerate() {
        let id = node_ids[node.id.as_str()];
        let family = node_family(node);
        let degree = degrees.get(&id).copied().unwrap_or_default();
        identities.push(NodeIdentityRecord { id });
        styles.push(NodeStyleRecord {
            color: parse_hsl(&node.color_hsl)?,
            radius: node_radius(node, degree),
            kind: node_kind(node),
            flags: 0,
        });
        node_products.push(SceneNodeProduct {
            node_id: id,
            family_mask: family,
            scope_mask: ScopeMask::NOTE.0,
            review_mask: review_mask(node.review.as_deref()),
            label: Arc::from(node.label.as_str()),
            inspector_ref: NO_REFERENCE,
            provenance_ref: NO_REFERENCE,
        });
        if node.kind == "entity" {
            let source = node.source_id.as_deref().unwrap_or(node.id.as_str());
            mappings.push(EntityNodeMappingRecord {
                entity_id: stable_id(b"phoenix.legacy.entity/v1\0", source),
                node_id: id,
            });
        }
        let projected = project_node_positions(
            id,
            ordinal,
            scene.nodes.len(),
            family.trailing_zeros().min(u32::from(u16::MAX)) as u16,
            degree,
        );
        for (page, position) in positions.iter_mut().zip(projected) {
            page.push(position);
        }
        positions[ArchiveManifold::Caps as usize][ordinal] = PositionRecord {
            position: node.position.map(|value| value * CAPS_WORLD_SCALE),
        };
    }

    let mut edge_ids = HashSet::with_capacity(scene.edges.len());
    let mut topology = Vec::with_capacity(scene.edges.len());
    let mut edges = Vec::with_capacity(scene.edges.len());
    let mut edge_products = Vec::with_capacity(scene.edges.len());
    for edge in &scene.edges {
        let id = stable_id(b"phoenix.legacy.edge/v1\0", &edge.id);
        if !edge_ids.insert(id) {
            bail!("duplicate or colliding edge identity {:?}", edge.id);
        }
        let source_id = node_ids[edge.source_id.as_str()];
        let target_id = node_ids[edge.target_id.as_str()];
        let relation = relation_family(edge);
        topology.push(TopologyRecord {
            source_id,
            target_id,
        });
        let source_slot = node_slots[edge.source_id.as_str()];
        let target_slot = node_slots[edge.target_id.as_str()];
        edges.push(EdgeRecord {
            id,
            color: edge_color(styles[source_slot].color, styles[target_slot].color),
            width: (0.24 + edge.confidence.clamp(0.0, 1.0) * 0.12).min(0.36),
            kind: relation as u16,
            flags: 0,
        });
        edge_products.push(SceneEdgeProduct {
            edge_id: id,
            family_mask: edge_family(edge),
            scope_mask: ScopeMask::NOTE.0,
            relation_mask: relation.mask().0,
            review_mask: review_mask(edge.review.as_deref()),
            inspector_ref: NO_REFERENCE,
            provenance_ref: NO_REFERENCE,
        });
    }

    Ok(NativeScenePublication {
        generation_id,
        kind: ScenePublicationKind::Full,
        registry_revision: 1,
        document_id: Some(stable_id(
            b"phoenix.legacy.document/v1\0",
            &scene.cohort.note_id,
        )),
        identities,
        styles,
        topology,
        edges,
        positions,
        node_products,
        edge_products,
        entity_mappings: mappings,
        references: Vec::new(),
    })
}

fn stable_id(domain: &[u8], value: &str) -> u64 {
    let mut hasher = blake3::Hasher::new();
    hasher.update(domain);
    hasher.update(value.as_bytes());
    let mut raw = [0_u8; 8];
    raw.copy_from_slice(&hasher.finalize().as_bytes()[..8]);
    u64::from_le_bytes(raw).max(1)
}

fn node_kind(node: &LegacyNode) -> u16 {
    match node.kind.as_str() {
        "note" => DOCUMENT_NODE_KIND,
        "episode" => EPISODE_NODE_KIND,
        "chunk" => CHUNK_NODE_KIND,
        "entity" => match node.style_key.as_deref() {
            Some("CHARACTER") => EntityKind::Character as u16,
            Some("LOCATION") => EntityKind::Location as u16,
            Some("NPC") => EntityKind::Npc as u16,
            Some("NETWORK" | "FACTION") => EntityKind::Faction as u16,
            _ => EntityKind::Custom as u16,
        },
        "event" => EntityKind::Event as u16,
        _ => EntityKind::Custom as u16,
    }
}

fn node_family(node: &LegacyNode) -> u64 {
    match node.family.as_deref() {
        Some("structure") => FamilyMask::STRUCTURE.0,
        Some("discourse") => FamilyMask::DISCOURSE.0,
        Some("fact" | "temporal" | "causal" | "memory") => FamilyMask::FACTS.0,
        _ => match node.style_key.as_deref() {
            Some("CHARACTER" | "NPC") => 1 << 0,
            Some("LOCATION") => 1 << 1,
            Some("NETWORK" | "FACTION") => 1 << 2,
            Some("ITEM") => 1 << 3,
            Some("CONCEPT") => 1 << 4,
            Some("EVENT") => 1 << 5,
            _ => 1 << 7,
        },
    }
}

fn edge_family(edge: &LegacyEdge) -> u64 {
    match edge.family.as_deref() {
        Some("structure") => FamilyMask::STRUCTURE.0,
        Some("discourse") => FamilyMask::DISCOURSE.0,
        Some("fact" | "temporal" | "causal" | "memory") => FamilyMask::FACTS.0,
        _ => FamilyMask::ENTITIES.0,
    }
}

fn relation_family(edge: &LegacyEdge) -> RelationFamily {
    match edge.family.as_deref() {
        Some("temporal") => RelationFamily::Temporal,
        Some("causal") => RelationFamily::Causal,
        Some("structure") => RelationFamily::Structural,
        Some("discourse") => RelationFamily::Communication,
        Some("registry") => RelationFamily::CoOccurrence,
        _ => match edge.edge_type.as_str() {
            "before" => RelationFamily::Temporal,
            "causes_or_explains" => RelationFamily::Causal,
            "co_occurs_with" | "anchored-cooccurrence" => RelationFamily::CoOccurrence,
            _ => RelationFamily::Observation,
        },
    }
}

fn review_mask(review: Option<&str>) -> u32 {
    match review {
        Some("accepted") => ReviewMask::ACCEPTED.0,
        Some("rejected") => ReviewMask::REJECTED.0,
        _ => ReviewMask::PROPOSED.0,
    }
}

fn node_radius(node: &LegacyNode, degree: u32) -> f32 {
    match node.kind.as_str() {
        "note" => 1.4,
        "episode" => 1.25,
        "chunk" => 0.8,
        "structure-root" => 1.0,
        "entity" => {
            (0.38 + (degree as f32 + node.total_mentions as f32 + 1.0).ln() * 0.16).min(0.9)
        }
        _ => 0.46,
    }
}

fn parse_hsl(value: &str) -> Result<[f32; 4]> {
    let fields = value.split_ascii_whitespace().collect::<Vec<_>>();
    if fields.len() != 3 {
        bail!("invalid HSL color {value:?}");
    }
    let hue = fields[0].parse::<f32>()?.rem_euclid(360.0) / 360.0;
    let saturation = percent(fields[1])?;
    let lightness = percent(fields[2])?;
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let sector = hue * 6.0;
    let x = chroma * (1.0 - (sector.rem_euclid(2.0) - 1.0).abs());
    let (red, green, blue) = match sector as u32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let offset = lightness - chroma * 0.5;
    Ok([red + offset, green + offset, blue + offset, 1.0])
}

fn percent(value: &str) -> Result<f32> {
    let parsed = value
        .strip_suffix('%')
        .context("HSL percentage is missing '%'")?
        .parse::<f32>()?;
    Ok((parsed / 100.0).clamp(0.0, 1.0))
}

fn edge_color(left: [f32; 4], right: [f32; 4]) -> [f32; 4] {
    [
        (left[0] + right[0]) * 0.42,
        (left[1] + right[1]) * 0.42,
        (left[2] + right[2]) * 0.42,
        0.24,
    ]
}

fn hex(bytes: [u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsl_parser_preserves_saturated_red_without_pastel_shift() {
        let color = parse_hsl("0 98% 50%").expect("valid HSL");
        assert!((color[0] - 0.99).abs() < 0.001);
        assert!((color[1] - 0.01).abs() < 0.001);
        assert!((color[2] - 0.01).abs() < 0.001);
        assert_eq!(color[3], 1.0);
    }

    #[test]
    fn materialized_scene_compiles_one_shared_identity_set() {
        let scene = MaterializedScene {
            format: MATERIALIZED_CONTRACT.into(),
            key: "caps".into(),
            cohort: Cohort {
                note_id: "note-a".into(),
                note_sha256: "00".repeat(32),
                snapshot_id: "snapshot-a".into(),
                authority_hash: "authority-a".into(),
            },
            source_mode: "embeddings".into(),
            manifold_mode: "lorentz".into(),
            nodes: vec![
                LegacyNode {
                    id: "embed:entity:a".into(),
                    label: "A".into(),
                    kind: "entity".into(),
                    total_mentions: 4,
                    position: [0.2, 0.3, 0.4],
                    color_hsl: "160 90% 45%".into(),
                    source_id: Some("entity-a".into()),
                    family: Some("registry".into()),
                    style_key: Some("CHARACTER".into()),
                    review: Some("accepted".into()),
                },
                LegacyNode {
                    id: "embed:chunk:0".into(),
                    label: "Chunk 0".into(),
                    kind: "chunk".into(),
                    total_mentions: 1,
                    position: [-0.2, 0.1, 0.3],
                    color_hsl: "330 90% 60%".into(),
                    source_id: Some("chunk-0".into()),
                    family: Some("structure".into()),
                    style_key: Some("chunk".into()),
                    review: Some("accepted".into()),
                },
            ],
            edges: vec![LegacyEdge {
                id: "edge-a".into(),
                source_id: "embed:entity:a".into(),
                target_id: "embed:chunk:0".into(),
                edge_type: "chunk-entity".into(),
                confidence: 0.8,
                family: Some("registry".into()),
                review: Some("accepted".into()),
            }],
        };
        let publication = compile(&scene, 7).expect("compile");
        assert_eq!(publication.identities.len(), 2);
        assert_eq!(publication.edges.len(), 1);
        assert_eq!(publication.entity_mappings.len(), 1);
        assert!(publication
            .positions
            .iter()
            .all(|page| page.len() == publication.identities.len()));
        assert_eq!(
            publication.positions[ArchiveManifold::Caps as usize][0].position,
            [8.0, 12.0, 16.0]
        );
        assert_ne!(
            publication.positions[ArchiveManifold::Hybrid as usize][0],
            publication.positions[ArchiveManifold::Caps as usize][0]
        );
    }
}
