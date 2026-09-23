//! CPU Supertonic CLI adapter. Process success plus a complete, validated WAV
//! establishes transport completion; neither implies transcript correctness.
//! The runner has no streaming/seed API: bounded PCM publication starts after
//! process exit. Stochastic first renders are retained by their input identity.
#[path = "supertonic_wav.rs"]
mod wav;
use crate::{bundle::pin, Cancellation, Error, PcmChunk, Request, Result};
use phoenix_reader_session::AudioCache;
use phoenix_tts_contract::{
    digest, AudioFormat, Binding, Digest, Envelope, Event, FinishReason, SynthesisIdentity,
    BLOCK_FRAMES,
};
use std::{
    fs::{File, OpenOptions},
    os::windows::{fs::OpenOptionsExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub const STYLES: [&str; 10] = ["F1", "F2", "F3", "F4", "F5", "M1", "M2", "M3", "M4", "M5"];
pub struct SupertonicBundle {
    runner: PathBuf,
    root: PathBuf,
    runtime: Digest,
    model: Digest,
    styles: [Digest; 10],
    _files: Vec<File>,
}
impl SupertonicBundle {
    pub fn open(runner: &Path, root: &Path, cancel: &Cancellation) -> Result<Self> {
        let runner = runner.canonicalize()?;
        let root = cli_path(root.canonicalize()?)?;
        let mut files = Vec::with_capacity(20);
        let exe = pin(&runner, 256 * 1024 * 1024, &mut files, cancel)?;
        let mut dlls: Vec<_> =
            std::fs::read_dir(runner.parent().ok_or(Error::Invalid("runner parent"))?)?
                .map(|e| e.map(|e| e.path()))
                .collect::<std::io::Result<_>>()?;
        dlls.retain(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("dll")));
        dlls.sort();
        if dlls.len() > 32 {
            return Err(Error::Invalid("Supertonic DLL bounds"));
        }
        let mut hashes = vec![exe];
        for path in dlls {
            hashes.push(pin(&path, 512 * 1024 * 1024, &mut files, cancel)?);
        }
        let runtime = digest(b"phoenix.supertonic-cpu/runtime/v1", &hashes)?;
        hashes.clear();
        for name in [
            "duration_predictor.onnx",
            "text_encoder.onnx",
            "vector_estimator.onnx",
            "vocoder.onnx",
            "tts.json",
            "unicode_indexer.json",
        ] {
            hashes.push(pin(
                &root.join("onnx").join(name),
                1024 * 1024 * 1024,
                &mut files,
                cancel,
            )?);
        }
        let model = digest(b"phoenix.supertonic-cpu/model/v1", &hashes)?;
        let mut styles = [[0; 32]; 10];
        for (i, style) in STYLES.iter().enumerate() {
            styles[i] = pin(
                &root.join("voice_styles").join(format!("{style}.json")),
                4 * 1024 * 1024,
                &mut files,
                cancel,
            )?;
        }
        Ok(Self {
            runner,
            root,
            runtime,
            model,
            styles,
            _files: files,
        })
    }
    pub fn identity(&self, style: &str, max_frames: u64) -> Result<SynthesisIdentity> {
        let index = STYLES
            .iter()
            .position(|s| *s == style)
            .ok_or(Error::Invalid("Unknown Supertonic style"))?;
        let hash = |text: &[u8]| *blake3::hash(text).as_bytes();
        let identity = SynthesisIdentity {
            provider: hash(b"phoenix.supertonic-cpu/v1"),
            runtime: self.runtime,
            model: self.model,
            tokenizer: self.model,
            codec: self.model,
            voice: self.styles[index],
            reference_audio: None,
            reference_transcript: None,
            direction: hash(b"fixed-style/no-direction"),
            generation_config: digest(
                b"phoenix.supertonic-config/v1",
                &(
                    style,
                    max_frames,
                    "cpu;en;steps5;speed1.05;seed-unavailable",
                ),
            )?,
            transformations: hash(b"supertonic-upstream-normalization/v1"),
            postprocessing: hash(b"mono16/44100-to-24000/64tap-blackman-sinc/v1"),
            seed: 0,
            format: AudioFormat::PCM24,
        };
        identity.validate()?;
        Ok(identity)
    }
}
pub struct SupertonicProvider {
    bundle: SupertonicBundle,
    output: PathBuf,
    next: u64,
    epoch: u64,
    unreaped: Option<Child>,
}
impl SupertonicProvider {
    pub fn new(bundle: SupertonicBundle, output: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&output)?;
        Ok(Self {
            bundle,
            output: cli_path(output.canonicalize()?)?,
            next: 1,
            epoch: 0,
            unreaped: None,
        })
    }
    pub fn generate<F>(
        &mut self,
        r: Request<'_>,
        style: &str,
        cache: &mut AudioCache,
        cancel: &Cancellation,
        mut sink: F,
    ) -> Result<Digest>
    where
        F: FnMut(PcmChunk<'_>) -> Result<()>,
    {
        if self.unreaped.is_some() {
            return Err(Error::Invalid("Supertonic process exit unconfirmed"));
        }
        if r.epoch == 0
            || r.epoch < self.epoch
            || r.plan == [0; 32]
            || r.text.trim().is_empty()
            || r.text.len() > 2048
            || r.text.contains('\0')
            || r.max_frames == 0
            || r.max_frames > 1_440_000
        {
            return Err(Error::Invalid("Supertonic request bounds"));
        }
        let deadline = Instant::now() + Duration::from_secs(120);
        check(cancel, deadline)?;
        let identity = self.bundle.identity(style, r.max_frames)?;
        let key = identity.audio_key(r.text)?;
        self.epoch = r.epoch;
        if cache.contains(key) {
            cache.get(key)?;
            return Ok(key);
        }
        let request = self.next;
        self.next = request
            .checked_add(1)
            .ok_or(Error::Invalid("Request counter exhausted"))?;
        let job = JobDir(self.output.join(uuid::Uuid::new_v4().to_string()));
        std::fs::create_dir(&job.0)?;
        let mut child = Command::new(&self.bundle.runner)
            .current_dir(self.bundle.runner.parent().unwrap())
            .args(["--onnx-dir"])
            .arg(self.bundle.root.join("onnx"))
            .arg("--voice-style")
            .arg(
                self.bundle
                    .root
                    .join("voice_styles")
                    .join(format!("{style}.json")),
            )
            .arg("--text")
            .arg(r.text)
            .args([
                "--lang",
                "en",
                "--n-test",
                "1",
                "--total-step",
                "5",
                "--speed",
                "1.05",
            ])
            .arg("--save-dir")
            .arg(&job.0)
            .creation_flags(0x08000000)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let exit = loop {
            let state = check(cancel, deadline).and_then(|_| child.try_wait().map_err(Error::from));
            match state {
                Ok(Some(status)) => break status,
                Ok(None) => thread::sleep(Duration::from_millis(20)),
                Err(error) => {
                    let _ = child.kill();
                    let until = Instant::now() + Duration::from_secs(2);
                    loop {
                        if matches!(child.try_wait(), Ok(Some(_))) {
                            return Err(error);
                        }
                        if Instant::now() >= until {
                            self.unreaped = Some(child);
                            return Err(Error::Invalid("Supertonic process exit unconfirmed"));
                        }
                        thread::sleep(Duration::from_millis(10));
                    }
                }
            }
        };
        if !exit.success() {
            return Err(Error::Invalid("Supertonic runner failed"));
        }
        check(cancel, deadline)?;
        let mut output = None;
        for entry in std::fs::read_dir(&job.0)? {
            let entry = entry?;
            if entry
                .path()
                .extension()
                .is_some_and(|s| s.eq_ignore_ascii_case("wav"))
            {
                if output.is_some() || !entry.file_type()?.is_file() {
                    return Err(Error::Invalid("Ambiguous Supertonic output"));
                }
                output = Some(entry.path());
            }
        }
        let file = OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(output.ok_or(Error::Invalid("Missing Supertonic WAV"))?)?;
        if file.metadata()?.len() > 16 * 1024 * 1024 {
            return Err(Error::Invalid("Supertonic WAV bounds"));
        }
        // SAFETY: exclusive read-sharing denies mutation/deletion until map drops.
        let map = unsafe { memmap2::MmapOptions::new().map(&file)? };
        let audio = wav::PcmWave::parse(&map)?;
        let frames = audio.output_frames();
        if frames == 0 || frames > r.max_frames {
            return Err(Error::Invalid("Supertonic output frame bounds"));
        }
        let binding = Binding {
            epoch: r.epoch,
            request,
            plan: r.plan,
            segment: r.segment,
            audio_key: key,
        };
        let provider = identity.provider;
        let mut writer = cache.begin(binding, identity, r.text, r.max_frames)?;
        writer.push(
            Envelope {
                binding,
                sequence: 0,
                event: Event::Started {
                    provider,
                    format: AudioFormat::PCM24,
                },
            },
            &[],
        )?;
        let mut pcm = [0u8; BLOCK_FRAMES * 2];
        let filter = wav::Resampler::new();
        let mut first = 0;
        let mut sequence = 1;
        while first < frames {
            check(cancel, deadline)?;
            let count = (frames - first).min(BLOCK_FRAMES as u64) as usize;
            filter.render(&audio, first, &mut pcm[..count * 2]);
            writer.push(
                Envelope {
                    binding,
                    sequence,
                    event: Event::AudioChunk {
                        first_frame: first,
                        frames: count as u32,
                    },
                },
                &pcm[..count * 2],
            )?;
            sink(PcmChunk {
                binding,
                first_frame: first,
                pcm: &pcm[..count * 2],
            })?;
            first += count as u64;
            sequence += 1;
        }
        check(cancel, deadline)?;
        writer.finish(
            Envelope {
                binding,
                sequence,
                event: Event::Completed {
                    frames,
                    reason: FinishReason::Normal,
                },
            },
            None,
        )?;
        Ok(key)
    }
}
impl Drop for SupertonicProvider {
    fn drop(&mut self) {
        if let Some(child) = &mut self.unreaped {
            let _ = child.kill();
        }
    }
}
struct JobDir(PathBuf);
impl Drop for JobDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn check(cancel: &Cancellation, deadline: Instant) -> Result<()> {
    if cancel.is_cancelled() {
        Err(Error::Cancelled)
    } else if Instant::now() >= deadline {
        Err(Error::Timeout)
    } else {
        Ok(())
    }
}

// The upstream Rust CLI joins paths with forward slashes. Windows verbatim
// paths reject those separators, so pass equivalent ordinary absolute paths.
fn cli_path(path: PathBuf) -> Result<PathBuf> {
    let value = path
        .to_str()
        .ok_or(Error::Invalid("CLI paths must be Unicode"))?;
    if let Some(unc) = value.strip_prefix(r"\\?\UNC\") {
        Ok(PathBuf::from(format!(r"\\{unc}")))
    } else if let Some(local) = value.strip_prefix(r"\\?\") {
        Ok(PathBuf::from(local))
    } else {
        Ok(path)
    }
}
