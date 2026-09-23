use super::*;

// This grouping is solely to keep duplicate masked templates on one candidate-
// specific observer's evaluation side. It never closes semantic SAME judgments.
pub(super) fn assign_template_splits(rows: &mut [PrivateLedgerRow]) {
    let mut parents: Vec<usize> = (0..rows.len()).collect();
    let mut by_template: HashMap<(String, String), usize> = HashMap::new();
    for (index, row) in rows.iter().enumerate() {
        for template in [&row.left_template_sha256, &row.right_template_sha256] {
            let key = (row.candidate_id.clone(), template.clone());
            if let Some(previous) = by_template.insert(key, index) {
                union_template_split_groups(&mut parents, index, previous);
            }
        }
    }
    let mut component_hash: HashMap<usize, String> = HashMap::new();
    for index in 0..rows.len() {
        let root = find_template_group(&mut parents, index);
        component_hash
            .entry(root)
            .and_modify(|value| {
                if rows[index].packet_id < *value {
                    *value = rows[index].packet_id.clone();
                }
            })
            .or_insert_with(|| rows[index].packet_id.clone());
    }
    for index in 0..rows.len() {
        let root = find_template_group(&mut parents, index);
        let split_key = format!("{}:{}", rows[index].candidate_id, component_hash[&root]);
        rows[index].split = if stable_hash(split_key.as_bytes()) % 4 == 0 {
            "holdout"
        } else {
            "fit"
        }
        .to_string();
    }
}

fn find_template_group(parents: &mut [usize], x: usize) -> usize {
    if parents[x] != x {
        let root = find_template_group(parents, parents[x]);
        parents[x] = root;
    }
    parents[x]
}

fn union_template_split_groups(parents: &mut [usize], a: usize, b: usize) {
    let ra = find_template_group(parents, a);
    let rb = find_template_group(parents, b);
    if ra != rb {
        parents[rb] = ra;
    }
}

pub(super) fn validate_packets(
    packets: &[ReviewPacket],
    ledger: &[PrivateLedgerRow],
) -> Result<()> {
    ensure!(
        packets.len() == ledger.len(),
        "packet-ledger length mismatch"
    );
    let mut ids = HashSet::new();
    let mut docs = HashSet::new();
    let mut templates_by_split: HashMap<(String, String), String> = HashMap::new();
    for (packet, row) in packets.iter().zip(ledger) {
        ensure!(
            ids.insert(packet.packet_id.as_str()),
            "duplicate opaque packet ID"
        );
        ensure!(
            packet.packet_id == row.packet_id,
            "packet order/ledger mismatch"
        );
        ensure!(
            packet.judgment.is_none(),
            "sampler assigned a semantic judgment"
        );
        ensure!(
            docs.insert(row.left_document_sha256.as_str()),
            "document reused in packet set"
        );
        ensure!(
            docs.insert(row.right_document_sha256.as_str()),
            "document reused in packet set"
        );
        for template in [&row.left_template_sha256, &row.right_template_sha256] {
            let key = (row.candidate_id.clone(), template.clone());
            if let Some(previous) = templates_by_split.insert(key, row.split.clone()) {
                ensure!(
                    previous == row.split,
                    "masked context template crossed fit/holdout split"
                );
            }
        }
        ensure!(
            row.split == "fit" || row.split == "holdout",
            "missing frozen split"
        );
    }
    Ok(())
}

pub(super) fn make_receipt(
    _roster: &Roster,
    roster_bytes: &[u8],
    counts: Vec<SourceCount>,
    rows: &[PrivateLedgerRow],
    docs: usize,
    token_leaks: usize,
) -> Result<Receipt> {
    let split_template_overlap = count_template_split_overlap(rows);
    ensure!(
        split_template_overlap == 0,
        "masked template crossed fit/holdout split"
    );
    let mut selected = [[0usize; 2]; 3];
    for row in rows {
        let candidate = RELATIONS
            .iter()
            .position(|r| r.id == row.candidate_id)
            .context("candidate id")?;
        let band = if row.overlap_band == "low" { 0 } else { 1 };
        selected[candidate][band] += 1;
    }
    Ok(Receipt {
        schema: "phoenix.lexical.lt9-la2-p1n3-pre-review/v1",
        date: DATE,
        status: "blind_packets_ready_no_judgments",
        source_roster_sha256: sha256(roster_bytes),
        source_hashes_verified: true,
        corpus_counts: counts,
        target_pairs_per_relation_band: TARGET_PER_BAND,
        max_pairs_per_corpus_band: MAX_PAIRS_PER_CORPUS_BAND,
        selected_pairs_per_relation_band: selected,
        review_packet_count: rows.len(),
        unique_document_count: docs,
        fit_packets: rows.iter().filter(|r| r.split == "fit").count(),
        holdout_packets: rows.iter().filter(|r| r.split == "holdout").count(),
        split_template_overlap,
        candidate_tokens_in_any_feature: token_leaks,
        judgment_values_assigned: 0,
        scope: Scope {
            qrels_or_queries_read: false,
            previous_outcomes_read: false,
            expected_family_labels_used: false,
            retrieval_or_ranking_run: false,
            authority_updated: false,
            human_judgments_assigned_by_sampler: false,
            reviewer_receives_provenance_or_strata: false,
        },
    })
}

fn count_template_split_overlap(rows: &[PrivateLedgerRow]) -> usize {
    let mut seen: HashMap<(String, String), &str> = HashMap::new();
    let mut conflicts = HashSet::new();
    for row in rows {
        for template in [&row.left_template_sha256, &row.right_template_sha256] {
            let key = (row.candidate_id.clone(), template.clone());
            if let Some(previous) = seen.insert(key.clone(), row.split.as_str()) {
                if previous != row.split {
                    conflicts.insert(key);
                }
            }
        }
    }
    conflicts.len()
}

pub(super) fn make_root(
    repo: &Path,
    protocol: &Path,
    roster: &Path,
    packets: &Path,
    rubric: &Path,
    ledger: &Path,
    receipt: &Path,
) -> Result<PreReviewRoot> {
    let binary = env::current_exe()?;
    let paths = vec![
        protocol.to_path_buf(),
        roster.to_path_buf(),
        repo.join(".gitattributes"),
        repo.join("experiments/lt9-la2-p1n3/Cargo.toml"),
        repo.join("experiments/lt9-la2-p1n3/Cargo.lock"),
        repo.join("apps/phoenix-memory-lock/src/bin/lt9_la2p1n3.rs"),
        repo.join("apps/phoenix-memory-lock/src/bin/lt9_la2p1n3_features.rs"),
        repo.join("apps/phoenix-memory-lock/src/bin/lt9_la2p1n3_seal.rs"),
    ];
    let mut source_sha256 = BTreeMap::new();
    for path in &paths {
        source_sha256.insert(
            path.strip_prefix(repo)
                .unwrap_or(path)
                .to_string_lossy()
                .to_string(),
            hash_file(path)?,
        );
    }
    Ok(PreReviewRoot {
        schema: "phoenix.lexical.lt9-la2-p1n3-pre-review-root/v1",
        date: DATE,
        branch: "codex/phoenix-native-p1n3-natural-pairwise-20260923".to_string(),
        protocol_sha256: hash_file(protocol)?,
        roster_sha256: hash_file(roster)?,
        source_sha256,
        binary_sha256: hash_file(&binary)?,
        packets_sha256: hash_file(packets)?,
        rubric_sha256: hash_file(rubric)?,
        private_ledger_sha256: hash_file(ledger)?,
        receipt_sha256: hash_file(receipt)?,
        judgments_completed: false,
    })
}
