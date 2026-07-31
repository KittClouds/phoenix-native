use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};

use tempfile::tempdir;

use crate::{
    write_embedding_pages_new, EmbeddingPageError, EmbeddingPageExpectation,
    EmbeddingPageWriteAuthority, EmbeddingRowV1, VerifiedEmbeddingPagesV1, ROW_FLAG_NORMALIZED,
};

fn authority() -> EmbeddingPageWriteAuthority {
    EmbeddingPageWriteAuthority {
        generation_hash: [1; 32],
        source_set_hash: [2; 32],
        model_identity_hash: [3; 32],
        model_asset_hash: [4; 32],
        config_hash: [5; 32],
        dimension: 4,
    }
}

fn rows() -> [EmbeddingRowV1; 2] {
    [
        EmbeddingRowV1 {
            subject_id: 10,
            source_id: 1,
            content_hash: [7; 32],
            vector_start: 0,
            source_start: 0,
            source_end: 4,
            ordinal: 0,
            dimension: 4,
            source_kind: 1,
            content_kind: 5,
            flags: ROW_FLAG_NORMALIZED,
            reserved: [0; 2],
        },
        EmbeddingRowV1 {
            subject_id: 20,
            source_id: 2,
            content_hash: [8; 32],
            vector_start: 4,
            source_start: 0,
            source_end: 5,
            ordinal: 1,
            dimension: 4,
            source_kind: 2,
            content_kind: 7,
            flags: ROW_FLAG_NORMALIZED,
            reserved: [0; 2],
        },
    ]
}

#[test]
fn writes_and_reopens_zero_copy_pages() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("test.phxe1");
    let vectors = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let pages = write_embedding_pages_new(&path, authority(), &rows(), &vectors).unwrap();

    assert_eq!(pages.rows().unwrap().len(), 2);
    assert_eq!(pages.vector(1).unwrap(), Some(&vectors[4..]));
    assert_eq!(
        pages.row_by_subject(20).unwrap().map(|value| value.0),
        Some(1)
    );

    let reopened = VerifiedEmbeddingPagesV1::open_expected(
        &path,
        EmbeddingPageExpectation {
            generation_hash: Some([1; 32]),
            source_set_hash: Some([2; 32]),
            model_identity_hash: Some([3; 32]),
            config_hash: Some([5; 32]),
        },
    )
    .unwrap();
    assert_eq!(
        reopened.header().artifact_hash,
        pages.header().artifact_hash
    );
}

#[test]
fn rejects_noncanonical_rows() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("bad.phxe1");
    let mut bad = rows();
    bad[1].subject_id = bad[0].subject_id;
    let vectors = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let error = write_embedding_pages_new(&path, authority(), &bad, &vectors).unwrap_err();
    assert!(matches!(error, EmbeddingPageError::NonCanonicalRows));
    assert!(!path.exists());
}

#[test]
fn corruption_fails_closed() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("corrupt.phxe1");
    let vectors = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let pages = write_embedding_pages_new(&path, authority(), &rows(), &vectors).unwrap();
    let offset = pages.header().vectors_offset;
    drop(pages);

    let mut file = OpenOptions::new().write(true).open(&path).unwrap();
    file.seek(SeekFrom::Start(offset)).unwrap();
    file.write_all(&[0x7f]).unwrap();
    file.sync_all().unwrap();

    let error = VerifiedEmbeddingPagesV1::open(&path).unwrap_err();
    assert!(matches!(
        error,
        EmbeddingPageError::HashMismatch { page: "vectors" }
    ));
}
