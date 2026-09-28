use super::{safe_component, sha256_id, Result};
use kammi_client::KammiClient;
use phoenix_scene_publisher::{
    ScenePublicationKind, ScenePublicationReceipt, ScenePublicationStore,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SceneAsset {
    pub kind: &'static str,
    pub path: PathBuf,
    pub artifact_id: String,
}

impl SceneAsset {
    pub fn from_path(kind: &'static str, path: PathBuf) -> Result<Self> {
        if !safe_component(kind) {
            return Err("invalid scene asset kind".into());
        }
        let artifact_id = file_sha256_id(&path)?;
        Ok(Self {
            kind,
            path,
            artifact_id,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SceneAssetPlan {
    pub generation_id: u64,
    pub receipt: ScenePublicationReceipt,
    pub assets: Vec<SceneAsset>,
}

impl SceneAssetPlan {
    /// `kernel_receipt` must come from the kernel's verified full publication,
    /// including compiler and visual authority. This reopens the immutable
    /// archive/index pair and refuses a changed current pointer. It does not
    /// replace the kernel's semantic qualification.
    pub fn from_kernel_verified_receipt(
        root: &Path,
        kernel_receipt: ScenePublicationReceipt,
    ) -> Result<Self> {
        if kernel_receipt.kind != ScenePublicationKind::Full {
            return Err("only verified full scenes may enter the vault".into());
        }
        let published = ScenePublicationStore::open_current_at(root)?
            .ok_or("no current Phoenix scene publication")?;
        if published.receipt != kernel_receipt {
            return Err("current scene differs from kernel verified receipt".into());
        }
        let generation_id = kernel_receipt.generation_id;
        let stem = format!("generation-{generation_id:016x}");
        let files = [
            ("scene-pointer", root.join("current.pspm")),
            ("scene-archive", root.join(format!("{stem}.psa"))),
            ("scene-index", root.join(format!("{stem}.pspi"))),
            ("compiler-v2", root.join(format!("{stem}.phxcav2"))),
            ("compiler-v3", root.join(format!("{stem}.phxcav3"))),
        ];
        let mut assets = Vec::with_capacity(files.len());
        for (kind, path) in files {
            assets.push(SceneAsset::from_path(kind, path)?);
        }
        Ok(Self {
            generation_id,
            receipt: kernel_receipt,
            assets,
        })
    }
}

pub trait SceneVaultTransport {
    fn vault_view(&self, vault_id: &str, actor_id: &str) -> Result<Value>;
    fn upload_asset(
        &self,
        path: &Path,
        vault_id: &str,
        actor_id: &str,
        kind: &str,
        request_id: &str,
    ) -> Result<Value>;
    fn select_generation(&self, body: &Value) -> Result<Value>;
}

impl SceneVaultTransport for KammiClient {
    fn vault_view(&self, vault_id: &str, actor_id: &str) -> Result<Value> {
        KammiClient::vault_view(self, vault_id, actor_id)
    }
    fn upload_asset(
        &self,
        path: &Path,
        vault_id: &str,
        actor_id: &str,
        kind: &str,
        request_id: &str,
    ) -> Result<Value> {
        self.vault_upload(path, vault_id, actor_id, kind, request_id)
    }
    fn select_generation(&self, body: &Value) -> Result<Value> {
        self.vault_select_generation(body)
    }
}

/// Stage exact immutable scene files, then select only against the source
/// epoch observed after upload. A concurrent source edit leaves staged CAS
/// objects unselected. This is a fixture path until the kernel hook is wired.
pub fn stage_verified_scene<T: SceneVaultTransport>(
    transport: &T,
    vault_id: &str,
    actor_id: &str,
    source_epoch: u64,
    plan: &SceneAssetPlan,
) -> Result<Value> {
    if plan.receipt.kind != ScenePublicationKind::Full
        || plan.receipt.generation_id != plan.generation_id
        || source_epoch > (1_u64 << 53) - 1
        || plan.generation_id > (1_u64 << 53) - 1
        || !safe_component(vault_id)
        || !safe_component(actor_id)
        || source_epoch == 0
        || plan.assets.is_empty()
        || plan.assets.len() > 64
    {
        return Err("invalid scene vault selection input".into());
    }
    let mut asset_ids = Vec::with_capacity(plan.assets.len());
    for asset in &plan.assets {
        if !safe_component(asset.kind) || file_sha256_id(&asset.path)? != asset.artifact_id {
            return Err("scene asset changed since plan creation".into());
        }
        let request_id = asset_request_id(vault_id, asset.kind, &asset.artifact_id);
        let receipt =
            transport.upload_asset(&asset.path, vault_id, actor_id, asset.kind, &request_id)?;
        if receipt.get("artifact_id").and_then(Value::as_str) != Some(asset.artifact_id.as_str()) {
            return Err("Library asset receipt differs from scene bytes".into());
        }
        asset_ids.push(asset.artifact_id.clone());
    }
    asset_ids.sort_unstable();
    asset_ids.dedup();
    let manifest = json!({
        "schema": "PHOENIX_VAULT_GENERATION_V1",
        "vault_id": vault_id,
        "source_epoch": source_epoch,
        "generation_id": plan.generation_id,
        "asset_ids": asset_ids,
    });
    let manifest_bytes = serde_json::to_vec(&manifest)?;
    let manifest_id = sha256_id(&manifest_bytes);
    let mut file = tempfile::NamedTempFile::new()?;
    file.write_all(&manifest_bytes)?;
    file.as_file_mut().sync_all()?;
    let manifest_receipt = transport.upload_asset(
        file.path(),
        vault_id,
        actor_id,
        "manifest",
        &asset_request_id(vault_id, "manifest", &manifest_id),
    )?;
    if manifest_receipt.get("artifact_id").and_then(Value::as_str) != Some(manifest_id.as_str()) {
        return Err("Library manifest receipt differs from exact bytes".into());
    }
    let view = transport.vault_view(vault_id, actor_id)?;
    if view.get("owner_actor").and_then(Value::as_str) != Some(actor_id)
        || view.get("source_epoch").and_then(Value::as_u64) != Some(source_epoch)
    {
        return Err("vault source epoch advanced during scene staging".into());
    }
    let selection = json!({
        "actor_id": actor_id,
        "vault_id": vault_id,
        "source_epoch": source_epoch,
        "generation_id": plan.generation_id,
        "manifest_artifact_id": manifest_id,
        "asset_ids": asset_ids,
        "request_id": format!("phoenix-generation-{}-{}-{}",
            plan.generation_id, source_epoch, &manifest_id[7..]),
    });
    let result = transport.select_generation(&selection)?;
    let receipt = result
        .get("generation")
        .ok_or("generation receipt missing")?;
    if receipt.get("generation_id").and_then(Value::as_u64) != Some(plan.generation_id)
        || receipt.get("source_epoch").and_then(Value::as_u64) != Some(source_epoch)
        || receipt.get("manifest_artifact_id").and_then(Value::as_str)
            != selection
                .get("manifest_artifact_id")
                .and_then(Value::as_str)
    {
        return Err("Library generation receipt differs from selected scene".into());
    }
    Ok(result)
}

fn file_sha256_id(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn asset_request_id(vault_id: &str, kind: &str, artifact_id: &str) -> String {
    let mut hasher = Sha256::new();
    for field in [vault_id, kind, artifact_id] {
        hasher.update((field.len() as u64).to_le_bytes());
        hasher.update(field.as_bytes());
    }
    format!("phoenix-asset-{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct Fake {
        epoch: u64,
        selected: RefCell<bool>,
    }
    impl SceneVaultTransport for Fake {
        fn vault_view(&self, _: &str, _: &str) -> Result<Value> {
            Ok(json!({"owner_actor":"phoenix-service", "source_epoch":self.epoch}))
        }
        fn upload_asset(&self, path: &Path, _: &str, _: &str, _: &str, _: &str) -> Result<Value> {
            Ok(json!({"artifact_id":file_sha256_id(path)?}))
        }
        fn select_generation(&self, body: &Value) -> Result<Value> {
            *self.selected.borrow_mut() = true;
            Ok(json!({"generation": {
                "generation_id":body["generation_id"],
                "source_epoch":body["source_epoch"],
                "manifest_artifact_id":body["manifest_artifact_id"]
            }}))
        }
    }

    #[test]
    fn changed_source_epoch_never_selects_staged_assets() -> Result<()> {
        let root = tempfile::tempdir()?;
        let path = root.path().join("asset");
        std::fs::write(&path, b"scene")?;
        let plan = SceneAssetPlan {
            generation_id: 2,
            receipt: ScenePublicationReceipt {
                generation_id: 2,
                kind: ScenePublicationKind::Full,
                registry_revision: 1,
                document_id: Some(1),
                node_count: 1,
                edge_count: 0,
                entity_count: 0,
                archive_bytes: 0,
                product_index_bytes: 0,
                archive_cohort_hash: [0; 32],
                product_index_hash: [0; 32],
            },
            assets: vec![SceneAsset {
                kind: "scene-archive",
                path: path.clone(),
                artifact_id: file_sha256_id(&path)?,
            }],
        };
        let fake = Fake {
            epoch: 2,
            selected: RefCell::new(false),
        };
        assert!(stage_verified_scene(&fake, "vault.test", "phoenix-service", 1, &plan).is_err());
        assert!(!*fake.selected.borrow());
        Ok(())
    }
}
