//! Lazy engine enrollment: a CPU-only session never requires a Breeze model.
use super::{voices::VoiceSpec, Config};
use phoenix_reader_session::{NarrationPlan, VoiceChoice};
use phoenix_tts_native::{
    supertonic::{SupertonicBundle, SupertonicProvider},
    Bundle, Cancellation, NativeProvider, VoiceAsset,
};
use std::{path::PathBuf, time::Duration};
#[derive(serde::Deserialize)]
pub struct CpuConfig {
    pub runner: PathBuf,
    pub models: PathBuf,
}
pub struct Bundles {
    pub breeze: Option<Bundle>,
    pub cpu: Option<SupertonicBundle>,
}
impl Bundles {
    pub fn open(
        config: &Config,
        used: &[&VoiceSpec],
        cancel: &Cancellation,
    ) -> anyhow::Result<Self> {
        let breeze = if used.iter().any(|v| v.supertonic_style.is_none()) {
            Some(Bundle::open_cancellable(
                &config.worker,
                &config.model,
                &config.dll_directory,
                cancel,
            )?)
        } else {
            None
        };
        let cpu = if used.iter().any(|v| v.supertonic_style.is_some()) {
            let cpu = config.supertonic.as_ref().ok_or_else(|| {
                anyhow::anyhow!("Install the Supertonic CPU runtime to use this voice")
            })?;
            Some(SupertonicBundle::open(&cpu.runner, &cpu.models, cancel)?)
        } else {
            None
        };
        Ok(Self { breeze, cpu })
    }
    pub fn for_plan(
        config: &Config,
        plan: &NarrationPlan,
        selected: Option<VoiceChoice>,
        cancel: &Cancellation,
    ) -> anyhow::Result<Self> {
        let narrator = selected
            .or_else(|| config.cast.as_ref().map(|c| c.narrator))
            .unwrap_or(VoiceChoice::of(&config.voices[0].profile)?);
        let mut used = Vec::new();
        let choices = config
            .voices
            .iter()
            .map(|v| VoiceChoice::of(&v.profile))
            .collect::<Result<Vec<_>, _>>()?;
        for segment in &plan.spec().segments {
            let choice = config
                .cast
                .as_ref()
                .map(|c| {
                    c.resolve_with_narrator(segment.source, narrator)
                        .map(|(voice, _)| voice)
                })
                .transpose()?
                .unwrap_or(narrator);
            let index = choices
                .iter()
                .position(|c| *c == choice)
                .ok_or_else(|| anyhow::anyhow!("An assigned voice is missing"))?;
            let spec = &config.voices[index];
            if !used.iter().any(|v: &&VoiceSpec| std::ptr::eq(*v, spec)) {
                used.push(spec);
            }
        }
        Self::open(config, &used, cancel)
    }
    pub fn identity(
        &self,
        spec: &VoiceSpec,
        instruction: &str,
        asset: Option<&VoiceAsset>,
    ) -> anyhow::Result<phoenix_tts_contract::SynthesisIdentity> {
        if let Some(style) = &spec.supertonic_style {
            anyhow::ensure!(
                asset.is_none() && spec.profile.reference.is_none(),
                "CPU voices cannot use Breeze references"
            );
            Ok(self
                .cpu
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("CPU engine not enrolled"))?
                .identity(style, 1_440_000)?)
        } else {
            let bundle = self
                .breeze
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Breeze engine not enrolled"))?;
            Ok(match asset {
                Some(asset) => asset.identity(bundle, instruction, spec.profile.seed, 1_440_000)?,
                None => bundle.identity(instruction, spec.profile.seed, 1_440_000)?,
            })
        }
    }
}
pub struct Providers {
    pub breeze: Option<NativeProvider>,
    pub cpu: Option<SupertonicProvider>,
}
impl Providers {
    pub fn new(bundles: Bundles, storage: PathBuf) -> anyhow::Result<Self> {
        Ok(Self {
            breeze: bundles
                .breeze
                .map(|b| NativeProvider::new(b, Duration::from_secs(240), Duration::from_secs(120)))
                .transpose()?,
            cpu: bundles
                .cpu
                .map(|b| SupertonicProvider::new(b, storage.join("supertonic-jobs")))
                .transpose()?,
        })
    }
}
