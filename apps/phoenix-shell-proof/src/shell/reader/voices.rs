use phoenix_reader_session::{
    CastProfile, NarrationPlan, UtteranceVoices, VoiceChoice, VoiceProfile,
};
use phoenix_tts_native::VoiceAsset;
use serde::Deserialize;
use std::{path::PathBuf, sync::Arc};

#[derive(Clone, Deserialize)]
pub struct VoiceSpec {
    pub profile: VoiceProfile,
    pub asset: Option<PathBuf>,
    #[serde(default)]
    pub supertonic_style: Option<String>,
}
impl Default for VoiceSpec {
    fn default() -> Self {
        Self {
            profile: VoiceProfile {
                id: *blake3::hash(b"phoenix.default-narrator/v1").as_bytes(),
                revision: 1,
                name: "Calm narrator".into(),
                description: "A calm, clear English narrator.".into(),
                reference: None,
                default_delivery: String::new(),
                seed: 42,
            },
            asset: None,
            supertonic_style: None,
        }
    }
}
pub struct PreparedVoice {
    pub name: String,
    pub instruction: String,
    pub seed: u32,
    pub asset: Option<VoiceAsset>,
    pub supertonic_style: Option<String>,
}
pub struct PreparedVoices {
    pub voices: Box<[Arc<PreparedVoice>]>,
    pub slots: Box<[u16]>,
    pub table: UtteranceVoices,
}
pub fn prepare(
    specs: &[VoiceSpec],
    selected: Option<VoiceChoice>,
    cast: Option<&CastProfile>,
    source: &str,
    plan: &NarrationPlan,
    bundle: &super::engines::Bundles,
) -> anyhow::Result<PreparedVoices> {
    anyhow::ensure!(
        !specs.is_empty() && specs.len() <= 257,
        "Voice library bounds"
    );
    let choices = specs
        .iter()
        .map(|v| VoiceChoice::of(&v.profile))
        .collect::<Result<Vec<_>, _>>()?;
    for (i, choice) in choices.iter().enumerate() {
        anyhow::ensure!(!choices[..i].contains(choice), "Duplicate voice profile");
    }
    if let Some(cast) = cast {
        cast.validate(source, plan)?;
    }
    let narrator = selected
        .or_else(|| cast.map(|c| c.narrator))
        .unwrap_or(choices[0]);
    let mut prepared = Vec::new();
    let mut identities = Vec::new();
    let mut keys: Vec<(VoiceChoice, String)> = Vec::new();
    let mut slots = Vec::with_capacity(plan.spec().segments.len());
    for segment in &plan.spec().segments {
        let (choice, delivery) = if let Some(cast) = cast {
            cast.resolve_with_narrator(segment.source, narrator)?
        } else {
            (narrator, "")
        };
        if let Some(index) = keys.iter().position(|(c, d)| *c == choice && d == delivery) {
            slots.push(index as u16);
            continue;
        }
        anyhow::ensure!(prepared.len() < 257, "Too many distinct voice directions");
        let index = choices.iter().position(|c| *c == choice).ok_or_else(|| {
            anyhow::anyhow!("Assigned voice is missing or changed; choose its saved revision")
        })?;
        let spec = &specs[index];
        let profile = &spec.profile;
        let delivery = if delivery.is_empty() {
            &profile.default_delivery
        } else {
            delivery
        };
        let instruction = if profile.reference.is_some() {
            delivery.to_owned()
        } else if delivery.is_empty() {
            profile.description.clone()
        } else {
            format!("{} {delivery}", profile.description)
        };
        let asset = match (&profile.reference, &spec.asset) {
            (Some(reference), Some(path)) => {
                let asset =
                    VoiceAsset::open(path, reference.encoded, reference.model, reference.codec)?;
                anyhow::ensure!(
                    *blake3::hash(asset.transcript().as_bytes()).as_bytes() == reference.transcript,
                    "Voice reference transcript mismatch"
                );
                Some(asset)
            }
            (None, None) => None,
            _ => anyhow::bail!("Voice reference enrollment is incomplete"),
        };
        if spec.supertonic_style.is_some() {
            anyhow::ensure!(
                delivery.is_empty(),
                "Supertonic uses fixed styles; custom character delivery requires Breeze"
            );
        }
        let identity = bundle.identity(spec, &instruction, asset.as_ref())?;
        // Keep the assignment key's explicit override, separate from default delivery.
        let override_delivery = cast
            .map(|c| c.resolve(segment.source).map(|(_, d)| d))
            .transpose()?
            .unwrap_or("");
        keys.push((choice, override_delivery.to_owned()));
        slots.push(prepared.len() as u16);
        prepared.push(Arc::new(PreparedVoice {
            name: profile.name.clone(),
            instruction,
            seed: profile.seed,
            asset,
            supertonic_style: spec.supertonic_style.clone(),
        }));
        identities.push(identity);
    }
    let table = UtteranceVoices::new(plan, identities, slots.clone())?;
    Ok(PreparedVoices {
        voices: prepared.into_boxed_slice(),
        slots: slots.into_boxed_slice(),
        table,
    })
}

pub fn load_library(storage: &std::path::Path, specs: &mut Vec<VoiceSpec>) -> anyhow::Result<()> {
    if specs.is_empty() {
        specs.push(VoiceSpec::default());
    }
    let root = storage.join("voices");
    let library = phoenix_reader_session::VoiceLibrary::open(&root)?;
    for spec in specs.iter() {
        library.save(&spec.profile)?;
    }
    for profile in library.list()? {
        let choice = VoiceChoice::of(&profile)?;
        if specs
            .iter()
            .any(|s| VoiceChoice::of(&s.profile).ok() == Some(choice))
        {
            continue;
        }
        let asset = profile
            .reference
            .as_ref()
            .map(|r| root.join(format!("{}.breeze", blake3::Hash::from(r.encoded).to_hex())));
        let supertonic_style = phoenix_tts_native::supertonic::STYLES
            .iter()
            .find(|style| profile.id == cpu_id(style))
            .map(|s| s.to_string());
        specs.push(VoiceSpec {
            profile,
            asset,
            supertonic_style,
        });
    }
    anyhow::ensure!(specs.len() <= 257, "Voice catalog bounds");
    Ok(())
}
fn cpu_id(style: &str) -> [u8; 32] {
    *blake3::hash(format!("phoenix.supertonic-style/v1/{style}").as_bytes()).as_bytes()
}
pub fn add_cpu_voices(specs: &mut Vec<VoiceSpec>) {
    if specs.is_empty() {
        specs.push(VoiceSpec::default());
    }
    for style in phoenix_tts_native::supertonic::STYLES {
        if specs.iter().any(|s| s.profile.id == cpu_id(style)) {
            continue;
        }
        specs.push(VoiceSpec {
            profile: VoiceProfile {
                id: cpu_id(style),
                revision: 1,
                name: format!("Supertonic {style}"),
                description: format!("Fixed {style} voice style · runs on CPU · English"),
                reference: None,
                default_delivery: String::new(),
                seed: 0,
            },
            asset: None,
            supertonic_style: Some(style.into()),
        });
    }
}

#[allow(clippy::too_many_arguments)]
pub fn assign_passage(
    storage: &std::path::Path,
    previous: Option<&CastProfile>,
    source: &str,
    plan: &NarrationPlan,
    segment: u32,
    name: &str,
    voice: VoiceChoice,
    narrator: VoiceChoice,
) -> anyhow::Result<()> {
    use phoenix_reader_session::{CastMember, SpeakerAssignment, VoiceLibrary};
    anyhow::ensure!(
        !name.trim().is_empty() && name.len() <= 256,
        "Enter a character name"
    );
    let character = *blake3::hash(name.trim().to_lowercase().as_bytes()).as_bytes();
    let range = plan.segment(segment)?.source;
    let mut cast = previous.cloned().unwrap_or_else(|| CastProfile {
        revision: 1,
        document: plan.spec().document,
        narrator,
        members: Vec::new(),
        assignments: Vec::new(),
    });
    cast.validate(source, plan)?;
    cast.revision = cast
        .revision
        .checked_add(1)
        .ok_or_else(|| anyhow::anyhow!("Cast revision exhausted"))?;
    cast.narrator = narrator;
    if let Some(member) = cast.members.iter_mut().find(|m| m.character == character) {
        member.voice = voice;
    } else {
        cast.members.push(CastMember {
            character,
            name: name.trim().into(),
            voice,
            delivery: String::new(),
        });
    }
    // The UI assigns an entire planned passage. Never split transformed source implicitly.
    cast.assignments.retain(|a| !a.source.overlaps(range));
    cast.assignments.push(SpeakerAssignment {
        source: range,
        character,
    });
    cast.assignments.sort_unstable_by_key(|a| a.source.start);
    VoiceLibrary::open(storage.join("voices"))?.save_cast(&cast, source, plan)?;
    Ok(())
}
