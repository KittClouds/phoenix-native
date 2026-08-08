use phoenix_lexical_qps::{RankEvidenceV3, RelevanceTier};

const DOMAIN: &[u8] = b"phoenix-qps-v3-evidence-provenance\0";

/// Content identity for primitive evidence. This follows the evidence itself,
/// so renaming a private query or document cannot disguise release ancestry.
pub(super) fn evidence_fingerprint(evidence: RankEvidenceV3, tier: RelevanceTier) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(DOMAIN);
    hasher.update(&evidence.schema_version.to_le_bytes());
    hasher.update(&evidence.query_groups.to_le_bytes());
    hasher.update(&evidence.matched_groups.to_le_bytes());
    hasher.update(&evidence.missing_groups.to_le_bytes());
    hasher.update(&evidence.query_flags.to_le_bytes());
    hasher.update(&evidence.field_count.to_le_bytes());
    hasher.update(&[tier as u8]);
    for value in evidence.values {
        hasher.update(&value.to_bits().to_le_bytes());
    }
    *hasher.finalize().as_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_follows_evidence_not_renamed_identity() {
        let mut evidence = RankEvidenceV3 {
            schema_version: 3,
            query_groups: 1,
            matched_groups: 1,
            missing_groups: 0,
            query_flags: 0,
            field_count: 1,
            values: [0.0; 30],
        };
        let first = evidence_fingerprint(evidence, RelevanceTier::CompleteExactGroups);
        assert_eq!(
            first,
            evidence_fingerprint(evidence, RelevanceTier::CompleteExactGroups)
        );
        evidence.values[0] = f32::from_bits(1);
        assert_ne!(
            first,
            evidence_fingerprint(evidence, RelevanceTier::CompleteExactGroups)
        );
    }
}
