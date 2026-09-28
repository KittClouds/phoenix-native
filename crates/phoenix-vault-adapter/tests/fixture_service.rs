//! Explicit, ignored product fixture against the accepted local Library.
//! Never point this at a user's live Phoenix workspace or service actor.

use kammi_client::KammiClient;
use phoenix_reader_session::{plan_markdown, PlannerConfig, ReaderSession};
use phoenix_scene_publisher::{ScenePublicationKind, ScenePublicationReceipt};
use phoenix_vault_adapter::{
    saved_sources, stage_verified_scene, MirrorCursorStore, MirrorOutbox, Result, SavedSource,
    SceneAsset, SceneAssetPlan,
};
use phoenix_workspace::{commit_document, open_document, EntryKind, WorkspaceDocument, ROOT_ID};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use uuid::Uuid;

#[test]
#[ignore = "requires an explicitly provisioned fixture service actor"]
fn local_library_source_reader_and_stream_fixture() -> Result<()> {
    let token_path = std::env::var("PHOENIX_VAULT_FIXTURE_TOKEN_FILE")?;
    let actor_id = std::env::var("PHOENIX_VAULT_FIXTURE_ACTOR")?;
    let endpoint = std::env::var("PHOENIX_VAULT_FIXTURE_ENDPOINT")?;
    let token = fs::read_to_string(token_path)?;
    let client = KammiClient::new(&endpoint, token.trim())?;
    let vault_id = format!("phoenix.fixture.{}", Uuid::new_v4().simple());
    client.vault_create(&serde_json::json!({
        "actor_id":actor_id, "vault_id":vault_id,
        "request_id":format!("{vault_id}.create"),
    }))?;

    let fixture = tempfile::tempdir()?;
    let workspace_path = fixture.path().join("workspace.json");
    let mut workspace = WorkspaceDocument::seeded();
    let small = workspace.create(ROOT_ID, EntryKind::Note, "Reader fixture")?;
    let large = workspace.create(ROOT_ID, EntryKind::Note, "Stream fixture")?;
    workspace.save_atomic(&workspace_path)?;
    let small_lease = commit_document(
        &workspace_path,
        &workspace,
        open_document(&workspace_path, &workspace, small)?.token(),
        "One sentence. Two sentences.",
    )?;
    let large_text = "x".repeat(16 * 1024 * 1024);
    commit_document(
        &workspace_path,
        &workspace,
        open_document(&workspace_path, &workspace, large)?.token(),
        &large_text,
    )?;
    drop(large_text);

    let outbox = MirrorOutbox::at(fixture.path().join("outbox"));
    let cursor_store = MirrorCursorStore::at(
        fixture.path().join("mirror-cursor.json"),
        &vault_id,
        &actor_id,
    )?;
    let sources = saved_sources(&workspace_path)?;
    assert_eq!(sources.len(), 3);
    for (sequence, source) in sources.iter().enumerate() {
        outbox.enqueue(sequence as u64 + 1, &vault_id, &actor_id, source, 0)?;
    }
    for _ in 0..sources.len() {
        let ack = outbox.flush_next(&client)?.ok_or("missing queued source")?;
        // Replaying before the local acknowledgement is durable must return
        // the same Library event and not increment its epoch.
        let repeated = outbox
            .flush_next(&client)?
            .ok_or("missing replayed source")?;
        assert_eq!(ack, repeated);
        cursor_store.record_and_complete(&outbox, &ack)?;
    }
    let cursor = cursor_store.load()?;
    assert_eq!(cursor.source_epoch, 3);
    assert!(outbox.pending()?.is_empty());
    let view = client.vault_view(&vault_id, &actor_id)?;
    assert_eq!(view["source_epoch"], 3);
    assert_eq!(
        view["sources"]
            .as_object()
            .ok_or("missing source view")?
            .len(),
        3
    );

    let large_source = sources
        .iter()
        .find(|source| source.bytes.len() == 16 * 1024 * 1024)
        .ok_or("large fixture source missing")?;
    let mut response = client.vault_source_bytes(&vault_id, &large_source.source_id, &actor_id)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut total = 0_u64;
    loop {
        let count = response.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
        total += count as u64;
    }
    assert_eq!(total, 16 * 1024 * 1024);
    assert_eq!(
        format!("sha256:{:x}", hasher.finalize()),
        large_source.artifact_id
    );

    let plan = plan_markdown([1; 32], &small_lease, PlannerConfig::default())?.plan;
    let session = ReaderSession::new([2; 32], &plan, [3; 32])?;
    let locator = phoenix_vault_adapter::reader_locator_from_book_session(&plan, &session)?;
    let request = locator.vault_request(&cursor, &vault_id, &actor_id)?;
    let reader = client.vault_set_reader(&request)?;
    assert_eq!(reader["reader"]["revision"], 1);
    assert_eq!(reader["reader"]["offset"], request["offset"]);

    // Wire fixture only: these bytes are not a qualified Phoenix scene.
    // Production construction requires a kernel-verified publication receipt.
    let scene_path = fixture.path().join("synthetic-scene.bin");
    fs::write(&scene_path, vec![b's'; 11 * 1024 * 1024])?;
    let scene = SceneAssetPlan {
        generation_id: 1,
        receipt: ScenePublicationReceipt {
            generation_id: 1,
            kind: ScenePublicationKind::Full,
            registry_revision: 1,
            document_id: Some(small.0),
            node_count: 1,
            edge_count: 0,
            entity_count: 0,
            archive_bytes: 0,
            product_index_bytes: 0,
            archive_cohort_hash: [0; 32],
            product_index_hash: [0; 32],
        },
        assets: vec![SceneAsset::from_path("scene-fixture", scene_path)?],
    };
    stage_verified_scene(&client, &vault_id, &actor_id, 3, &scene)?;
    assert_eq!(
        client.vault_view(&vault_id, &actor_id)?["generation_current"],
        true
    );

    let mut package = client.vault_package(&vault_id, &actor_id)?;
    let package_root = package
        .headers()
        .get("x-vault-package-root")
        .ok_or("portable package root missing")?;
    assert!(package_root.to_str()?.starts_with("sha256:"));
    let mut exported = tempfile::NamedTempFile::new()?;
    std::io::copy(&mut package, &mut exported)?;
    assert!(fs::metadata(exported.path())?.len() > 0);

    let updated = commit_document(
        &workspace_path,
        &workspace,
        small_lease.token(),
        "One sentence. A changed ending.",
    )?;
    let changed = SavedSource::new(
        phoenix_vault_adapter::source_id(small),
        updated.revision.0,
        updated.content.as_bytes().to_vec(),
    )?;
    outbox.enqueue(4, &vault_id, &actor_id, &changed, 1)?;
    let changed_ack = outbox
        .flush_next(&client)?
        .ok_or("changed note not queued")?;
    cursor_store.record_and_complete(&outbox, &changed_ack)?;
    let final_view = client.vault_view(&vault_id, &actor_id)?;
    assert_eq!(final_view["source_epoch"], 4);
    assert_eq!(final_view["generation_current"], false);
    assert_eq!(final_view["reader"]["revision"], 1);
    println!("fixture_vault={vault_id} sources=3 source_epoch=4 scene_stale=true reader=ok package_root=present");
    Ok(())
}
