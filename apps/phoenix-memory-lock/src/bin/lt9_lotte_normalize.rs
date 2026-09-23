//! Corpus-only normalization for the frozen P1M2R LoTTE input.
//! It reads the ten named collection TSV members and never opens query/QAS files.

use anyhow::{ensure, Context, Result};
use hashbrown::HashSet;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

const EXPECTED_ARCHIVE_BYTES: u64 = 3_576_167_599;
const DOMAINS: [&str; 5] = [
    "writing",
    "recreation",
    "science",
    "technology",
    "lifestyle",
];

#[derive(Serialize)]
struct SourceFile {
    path: String,
    sha256: String,
    bytes: u64,
    input_rows: u64,
    duplicate_ids_skipped: u64,
}

#[derive(Serialize)]
struct CorpusFile {
    corpus_id: String,
    path: String,
    sha256: String,
    bytes: u64,
    unique_documents: u64,
    sources: Vec<SourceFile>,
}

#[derive(Serialize)]
struct Receipt {
    schema: &'static str,
    date: &'static str,
    scope: &'static str,
    archive_url: &'static str,
    archive_path: String,
    archive_sha256: String,
    archive_bytes: u64,
    expected_archive_bytes: u64,
    validity_labels_opened: bool,
    queries_or_qas_opened: bool,
    extracted_members: Vec<String>,
    corpora: Vec<CorpusFile>,
}

#[derive(Serialize)]
struct CorpusRecord<'a> {
    docid: &'a str,
    title: &'static str,
    text: &'a str,
}

fn normalize_source<R: BufRead, W: Write>(
    reader: &mut R,
    path: &Path,
    writer: &mut W,
    seen: &mut HashSet<String>,
) -> Result<SourceFile> {
    let mut hasher = Sha256::new();
    let mut line = Vec::with_capacity(8192);
    let mut input_rows = 0u64;
    let mut duplicate_ids_skipped = 0u64;
    let mut bytes = 0u64;
    loop {
        line.clear();
        let count = reader.read_until(b'\n', &mut line)?;
        if count == 0 {
            break;
        }
        bytes += count as u64;
        hasher.update(&line);
        while matches!(line.last(), Some(b'\n' | b'\r')) {
            line.pop();
        }
        if line.is_empty() {
            continue;
        }
        let row = std::str::from_utf8(&line)
            .with_context(|| format!("UTF-8 row in {}", path.display()))?;
        let (docid, text) = row
            .split_once('\t')
            .with_context(|| format!("missing TSV separator in {}", path.display()))?;
        ensure!(!docid.is_empty(), "empty passage ID in {}", path.display());
        input_rows += 1;
        if !seen.insert(docid.to_owned()) {
            duplicate_ids_skipped += 1;
            continue;
        }
        serde_json::to_writer(
            &mut *writer,
            &CorpusRecord {
                docid,
                title: "",
                text,
            },
        )?;
        writer.write_all(b"\n")?;
    }
    Ok(SourceFile {
        path: path.display().to_string(),
        sha256: format!("{:x}", hasher.finalize()),
        bytes,
        input_rows,
        duplicate_ids_skipped,
    })
}

fn hash_file(path: &Path) -> Result<(String, u64)> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut reader = BufReader::with_capacity(1 << 20, file);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    let mut bytes = 0u64;
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
        bytes += count as u64;
    }
    Ok((format!("{:x}", hasher.finalize()), bytes))
}

fn normalize_domain(domain: &str, extract_root: &Path, output_root: &Path) -> Result<CorpusFile> {
    let output_path = output_root.join(format!("lotte-{domain}.jsonl"));
    let output_file =
        File::create(&output_path).with_context(|| format!("create {}", output_path.display()))?;
    let mut writer = BufWriter::with_capacity(1 << 20, output_file);
    let mut seen = HashSet::<String>::with_capacity(250_000);
    let mut sources = Vec::with_capacity(2);
    let mut unique_documents = 0u64;

    for split in ["dev", "test"] {
        let input_path = extract_root
            .join("lotte")
            .join(domain)
            .join(split)
            .join("collection.tsv");
        let input_file = File::open(&input_path)
            .with_context(|| format!("open collection member {}", input_path.display()))?;
        let mut reader = BufReader::with_capacity(1 << 20, input_file);
        let source = normalize_source(&mut reader, &input_path, &mut writer, &mut seen)?;
        unique_documents += source.input_rows - source.duplicate_ids_skipped;
        sources.push(source);
    }

    ensure!(
        unique_documents > 0,
        "LoTTE {domain} has no unique collection rows"
    );
    writer.flush()?;
    drop(writer);
    let (sha256, bytes) = hash_file(&output_path)?;
    Ok(CorpusFile {
        corpus_id: format!("lotte-{domain}"),
        path: output_path.display().to_string(),
        sha256,
        bytes,
        unique_documents,
        sources,
    })
}

fn run(args: &[String]) -> Result<()> {
    ensure!(
        args.len() == 5 && args[0] == "--normalize",
        "usage: lt9_lotte_normalize --normalize <receipt.json> <lotte.tar.gz> <extract-root> <output-dir>"
    );
    let receipt_path = PathBuf::from(&args[1]);
    let archive_path = PathBuf::from(&args[2]);
    let extract_root = PathBuf::from(&args[3]);
    let output_root = PathBuf::from(&args[4]);

    let (archive_sha256, archive_bytes) = hash_file(&archive_path)?;
    ensure!(
        archive_bytes == EXPECTED_ARCHIVE_BYTES,
        "LoTTE archive byte length drift: expected {EXPECTED_ARCHIVE_BYTES}, got {archive_bytes}"
    );
    fs::create_dir_all(&output_root)?;
    let mut corpora = Vec::with_capacity(DOMAINS.len());
    let mut extracted_members = Vec::with_capacity(DOMAINS.len() * 2);
    for domain in DOMAINS {
        for split in ["dev", "test"] {
            extracted_members.push(format!("lotte/{domain}/{split}/collection.tsv"));
        }
        corpora.push(normalize_domain(domain, &extract_root, &output_root)?);
    }

    let receipt = Receipt {
        schema: "phoenix.lexical.lt9-la2-p1m2r-normalization/v1",
        date: "2026-09-23",
        scope: "LoTTE corpus-only normalization; dev then test; first passage ID wins; pooled, query, QAS, and judgment members excluded",
        archive_url: "https://downloads.cs.stanford.edu/nlp/data/colbert/colbertv2/lotte.tar.gz",
        archive_path: archive_path.display().to_string(),
        archive_sha256,
        archive_bytes,
        expected_archive_bytes: EXPECTED_ARCHIVE_BYTES,
        validity_labels_opened: false,
        queries_or_qas_opened: false,
        extracted_members,
        corpora,
    };
    if let Some(parent) = receipt_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(receipt_path, serde_json::to_vec_pretty(&receipt)?)?;
    Ok(())
}

fn main() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    run(&args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn preserves_first_duplicate_and_text_with_embedded_tab() {
        let mut seen = HashSet::new();
        let mut output = Vec::new();
        let mut dev = Cursor::new(b"p1\tfirst text\np2\tleft\tright\n".to_vec());
        let dev_stats = normalize_source(
            &mut dev,
            Path::new("dev/collection.tsv"),
            &mut output,
            &mut seen,
        )
        .unwrap();
        let mut test = Cursor::new(b"p1\tduplicate ignored\np3\tlast text\n".to_vec());
        let test_stats = normalize_source(
            &mut test,
            Path::new("test/collection.tsv"),
            &mut output,
            &mut seen,
        )
        .unwrap();
        let rows = std::str::from_utf8(&output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0]["text"], "first text");
        assert_eq!(rows[1]["text"], "left\tright");
        assert_eq!(rows[2]["docid"], "p3");
        assert_eq!(dev_stats.input_rows, 2);
        assert_eq!(test_stats.duplicate_ids_skipped, 1);
    }
}
