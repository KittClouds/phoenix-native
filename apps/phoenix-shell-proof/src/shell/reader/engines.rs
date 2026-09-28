//! Lazy engine enrollment: a CPU-only session never requires a Breeze model.
use super::{voices::VoiceSpec, Config};
use phoenix_reader_session::{NarrationPlan, VoiceChoice};
use phoenix_tts_native::{
    supertonic::{SupertonicBundle, SupertonicProvider},
    Bundle, Cancellation, Engine, NativeProvider, VoiceAsset,
};
use std::path::PathBuf;
#[derive(serde::Deserialize)]
pub struct CpuConfig {
    pub runner: PathBuf,
    pub models: PathBuf,
}
/// Qwen3-TTS Base: the PBN1 worker (DLLs beside it), talker and codec GGUFs.
#[derive(Clone, serde::Deserialize)]
pub struct QwenConfig {
    pub worker: PathBuf,
    pub talker: PathBuf,
    pub codec: PathBuf,
}
impl QwenConfig {
    pub fn open(&self, cancel: &Cancellation) -> anyhow::Result<Bundle> {
        Ok(Bundle::open_qwen(&self.worker, &self.talker, &self.codec, cancel)?)
    }
}
/// At most one GPU engine per session: Breeze or Qwen, plus Supertonic on CPU.
pub struct Bundles {
    pub gpu: Option<Bundle>,
    pub cpu: Option<SupertonicBundle>,
}
impl Bundles {
    pub fn open(
        config: &Config,
        used: &[&VoiceSpec],
        cancel: &Cancellation,
    ) -> anyhow::Result<Self> {
        let breeze = used
            .iter()
            .any(|v| v.supertonic_style.is_none() && !v.qwen);
        let qwen = used.iter().any(|v| v.qwen);
        anyhow::ensure!(
            !(breeze && qwen),
            "This book mixes Breeze and Qwen voices. Cast it with one GPU engine."
        );
        let gpu = if qwen {
            let qwen = config
                .qwen
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Install the Qwen runtime to use this voice"))?;
            Some(qwen.open(cancel)?)
        } else if breeze {
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
        Ok(Self { gpu, cpu })
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
                .gpu
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("GPU voice engine not enrolled"))?;
            anyhow::ensure!(
                (bundle.engine() == Engine::Qwen) == spec.qwen,
                "Voice engine mismatch"
            );
            Ok(match asset {
                Some(asset) => asset.identity(bundle, instruction, spec.profile.seed, 1_440_000)?,
                None => bundle.identity(instruction, spec.profile.seed, 1_440_000)?,
            })
        }
    }
}
pub struct Providers {
    /// The session's GPU engine worker (Breeze or Qwen).
    pub gpu: Option<NativeProvider>,
    pub cpu: Option<SupertonicProvider>,
}
impl Providers {
    /// Adopts the warm Breeze worker when it runs the same files; the worker
    /// goes back to the warm slot when these providers drop.
    pub fn new(bundles: Bundles, storage: PathBuf) -> anyhow::Result<Self> {
        Ok(Self {
            gpu: bundles
                .gpu
                .map(|b| match super::warm::take(&b) {
                    Some(provider) => Ok(provider),
                    None => NativeProvider::new(
                        b,
                        super::warm::STARTUP_TIMEOUT,
                        super::warm::REQUEST_TIMEOUT,
                    )
                    .inspect_err(|_| super::warm::cancel_take()),
                })
                .transpose()?,
            cpu: bundles
                .cpu
                .map(|b| SupertonicProvider::new(b, storage.join("supertonic-jobs")))
                .transpose()?,
        })
    }
}
impl Drop for Providers {
    fn drop(&mut self) {
        if let Some(provider) = self.gpu.take() {
            super::warm::give_back(provider);
        }
    }
}
