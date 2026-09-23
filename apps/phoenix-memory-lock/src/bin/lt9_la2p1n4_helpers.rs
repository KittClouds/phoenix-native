const RUBRIC: &str = "# Natural context compatibility review\n\nFor each item, read the displayed word pair and both excerpts. Assign exactly one label:\n\n- `SAME`: the two contexts express compatible contextual uses of the displayed lexical relation.\n- `DIFFERENT`: the contexts clearly express incompatible contextual uses or senses relevant to that relation.\n- `UNKNOWN`: the excerpts do not establish compatibility or incompatibility, or the case is genuinely ambiguous.\n\nJudge from the excerpts shown. Shared words alone do not establish `SAME`. Do not use outside information or guess the sampling purpose. Edit only `judgment` to `SAME`, `DIFFERENT`, or `UNKNOWN`; preserve IDs, word pairs, and context text exactly.\n";

#[derive(Clone)]
struct Selected {
    relation_index: usize,
    choice: PairChoice,
}

#[derive(Serialize)]
struct JudgmentTemplateRow {
    packet_id: String,
    judgment: Option<String>,
}

fn build_relation_pools(
    relation_index: usize,
    occ: &[Occurrence],
    pools: &mut [Vec<PairChoice>],
    eligible: &mut [[u64; 3]],
) -> Result<()> {
    let corpus_count = eligible.len();
    let mut by_corpus: Vec<Vec<PairChoice>> = (0..corpus_count).map(|_| Vec::new()).collect();
    for left in 0..occ.len() {
        for right in (left + 1)..occ.len() {
            let a = &occ[left];
            let b = &occ[right];
            if a.corpus_index != b.corpus_index || a.doc_hash == b.doc_hash {
                continue;
            }
            let pair = pair_features(&a.features, &b.features);
            let overlap = token_overlap(&a.features.tokens, &b.features.tokens);
            let key = stable_hash(
                format!(
                    "{}|{}|{}",
                    RELATIONS[relation_index].id, a.doc_hash, b.doc_hash
                )
                .as_bytes(),
            );
            let divergence = structural_divergence(&pair);
            by_corpus[a.corpus_index].push(PairChoice {
                left,
                right,
                corpus_index: a.corpus_index,
                overlap,
                divergence,
                stratum: usize::MAX,
                order_key: key,
            });
        }
    }
    for (corpus_index, rows) in by_corpus.iter_mut().enumerate() {
        eligible[corpus_index][relation_index] = rows.len() as u64;
        if rows.len() < 4 {
            continue;
        }
        rows.sort_by(|a, b| {
            a.overlap
                .total_cmp(&b.overlap)
                .then_with(|| a.order_key.cmp(&b.order_key))
        });
        let n = rows.len();
        let q = (n / 4).max(1);
        let low_end = q.min(n);
        let high_start = n.saturating_sub(q);
        let high_indices: Vec<usize> = (high_start..n).collect();
        let mut high_rank = high_indices.clone();
        high_rank.sort_by(|&i, &j| {
            rows[i]
                .divergence
                .cmp(&rows[j].divergence)
                .then_with(|| rows[i].order_key.cmp(&rows[j].order_key))
        });
        let hard_count = (high_rank.len() / 4).max(1).min(high_rank.len());
        let hard_indices: HashSet<usize> =
            high_rank.iter().rev().take(hard_count).copied().collect();

        let mut local_counts: Vec<u16> = Vec::with_capacity(occ.len());
        let mut excerpt_counts: Vec<u16> = Vec::with_capacity(occ.len());
        for item in occ.iter().filter(|o| o.corpus_index == corpus_index) {
            local_counts.push(item.features.local_token_count);
            excerpt_counts.push(item.excerpt_words);
        }
        let local_q1 = lower_quartile_u16(&local_counts);
        let excerpt_q1 = lower_quartile_u16(&excerpt_counts);
        for (rank, row) in rows.iter_mut().enumerate() {
            let a = &occ[row.left];
            let b = &occ[row.right];
            let pair = pair_features(&a.features, &b.features);
            let low_evidence = a.features.local_token_count <= local_q1
                && b.features.local_token_count <= local_q1
                && a.excerpt_words <= excerpt_q1
                && b.excerpt_words <= excerpt_q1;
            let weak_cue_overlap = pair.shared_tokens.len() <= 1
                && pair.shared_bigrams.is_empty()
                && pair.shared_trigrams.is_empty();
            row.stratum = classify_stratum(
                low_evidence,
                weak_cue_overlap,
                rank < low_end,
                rank >= high_start,
                hard_indices.contains(&rank),
            );
            let choice = *row;
            pools[relation_index * STRATA.len() + choice.stratum].push(choice);
        }
    }
    Ok(())
}

fn select_pairs(pools: &[Vec<PairChoice>], occurrences: &[Vec<Occurrence>]) -> Vec<Selected> {
    let mut selected = Vec::new();
    let mut used_docs: HashSet<&str> = HashSet::new();
    let mut selected_by_pool = vec![0usize; pools.len()];
    let mut by_corpus: HashMap<(usize, usize, usize), usize> = HashMap::new();
    let mut cursors = vec![0usize; pools.len()];
    let mut max_rounds = pools.iter().map(Vec::len).sum::<usize>().saturating_add(1);
    while max_rounds > 0 {
        max_rounds -= 1;
        let mut progress = false;
        for pool_index in 0..pools.len() {
            if selected_by_pool[pool_index] >= TARGET_PER_CANDIDATE_STRATUM {
                continue;
            }
            while cursors[pool_index] < pools[pool_index].len() {
                let choice = pools[pool_index][cursors[pool_index]];
                cursors[pool_index] += 1;
                let relation_index = pool_index / STRATA.len();
                let left = &occurrences[relation_index][choice.left];
                let right = &occurrences[relation_index][choice.right];
                if used_docs.contains(left.doc_hash.as_str())
                    || used_docs.contains(right.doc_hash.as_str())
                {
                    continue;
                }
                let cap = (relation_index, choice.stratum, choice.corpus_index);
                if *by_corpus.get(&cap).unwrap_or(&0) >= MAX_PER_CORPUS_CANDIDATE_STRATUM {
                    continue;
                }
                used_docs.insert(left.doc_hash.as_str());
                used_docs.insert(right.doc_hash.as_str());
                *by_corpus.entry(cap).or_insert(0) += 1;
                selected_by_pool[pool_index] += 1;
                selected.push(Selected {
                    relation_index,
                    choice,
                });
                progress = true;
                break;
            }
        }
        if !progress {
            break;
        }
    }
    selected
}

fn classify_stratum(
    low_evidence: bool,
    weak_cue_overlap: bool,
    low_overlap_quartile: bool,
    high_overlap_quartile: bool,
    high_structural_divergence: bool,
) -> usize {
    if low_evidence && weak_cue_overlap {
        0
    } else if high_overlap_quartile && high_structural_divergence {
        1
    } else if high_overlap_quartile {
        2
    } else if low_overlap_quartile {
        3
    } else {
        4
    }
}

fn prior_template_key(candidate_id: &str, template_hash: &str) -> String {
    format!("{candidate_id}:{template_hash}")
}

fn is_prior_template_excluded(
    candidate_id: &str,
    template_hash: &str,
    excluded: &HashSet<String>,
) -> bool {
    excluded.contains(&prior_template_key(candidate_id, template_hash))
}

fn structural_divergence(pair: &PairFeatures) -> u32 {
    pair.role_count_abs_delta
        .iter()
        .map(|&x| x as u32)
        .sum::<u32>()
        + pair.token_count_abs_delta as u32
        + pair.distance_bin_abs_delta as u32
        + u32::from(!pair.support_cue_equal)
        + u32::from(!pair.contradiction_cue_equal)
        + u32::from(!pair.same_field_kind)
}

fn lower_quartile_u16(values: &[u16]) -> u16 {
    if values.is_empty() {
        return 0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    sorted[(sorted.len() - 1) / 4]
}

fn document_hash(title: &str, text: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(title.as_bytes());
    hash.update([0]);
    hash.update(text.as_bytes());
    format!("{:x}", hash.finalize())
}
fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn hash_file(path: &Path) -> Result<String> {
    Ok(sha256(&fs::read(path)?))
}
fn stable_hash(bytes: &[u8]) -> u64 {
    let digest = Sha256::digest([SALT, bytes].concat());
    u64::from_le_bytes(digest[..8].try_into().expect("digest prefix"))
}
fn packet_id(candidate: &str, left: &str, right: &str) -> String {
    let (a, b) = if left <= right {
        (left, right)
    } else {
        (right, left)
    };
    format!(
        "n4-{}",
        &sha256(&[SALT, candidate.as_bytes(), a.as_bytes(), b.as_bytes()].concat())[..16]
    )
}
fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let mut file = File::create(path)?;
    serde_json::to_writer_pretty(&mut file, value)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

fn assign_template_splits(rows: &mut [PrivateLedgerRow]) {
    let mut parents: Vec<usize> = (0..rows.len()).collect();
    let mut seen: HashMap<(String, String), usize> = HashMap::new();
    for (i, row) in rows.iter().enumerate() {
        for template in [&row.left_template_sha256, &row.right_template_sha256] {
            let key = (row.candidate_id.clone(), template.clone());
            if let Some(previous) = seen.insert(key, i) {
                union(&mut parents, i, previous);
            }
        }
    }
    let mut roots: HashMap<usize, String> = HashMap::new();
    for i in 0..rows.len() {
        let root = find(&mut parents, i);
        roots
            .entry(root)
            .and_modify(|id| {
                if rows[i].packet_id < *id {
                    *id = rows[i].packet_id.clone();
                }
            })
            .or_insert_with(|| rows[i].packet_id.clone());
    }
    for i in 0..rows.len() {
        let root = find(&mut parents, i);
        let key = format!("{}:{}", rows[i].candidate_id, roots[&root]);
        rows[i].split = if stable_hash(key.as_bytes()) % 4 == 0 {
            "holdout"
        } else {
            "fit"
        }
        .to_string();
    }
}
fn find(parents: &mut [usize], x: usize) -> usize {
    if parents[x] != x {
        let root = find(parents, parents[x]);
        parents[x] = root;
    }
    parents[x]
}
fn union(parents: &mut [usize], a: usize, b: usize) {
    let (ra, rb) = (find(parents, a), find(parents, b));
    if ra != rb {
        parents[rb] = ra;
    }
}

fn validate_packets(
    packets: &[ReviewPacket],
    rows: &[PrivateLedgerRow],
    excluded: &HashSet<String>,
) -> Result<()> {
    ensure!(packets.len() == rows.len(), "packet/ledger count differs");
    let mut ids = HashSet::new();
    let mut docs = HashSet::new();
    let mut template_splits: HashMap<(String, String), &str> = HashMap::new();
    for (packet, row) in packets.iter().zip(rows) {
        ensure!(
            packet.packet_id == row.packet_id && packet.judgment.is_none(),
            "packet/ledger mismatch or label present"
        );
        ensure!(ids.insert(packet.packet_id.as_str()), "duplicate packet ID");
        for doc in [&row.left_document_sha256, &row.right_document_sha256] {
            ensure!(!excluded.contains(doc), "P1N3 document reused");
            ensure!(docs.insert(doc.as_str()), "P1N4 document reused");
        }
        let relation = RELATIONS
            .iter()
            .find(|r| r.id == row.candidate_id)
            .context("candidate relation")?;
        ensure!(
            feature_token_leaks(&row.left, &row.right, &row.pair_features, *relation) == 0,
            "candidate token leaked to feature representation"
        );
        for template in [&row.left_template_sha256, &row.right_template_sha256] {
            let key = (row.candidate_id.clone(), template.clone());
            if let Some(previous) = template_splits.insert(key, row.split.as_str()) {
                ensure!(previous == row.split, "template crosses frozen split");
            }
        }
    }
    Ok(())
}

fn make_receipt(
    roster: &Roster,
    roster_bytes: &[u8],
    prior_ledger: &[u8],
    excluded: &HashSet<String>,
    excluded_templates: &HashSet<String>,
    counts: Vec<CorpusCount>,
    rows: &[PrivateLedgerRow],
    docs: usize,
    leaks: usize,
) -> Result<Receipt> {
    let mut selected = [[0usize; 5]; 3];
    for row in rows {
        let r = RELATIONS
            .iter()
            .position(|x| x.id == row.candidate_id)
            .context("candidate id")?;
        let s = STRATA
            .iter()
            .position(|x| *x == row.sampling_stratum)
            .context("stratum")?;
        selected[r][s] += 1;
    }
    let shortfall = std::array::from_fn(|r| {
        std::array::from_fn(|s| TARGET_PER_CANDIDATE_STRATUM.saturating_sub(selected[r][s]))
    });
    Ok(Receipt {
        schema: "phoenix.lexical.lt9-la2-p1n4-targeted-acquisition/v1",
        date: DATE,
        status: "blind_packets_ready_no_judgments",
        source_roster_sha256: sha256(roster_bytes),
        p1n3_document_exclusion_ledger_sha256: sha256(prior_ledger),
        p1n3_documents_excluded: excluded.len(),
        p1n3_context_templates_excluded: excluded_templates.len(),
        source_hashes_verified: counts.len() == roster.corpora.len(),
        target_per_candidate_per_stratum: TARGET_PER_CANDIDATE_STRATUM,
        max_per_corpus_candidate_stratum: MAX_PER_CORPUS_CANDIDATE_STRATUM,
        stratum_names: STRATA,
        selected_by_candidate_stratum: selected,
        shortfall_by_candidate_stratum: shortfall,
        review_packet_count: rows.len(),
        unique_documents_in_packets: docs,
        fit_packets: rows.iter().filter(|r| r.split == "fit").count(),
        holdout_packets: rows.iter().filter(|r| r.split == "holdout").count(),
        template_split_conflicts: 0,
        candidate_token_feature_leaks: leaks,
        semantic_labels_assigned: 0,
        corpora: counts
            .into_iter()
            .map(|c| CorpusCount {
                corpus_id: c.corpus_id,
                documents_scanned: c.documents_scanned,
                p1n3_documents_excluded: c.p1n3_documents_excluded,
                p1n3_templates_excluded: c.p1n3_templates_excluded,
                relation_occurrences_seen: c.relation_occurrences_seen,
                reservoir_contexts: c.reservoir_contexts,
                eligible_context_pairs: c.eligible_context_pairs,
            })
            .collect(),
        scope: Scope {
            p1n3_packets_or_judgments_opened_by_sampler: false,
            p1n3_document_and_template_hashes_used_only_for_exclusion: true,
            qrels_queries_or_expected_sense_labels_read: false,
            retrieval_or_ranking_run: false,
            authority_updated: false,
            labels_assigned_by_sampler: false,
            reviewer_receives_provenance_features_or_strata: false,
        },
    })
}

fn make_root(
    repo: &Path,
    protocol: &Path,
    roster: &Path,
    prior_ledger: &Path,
    prior_root: &Path,
    packets: &Path,
    rubric: &Path,
    judgments_template: &Path,
    ledger: &Path,
    receipt: &Path,
) -> Result<PreReviewRoot> {
    let source = [
        protocol.to_path_buf(),
        roster.to_path_buf(),
        repo.join(".gitattributes"),
        repo.join("apps/phoenix-memory-lock/src/bin/lt9_la2p1n4.rs"),
        repo.join("apps/phoenix-memory-lock/src/bin/lt9_la2p1n4_helpers.rs"),
        repo.join("apps/phoenix-memory-lock/src/bin/lt9_la2p1n3_features.rs"),
        repo.join("experiments/lt9-la2-p1n4/Cargo.toml"),
        repo.join("experiments/lt9-la2-p1n4/Cargo.lock"),
    ];
    let mut source_sha256 = BTreeMap::new();
    for path in source {
        source_sha256.insert(
            path.strip_prefix(repo)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string(),
            hash_file(&path)?,
        );
    }
    let binary = env::current_exe()?;
    Ok(PreReviewRoot {
        schema: "phoenix.lexical.lt9-la2-p1n4-pre-review-root/v1",
        date: DATE,
        branch: "codex/phoenix-native-p1n4-targeted-compatibility-20260923".to_string(),
        protocol_sha256: hash_file(protocol)?,
        roster_sha256: hash_file(roster)?,
        prior_ledger_sha256: hash_file(prior_ledger)?,
        prior_root_sha256: hash_file(prior_root)?,
        source_sha256,
        binary_sha256: hash_file(&binary)?,
        packets_sha256: hash_file(packets)?,
        rubric_sha256: hash_file(rubric)?,
        judgment_template_sha256: hash_file(judgments_template)?,
        private_ledger_sha256: hash_file(ledger)?,
        receipt_sha256: hash_file(receipt)?,
        judgments_completed: false,
    })
}

#[cfg(test)]
mod tests {
    use crate::*;
    #[test]
    fn strata_precedence_preserves_ambiguity_and_hard_negative_cells() {
        assert_eq!(classify_stratum(true, true, false, true, true), 0);
        assert_eq!(classify_stratum(false, true, false, true, true), 1);
        assert_eq!(classify_stratum(false, true, false, true, false), 2);
        assert_eq!(classify_stratum(false, true, true, false, false), 3);
        assert_eq!(classify_stratum(false, false, false, false, false), 4);
    }
    #[test]
    fn structural_divergence_is_categorical_and_deterministic() {
        let p = PairFeatures {
            shared_tokens: vec![],
            shared_role_tokens: vec![],
            shared_bigrams: vec![],
            shared_trigrams: vec![],
            token_jaccard: 0.0,
            bigram_jaccard: 0.0,
            trigram_jaccard: 0.0,
            role_count_abs_delta: [2, 0, 1],
            token_count_abs_delta: 3,
            distance_bin_abs_delta: 2,
            support_cue_equal: false,
            contradiction_cue_equal: true,
            same_field_kind: false,
        };
        assert_eq!(structural_divergence(&p), 10);
        assert_eq!(structural_divergence(&p), structural_divergence(&p));
    }
    #[test]
    fn lower_quartile_uses_fixed_order_statistic() {
        assert_eq!(lower_quartile_u16(&[9, 1, 8, 2]), 1);
        assert_eq!(lower_quartile_u16(&[]), 0);
    }
    #[test]
    fn prior_template_exclusion_is_relation_specific() {
        let excluded: HashSet<String> = [prior_template_key("bank_to_water", "same-template")]
            .into_iter()
            .collect();
        assert!(is_prior_template_excluded(
            "bank_to_water",
            "same-template",
            &excluded
        ));
        assert!(!is_prior_template_excluded(
            "car_to_vehicle",
            "same-template",
            &excluded
        ));
    }
}
