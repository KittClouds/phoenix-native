use anyhow::{Context, Result};
use phoenix_memory_contract::{CandidateEndpointRoleV3, SemanticCandidateFamilyV3};
use phoenix_memory_semantics::{
    core_pack, narrative_pack, CandidateBuilder, CoreRelation, NarrativeRelation,
    VocabularyRelation,
};
use std::env;
use std::fs;
use std::path::PathBuf;

const CORE_PROBES: [(&str, CoreRelation, SemanticCandidateFamilyV3); 3] = [
    (
        "I’m immortal",
        CoreRelation::Attribute,
        SemanticCandidateFamilyV3::Attribute,
    ),
    (
        "Adam sends his regards",
        CoreRelation::Relationship,
        SemanticCandidateFamilyV3::Relationship,
    ),
    (
        "I’ve been hired to give you this",
        CoreRelation::Commitment,
        SemanticCandidateFamilyV3::Goal,
    ),
];

const NARRATIVE_PROBES: [(&str, NarrativeRelation, SemanticCandidateFamilyV3); 3] = [
    (
        "Ryan stopped",
        NarrativeRelation::CharacterState,
        SemanticCandidateFamilyV3::State,
    ),
    (
        "The room erupted in screams",
        NarrativeRelation::SceneMembership,
        SemanticCandidateFamilyV3::Event,
    ),
    (
        "on my next save",
        NarrativeRelation::EpisodeMembership,
        SemanticCandidateFamilyV3::Event,
    ),
];

fn main() {
    if let Err(error) = run() {
        eprintln!("shortrun-shadow: {error:#}");
        std::process::exit(2);
    }
}

fn run() -> Result<()> {
    let path = env::args_os()
        .nth(1)
        .map_or_else(default_path, PathBuf::from);
    let text = fs::read_to_string(&path)
        .with_context(|| format!("read Shortrun source {}", path.display()))?;
    let source_hash = blake3::hash(text.as_bytes());
    let subject = nonzero_id(b"entity/ryan");
    let mut candidates = Vec::with_capacity(CORE_PROBES.len() + NARRATIVE_PROBES.len());
    let mut evidence_ranges = Vec::with_capacity(candidates.capacity());

    for (needle, relation, family) in CORE_PROBES {
        let (start, end, evidence_id) = evidence(&text, needle)?;
        candidates.push(
            CandidateBuilder::new(core_pack(), family, relation.stable_name())
                .endpoint(subject, CandidateEndpointRoleV3::Subject)
                .evidence(evidence_id)
                .value(needle)
                .build()?,
        );
        evidence_ranges.push((evidence_id, start, end));
    }
    for (needle, relation, family) in NARRATIVE_PROBES {
        let (start, end, evidence_id) = evidence(&text, needle)?;
        candidates.push(
            CandidateBuilder::relation(narrative_pack(), family, relation)
                .endpoint(subject, CandidateEndpointRoleV3::Participant)
                .evidence(evidence_id)
                .value(needle)
                .build()?,
        );
        evidence_ranges.push((evidence_id, start, end));
    }

    let core_count = candidates
        .iter()
        .filter(|candidate| candidate.vocabulary_pack_id == core_pack().id)
        .count();
    let narrative_count = candidates
        .iter()
        .filter(|candidate| candidate.vocabulary_pack_id == narrative_pack().id)
        .count();
    println!("SHORTRUN_SHADOW_OK");
    println!("source={}", path.display());
    println!("bytes={}", text.len());
    println!("blake3={}", source_hash.to_hex());
    println!("core_candidates={core_count}");
    println!("narrative_candidates={narrative_count}");
    println!("evidence_ranges={}", evidence_ranges.len());
    println!("accepted=0");
    println!("all_status=proposed");
    Ok(())
}

fn evidence(text: &str, needle: &str) -> Result<(u32, u32, u64)> {
    let start = text
        .find(needle)
        .with_context(|| format!("required Shortrun evidence is missing: {needle:?}"))?;
    let end = start + needle.len();
    let start_u32 = u32::try_from(start).context("evidence start exceeds u32")?;
    let end_u32 = u32::try_from(end).context("evidence end exceeds u32")?;
    let mut bytes = Vec::with_capacity(8 + needle.len());
    bytes.extend_from_slice(&start_u32.to_le_bytes());
    bytes.extend_from_slice(&end_u32.to_le_bytes());
    bytes.extend_from_slice(needle.as_bytes());
    Ok((start_u32, end_u32, nonzero_id(&bytes)))
}

fn nonzero_id(bytes: &[u8]) -> u64 {
    let hash = blake3::hash(bytes);
    let mut id = u64::from_le_bytes(hash.as_bytes()[..8].try_into().unwrap_or([0; 8]));
    if id == 0 {
        id = 1;
    }
    id
}

fn default_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("docs")
        .join("shortrun.md")
}
