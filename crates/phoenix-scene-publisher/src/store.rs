use crate::manifest::MANIFEST_FILE;
use crate::{
    NativeScenePublication, ScenePublicationError, ScenePublicationKind, ScenePublicationReceipt,
};
use phoenix_scene_archive::{
    ArchiveManifold, PageKey, PageKind, PhoenixSceneArchiveBuilderV1, PhoenixSceneArchiveV1,
};
use phoenix_scene_contract::{DocumentId, ResidentScene, SceneSource};
use phoenix_scene_product_index::{
    EdgeProductRecord, NodeProductRecord, PhoenixSceneProductIndexBuilderV1,
    PhoenixSceneProductIndexV1, ProductIndexBinding,
};
use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use windows::core::PCWSTR;
use windows::Win32::Storage::FileSystem::{
    MoveFileExW, ReplaceFileW, MOVEFILE_WRITE_THROUGH, REPLACEFILE_WRITE_THROUGH,
};

const STORE_DIRECTORY: &str = "scene-publications-v1";

#[derive(Debug)]
pub struct PublishedScene {
    pub scene: Arc<ResidentScene>,
    pub product_index: Arc<PhoenixSceneProductIndexV1>,
    pub receipt: ScenePublicationReceipt,
}

#[derive(Debug)]
pub struct ScenePublicationStore {
    root: PathBuf,
    nonce: AtomicU64,
}

impl ScenePublicationStore {
    pub fn for_workspace(workspace_path: &Path) -> Result<Self, ScenePublicationError> {
        let parent = workspace_path.parent().ok_or_else(|| {
            ScenePublicationError::WorkspacePathWithoutParent(workspace_path.to_path_buf())
        })?;
        Ok(Self {
            root: parent.join(STORE_DIRECTORY),
            nonce: AtomicU64::new(1),
        })
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn next_generation(&self) -> Result<u64, ScenePublicationError> {
        self.read_manifest()?.map_or(Ok(1), |receipt| {
            receipt
                .generation_id
                .checked_add(1)
                .ok_or(ScenePublicationError::RangeOverflow)
        })
    }

    pub fn current_receipt(
        &self,
    ) -> Result<Option<ScenePublicationReceipt>, ScenePublicationError> {
        self.read_manifest()
    }

    pub fn open_current(&self) -> Result<Option<PublishedScene>, ScenePublicationError> {
        self.read_manifest()?
            .map(|receipt| self.open_receipt(receipt))
            .transpose()
    }

    pub fn publish(
        &self,
        publication: NativeScenePublication,
    ) -> Result<PublishedScene, ScenePublicationError> {
        publication.validate()?;
        if let Some(current) = self.read_manifest()? {
            if current.kind == ScenePublicationKind::Full
                && publication.kind == ScenePublicationKind::RegistryOnly
            {
                return Err(ScenePublicationError::FullGenerationProtected {
                    current: current.generation_id,
                    incoming: publication.generation_id,
                });
            }
            if publication.generation_id <= current.generation_id {
                return Err(ScenePublicationError::StaleGeneration {
                    current: current.generation_id,
                    incoming: publication.generation_id,
                });
            }
        }
        fs::create_dir_all(&self.root).map_err(|source| io(&self.root, source))?;
        let nonce = self.nonce.fetch_add(1, Ordering::Relaxed);
        let stem = generation_stem(publication.generation_id);
        let archive_final = self.root.join(format!("{stem}.psa"));
        let index_final = self.root.join(format!("{stem}.pspi"));
        ensure_absent(&archive_final)?;
        ensure_absent(&index_final)?;
        let archive_pending = self.pending_path(&stem, nonce, "psa");
        let index_pending = self.pending_path(&stem, nonce, "pspi");

        let built = self.build_pair(publication, &archive_pending, &index_pending);
        let (receipt, source) = match built {
            Ok(value) => value,
            Err(error) => {
                remove_pending(&archive_pending);
                remove_pending(&index_pending);
                return Err(error);
            }
        };
        move_new_file(&archive_final, &archive_pending)?;
        if let Err(error) = move_new_file(&index_final, &index_pending) {
            remove_pending(&index_pending);
            return Err(error);
        }
        self.commit_manifest(receipt)?;
        self.open_receipt_with_source(receipt, source)
    }

    fn build_pair(
        &self,
        publication: NativeScenePublication,
        archive_path: &Path,
        index_path: &Path,
    ) -> Result<(ScenePublicationReceipt, SceneSource), ScenePublicationError> {
        let kind = publication.kind;
        let source = source_for_kind(kind);
        let registry_revision = publication.registry_revision;
        let document_id = publication.document_id;
        let entity_count = publication.entity_mappings.len() as u64;
        let generation_id = publication.generation_id;
        let node_count = publication.identities.len() as u64;
        let edge_count = publication.edges.len() as u64;

        let mut archive = PhoenixSceneArchiveBuilderV1::new(generation_id)?;
        archive
            .add_records(
                PageKey::shared(PageKind::NodeIdentity),
                &publication.identities,
            )?
            .add_records(PageKey::shared(PageKind::NodeStyle), &publication.styles)?
            .add_records(PageKey::shared(PageKind::Topology), &publication.topology)?
            .add_records(PageKey::shared(PageKind::Edge), &publication.edges)?;
        for (manifold, positions) in ArchiveManifold::ALL.into_iter().zip(&publication.positions) {
            archive.add_records(PageKey::manifold(PageKind::Positions, manifold), positions)?;
        }
        let archive_receipt = archive.write_to_path(archive_path)?;
        let opened_archive = PhoenixSceneArchiveV1::open(archive_path)?;

        let mut index = PhoenixSceneProductIndexBuilderV1::new(ProductIndexBinding::from_archive(
            &opened_archive,
        ));
        for product in publication.node_products {
            index.push_node(
                NodeProductRecord {
                    node_id: product.node_id,
                    family_mask: product.family_mask,
                    scope_mask: product.scope_mask,
                    review_mask: product.review_mask,
                    label_offset: 0,
                    label_len: 0,
                    inspector_ref: product.inspector_ref,
                    provenance_ref: product.provenance_ref,
                    reserved: 0,
                },
                &product.label,
            )?;
        }
        for product in publication.edge_products {
            index.push_edge(EdgeProductRecord {
                edge_id: product.edge_id,
                family_mask: product.family_mask,
                scope_mask: product.scope_mask,
                relation_mask: product.relation_mask,
                review_mask: product.review_mask,
                inspector_ref: product.inspector_ref,
                provenance_ref: product.provenance_ref,
                reserved: 0,
            });
        }
        for mapping in publication.entity_mappings {
            index.push_mapping(mapping);
        }
        for reference in publication.references {
            index.push_reference(reference)?;
        }
        let index_receipt = index.write_to_path(index_path)?;
        let opened_index = PhoenixSceneProductIndexV1::open(index_path)?;
        opened_index.bind_to_archive(&opened_archive)?;

        Ok((
            ScenePublicationReceipt {
                generation_id,
                kind,
                registry_revision,
                document_id,
                node_count,
                edge_count,
                entity_count,
                archive_bytes: archive_receipt.file_len,
                product_index_bytes: index_receipt.file_len,
                archive_cohort_hash: archive_receipt.cohort_hash,
                product_index_hash: index_receipt.index_hash,
            },
            source,
        ))
    }

    fn open_receipt(
        &self,
        receipt: ScenePublicationReceipt,
    ) -> Result<PublishedScene, ScenePublicationError> {
        self.open_receipt_with_source(receipt, source_for_kind(receipt.kind))
    }

    fn open_receipt_with_source(
        &self,
        receipt: ScenePublicationReceipt,
        source: SceneSource,
    ) -> Result<PublishedScene, ScenePublicationError> {
        let stem = generation_stem(receipt.generation_id);
        let archive_path = self.root.join(format!("{stem}.psa"));
        let index_path = self.root.join(format!("{stem}.pspi"));
        ensure_present(&archive_path)?;
        ensure_present(&index_path)?;
        let archive = Arc::new(PhoenixSceneArchiveV1::open(&archive_path)?);
        let product_index = Arc::new(PhoenixSceneProductIndexV1::open(&index_path)?);
        product_index.bind_to_archive(&archive)?;
        verify_receipt(receipt, &archive, &product_index)?;
        let document = receipt.document_id.map(DocumentId);
        let scene = Arc::new(ResidentScene::from_archive_with_source(
            archive, document, source,
        )?);
        Ok(PublishedScene {
            scene,
            product_index,
            receipt,
        })
    }

    fn read_manifest(&self) -> Result<Option<ScenePublicationReceipt>, ScenePublicationError> {
        let path = self.root.join(MANIFEST_FILE);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(io(&path, source)),
        };
        ScenePublicationReceipt::decode(&bytes).map(Some)
    }

    fn commit_manifest(
        &self,
        receipt: ScenePublicationReceipt,
    ) -> Result<(), ScenePublicationError> {
        let manifest = self.root.join(MANIFEST_FILE);
        let nonce = self.nonce.fetch_add(1, Ordering::Relaxed);
        let pending = self.pending_path("current", nonce, "pspm");
        write_synced_new(&pending, &receipt.encode())?;
        let result = if manifest.exists() {
            replace_file(&manifest, &pending)
        } else {
            move_new_file(&manifest, &pending)
        };
        if result.is_err() {
            remove_pending(&pending);
        }
        result
    }

    fn pending_path(&self, stem: &str, nonce: u64, extension: &str) -> PathBuf {
        self.root.join(format!(
            ".{stem}.{}.{}.pending.{extension}",
            std::process::id(),
            nonce
        ))
    }
}

fn verify_receipt(
    receipt: ScenePublicationReceipt,
    archive: &PhoenixSceneArchiveV1,
    index: &PhoenixSceneProductIndexV1,
) -> Result<(), ScenePublicationError> {
    let archive_header = archive.header();
    let index_header = index.header();
    let pages = archive.open_manifold(ArchiveManifold::Hybrid)?;
    if archive_header.generation_id != receipt.generation_id
        || archive_header.cohort_hash != receipt.archive_cohort_hash
        || archive_header.file_len != receipt.archive_bytes
        || index_header.index_hash != receipt.product_index_hash
        || index_header.file_len != receipt.product_index_bytes
        || pages.identities.len() as u64 != receipt.node_count
        || pages.edges.len() as u64 != receipt.edge_count
        || index_header.mapping_count as u64 != receipt.entity_count
    {
        return Err(ScenePublicationError::CorruptManifest(
            "artifact receipt mismatch",
        ));
    }
    Ok(())
}

fn source_for_kind(kind: ScenePublicationKind) -> SceneSource {
    match kind {
        ScenePublicationKind::RegistryOnly => SceneSource::RegistryOnly,
        ScenePublicationKind::Full => SceneSource::Backend,
    }
}

fn generation_stem(generation: u64) -> String {
    format!("generation-{generation:016x}")
}

fn ensure_present(path: &Path) -> Result<(), ScenePublicationError> {
    if path.is_file() {
        Ok(())
    } else {
        Err(ScenePublicationError::MissingArtifact(path.to_path_buf()))
    }
}

fn ensure_absent(path: &Path) -> Result<(), ScenePublicationError> {
    if path.exists() {
        Err(ScenePublicationError::ArtifactAlreadyExists(
            path.to_path_buf(),
        ))
    } else {
        Ok(())
    }
}

fn write_synced_new(path: &Path, bytes: &[u8]) -> Result<(), ScenePublicationError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|source| io(path, source))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|source| io(path, source))
}

fn move_new_file(destination: &Path, source: &Path) -> Result<(), ScenePublicationError> {
    let destination_wide = wide_null(destination.as_os_str());
    let source_wide = wide_null(source.as_os_str());
    // SAFETY: Both UTF-16 buffers are NUL terminated and remain live for the call.
    unsafe {
        MoveFileExW(
            PCWSTR(source_wide.as_ptr()),
            PCWSTR(destination_wide.as_ptr()),
            MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(|source| ScenePublicationError::AtomicReplace {
        path: destination.to_path_buf(),
        source,
    })
}

fn replace_file(destination: &Path, replacement: &Path) -> Result<(), ScenePublicationError> {
    let destination_wide = wide_null(destination.as_os_str());
    let replacement_wide = wide_null(replacement.as_os_str());
    // SAFETY: Both UTF-16 buffers are NUL terminated and remain live for the call.
    unsafe {
        ReplaceFileW(
            PCWSTR(destination_wide.as_ptr()),
            PCWSTR(replacement_wide.as_ptr()),
            PCWSTR::null(),
            REPLACEFILE_WRITE_THROUGH,
            None,
            None,
        )
    }
    .map_err(|source| ScenePublicationError::AtomicReplace {
        path: destination.to_path_buf(),
        source,
    })
}

fn wide_null(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}

fn remove_pending(path: &Path) {
    if path.exists() {
        let _ = fs::remove_file(path);
    }
}

fn io(path: &Path, source: std::io::Error) -> ScenePublicationError {
    ScenePublicationError::Io {
        path: path.to_path_buf(),
        source,
    }
}
