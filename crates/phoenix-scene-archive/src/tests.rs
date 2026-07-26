use crate::{
    ArchiveError, ArchiveManifold, LabelPriorityRecord, NodeIdentityRecord, PageKey, PageKind,
    PaletteEntryRecord, PhoenixSceneArchiveBuilderV1, PhoenixSceneArchiveV1, RelationMaskRecord,
    MAX_ARCHIVE_BYTES, MAX_PAGE_BYTES,
};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const EXPECTED_HASH: [u8; 32] = [
    0x5b, 0xc5, 0x49, 0x2b, 0xb9, 0xfe, 0x44, 0x0e, 0x9a, 0xb8, 0x94, 0x0e, 0x5c, 0x69, 0xe0, 0x37,
    0x85, 0xd4, 0x91, 0x6b, 0xd7, 0xa5, 0x43, 0xcf, 0x67, 0xd9, 0x39, 0x96, 0x9f, 0xc0, 0x4a, 0xfe,
];

#[path = "../tests/support/cohort.rs"]
mod cohort;
use cohort::FrozenCohort;

#[test]
fn test_only_freezer_reproduces_the_frozen_archive() -> Result<(), ArchiveError> {
    let path = temp_path("reproduced-cohort");
    assert_eq!(cohort::COHORT_NAME, "phoenix-cleanroom-10k-50k-v1");
    let receipt = cohort::freeze(&path)?;
    assert_eq!(receipt.generation_id, cohort::GENERATION_ID);
    assert_eq!(receipt.page_count, cohort::PAGE_COUNT);
    assert_eq!(receipt.cohort_hash, EXPECTED_HASH);
    let expected = fs::read(fixture_path())?;
    let actual = fs::read(&path)?;
    assert_eq!(actual, expected);
    remove(&path);
    Ok(())
}

#[test]
fn frozen_cohort_matches_every_shared_and_manifold_record() -> Result<(), ArchiveError> {
    let archive = PhoenixSceneArchiveV1::open(fixture_path())?;
    assert_eq!(archive.header().generation_id, cohort::GENERATION_ID);
    assert_eq!(archive.header().page_count, cohort::PAGE_COUNT);
    assert_eq!(archive.header().cohort_hash, EXPECTED_HASH);
    let expected = FrozenCohort::generate();

    for (index, manifold) in ArchiveManifold::ALL.into_iter().enumerate() {
        let pages = archive.open_manifold(manifold)?;
        assert_eq!(pages.identities, expected.identities);
        assert_eq!(pages.styles, expected.styles);
        assert_eq!(pages.topology, expected.topology);
        assert_eq!(pages.edges, expected.edges);
        assert_eq!(pages.positions, expected.positions[index]);
    }
    assert_eq!(
        archive.typed_page::<LabelPriorityRecord>(PageKey::shared(PageKind::LabelPriority))?,
        expected.labels
    );
    assert_eq!(
        archive.typed_page::<RelationMaskRecord>(PageKey::shared(PageKind::RelationMasks))?,
        expected.relations
    );
    assert_eq!(
        archive.typed_page::<PaletteEntryRecord>(PageKey::shared(PageKind::PalettePolicy))?,
        expected.palette
    );
    Ok(())
}

#[test]
fn shared_pages_exist_once_and_optional_pages_are_lazy() -> Result<(), ArchiveError> {
    let archive = PhoenixSceneArchiveV1::open(fixture_path())?;
    assert_eq!(archive.verified_page_count(), 0);
    archive.open_manifold(ArchiveManifold::Hybrid)?;
    assert_eq!(archive.verified_page_count(), 5);
    archive.open_manifold(ArchiveManifold::Hybrid)?;
    assert_eq!(archive.verified_page_count(), 5);
    archive.open_manifold(ArchiveManifold::Transit)?;
    assert_eq!(archive.verified_page_count(), 6);
    archive.guides(ArchiveManifold::Transit)?;
    assert_eq!(archive.verified_page_count(), 7);

    for kind in shared_kinds() {
        assert_eq!(
            archive
                .descriptors()
                .iter()
                .filter(|descriptor| descriptor.key.kind == kind)
                .count(),
            1
        );
    }
    assert!(archive.verified_page_count() < u64::from(archive.header().page_count));
    Ok(())
}

#[test]
fn prepared_path_and_guide_pages_are_well_formed() -> Result<(), ArchiveError> {
    let archive = PhoenixSceneArchiveV1::open(fixture_path())?;
    for manifold in ArchiveManifold::ALL {
        let guides = archive.guides(manifold)?;
        assert_eq!(guides.strokes.len(), 8);
        assert_eq!(guides.points.len(), 520);
        for (kind, style, points_per_edge) in [
            (PageKind::StraightPaths, 0, 2),
            (PageKind::CurvedPaths, 1, 3),
            (PageKind::BundledPaths, 2, 4),
        ] {
            let paths = archive.paths(kind, manifold)?;
            assert_eq!(paths.style, style);
            assert_eq!(paths.paths.len(), cohort::EDGE_COUNT);
            assert_eq!(paths.points.len(), cohort::EDGE_COUNT * points_per_edge);
        }
    }
    Ok(())
}

#[test]
fn missing_and_corrupt_pages_fail_closed() -> Result<(), ArchiveError> {
    let missing_path = temp_path("missing-page");
    let mut builder = PhoenixSceneArchiveBuilderV1::new(7)?;
    builder.add_records(
        PageKey::shared(PageKind::NodeIdentity),
        &[NodeIdentityRecord { id: 1 }],
    )?;
    builder.write_to_path(&missing_path)?;
    let missing = PhoenixSceneArchiveV1::open(&missing_path)?;
    assert!(matches!(
        missing.open_manifold(ArchiveManifold::Hybrid),
        Err(ArchiveError::MissingPage(_))
    ));
    remove(&missing_path);

    let corrupt_path = copied_fixture("corrupt-page")?;
    let archive = PhoenixSceneArchiveV1::open(&corrupt_path)?;
    let position = archive
        .descriptors()
        .iter()
        .find(|descriptor| {
            descriptor.key == PageKey::manifold(PageKind::Positions, ArchiveManifold::Hybrid)
        })
        .copied()
        .ok_or(ArchiveError::MissingPage(PageKey::manifold(
            PageKind::Positions,
            ArchiveManifold::Hybrid,
        )))?;
    drop(archive);
    write_bytes(&corrupt_path, position.offset, &[0xFF])?;
    let corrupt = PhoenixSceneArchiveV1::open(&corrupt_path)?;
    assert!(matches!(
        corrupt.open_manifold(ArchiveManifold::Hybrid),
        Err(ArchiveError::CorruptPage(key))
            if key == PageKey::manifold(PageKind::Positions, ArchiveManifold::Hybrid)
    ));
    remove(&corrupt_path);
    Ok(())
}

#[test]
fn corrupt_header_and_directory_fail_before_page_access() -> Result<(), ArchiveError> {
    let header_path = copied_fixture("corrupt-header")?;
    write_bytes(&header_path, 0, b"BADMAGIC")?;
    assert!(matches!(
        PhoenixSceneArchiveV1::open(&header_path),
        Err(ArchiveError::CorruptHeader(_))
    ));
    remove(&header_path);

    let directory_path = copied_fixture("corrupt-directory")?;
    write_bytes(&directory_path, 128, &[0xEE])?;
    assert!(matches!(
        PhoenixSceneArchiveV1::open(&directory_path),
        Err(ArchiveError::CorruptDirectory(_))
    ));
    remove(&directory_path);
    Ok(())
}

#[test]
fn unsupported_format_page_kind_and_page_version_are_named() -> Result<(), ArchiveError> {
    let format_path = copied_fixture("unsupported-format")?;
    write_bytes(&format_path, 8, &99_u32.to_le_bytes())?;
    assert!(matches!(
        PhoenixSceneArchiveV1::open(&format_path),
        Err(ArchiveError::UnsupportedFormatVersion(99))
    ));
    remove(&format_path);

    let kind_path = copied_fixture("unsupported-kind")?;
    write_bytes(&kind_path, 128, &99_u16.to_le_bytes())?;
    refresh_directory_hash(&kind_path)?;
    assert!(matches!(
        PhoenixSceneArchiveV1::open(&kind_path),
        Err(ArchiveError::UnsupportedPageKind(99))
    ));
    remove(&kind_path);

    let version_path = copied_fixture("unsupported-page-version")?;
    write_bytes(&version_path, 132, &99_u32.to_le_bytes())?;
    refresh_directory_hash(&version_path)?;
    assert!(matches!(
        PhoenixSceneArchiveV1::open(&version_path),
        Err(ArchiveError::UnsupportedPageVersion { version: 99, .. })
    ));
    remove(&version_path);
    Ok(())
}

#[test]
fn archive_and_page_size_limits_fail_before_mapping_or_hashing() -> Result<(), ArchiveError> {
    let archive_path = temp_path("oversized-archive");
    File::create(&archive_path)?.set_len(MAX_ARCHIVE_BYTES + 1)?;
    assert!(matches!(
        PhoenixSceneArchiveV1::open(&archive_path),
        Err(ArchiveError::OversizedArchive { .. })
    ));
    remove(&archive_path);

    let page_path = copied_fixture("oversized-page")?;
    write_bytes(&page_path, 144, &(MAX_PAGE_BYTES + 1).to_le_bytes())?;
    refresh_directory_hash(&page_path)?;
    assert!(matches!(
        PhoenixSceneArchiveV1::open(&page_path),
        Err(ArchiveError::OversizedPage { .. })
    ));
    remove(&page_path);
    Ok(())
}

fn shared_kinds() -> [PageKind; 7] {
    [
        PageKind::NodeIdentity,
        PageKind::NodeStyle,
        PageKind::Topology,
        PageKind::Edge,
        PageKind::LabelPriority,
        PageKind::RelationMasks,
        PageKind::PalettePolicy,
    ]
}

fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("phoenix-comparison-v1.psa")
}

fn copied_fixture(label: &str) -> Result<PathBuf, ArchiveError> {
    let path = temp_path(label);
    fs::copy(fixture_path(), &path)?;
    Ok(path)
}

fn refresh_directory_hash(path: &Path) -> Result<(), ArchiveError> {
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    let mut header = [0_u8; 128];
    file.read_exact(&mut header)?;
    let page_count = u32::from_le_bytes(header[24..28].try_into().unwrap_or([0; 4]));
    let mut directory = vec![0_u8; page_count as usize * 96];
    file.read_exact(&mut directory)?;
    let hash = blake3::hash(&directory);
    file.seek(SeekFrom::Start(88))?;
    file.write_all(hash.as_bytes())?;
    file.sync_all()?;
    Ok(())
}

fn write_bytes(path: &Path, offset: u64, bytes: &[u8]) -> Result<(), ArchiveError> {
    let mut file = OpenOptions::new().write(true).open(path)?;
    file.seek(SeekFrom::Start(offset))?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn temp_path(label: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    std::env::temp_dir().join(format!(
        "phoenix-scene-archive-{label}-{}-{}.psa",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

fn remove(path: &Path) {
    let _ = fs::remove_file(path);
}
