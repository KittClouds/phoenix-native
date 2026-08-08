use crate::open::ensure_strictly_increasing;
use crate::{MemoryContractError, ModelSemanticRoleV3, SemanticCandidateRecordV3};
use hashbrown::HashSet;
use phoenix_graph_generation_v2::{ModelIdentityRecord, NliAdjudicationRecord};

pub(crate) fn validate_nli_adjudications(
    adjudications: &[NliAdjudicationRecord],
    candidates: &[SemanticCandidateRecordV3],
    models: &[ModelIdentityRecord],
) -> Result<(), MemoryContractError> {
    ensure_strictly_increasing(
        adjudications
            .iter()
            .map(|record| (record.candidate_id.0, record.model_index)),
    )?;
    let candidate_ids = candidates
        .iter()
        .map(|record| record.candidate_id)
        .collect::<HashSet<_>>();
    for adjudication in adjudications {
        let probabilities = [
            f32::from_bits(adjudication.contradiction_bits),
            f32::from_bits(adjudication.entailment_bits),
            f32::from_bits(adjudication.neutral_bits),
        ];
        let probability_sum = probabilities.iter().sum::<f32>();
        if !candidate_ids.contains(&adjudication.candidate_id.0)
            || models
                .get(adjudication.model_index as usize)
                .and_then(|model| ModelSemanticRoleV3::from_flags(model.flags))
                != Some(ModelSemanticRoleV3::DedicatedNliObserver)
            || probabilities
                .iter()
                .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
            || (probability_sum - 1.0).abs() > 0.01
        {
            return Err(MemoryContractError::InvalidSourceModel(
                "NLI adjudication must bind a candidate to the dedicated NLI observer lane",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytemuck::Zeroable;
    use phoenix_graph_generation_v2::CandidateId;

    #[test]
    fn only_dedicated_nli_models_may_author_adjudications() {
        let candidate_id = [0x51; 32];
        let mut candidate = SemanticCandidateRecordV3::zeroed();
        candidate.candidate_id = candidate_id;
        let adjudication = normalized_adjudication(candidate_id);
        let mut model = ModelIdentityRecord::zeroed();
        model.flags = ModelSemanticRoleV3::DedicatedNliObserver.flags();
        assert!(validate_nli_adjudications(&[adjudication], &[candidate], &[model]).is_ok());

        model.flags = ModelSemanticRoleV3::SteerableSemanticObserver.flags();
        assert!(matches!(
            validate_nli_adjudications(&[adjudication], &[candidate], &[model]),
            Err(MemoryContractError::InvalidSourceModel(
                "NLI adjudication must bind a candidate to the dedicated NLI observer lane"
            ))
        ));
    }

    #[test]
    fn nli_probabilities_are_finite_bounded_and_normalized() {
        let candidate_id = [0x61; 32];
        let mut candidate = SemanticCandidateRecordV3::zeroed();
        candidate.candidate_id = candidate_id;
        let mut adjudication = normalized_adjudication(candidate_id);
        adjudication.contradiction_bits = 0.9_f32.to_bits();
        adjudication.entailment_bits = 0.9_f32.to_bits();
        adjudication.neutral_bits = 0.9_f32.to_bits();
        let mut model = ModelIdentityRecord::zeroed();
        model.flags = ModelSemanticRoleV3::DedicatedNliObserver.flags();
        assert!(validate_nli_adjudications(&[adjudication], &[candidate], &[model]).is_err());
    }

    fn normalized_adjudication(candidate_id: [u8; 32]) -> NliAdjudicationRecord {
        NliAdjudicationRecord {
            candidate_id: CandidateId(candidate_id),
            contradiction_bits: 0.1_f32.to_bits(),
            entailment_bits: 0.8_f32.to_bits(),
            neutral_bits: 0.1_f32.to_bits(),
            label: 2,
            status: 1,
            model_index: 0,
            flags: 0,
        }
    }
}
