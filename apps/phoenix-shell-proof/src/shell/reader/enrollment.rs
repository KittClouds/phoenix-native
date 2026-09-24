//! User-initiated Breeze reference enrollment. The book worker is retired
//! before this cold path starts so two model instances never contend for VRAM.
use super::worker;
use anyhow::{Context as _, Result};
use phoenix_reader_session::{VoiceChoice, VoiceLibrary, VoiceProfile, VoiceReference};
use phoenix_tts_native::{Bundle, Cancellation, VoiceAsset, MAX_VOICE_BYTES};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::windows::{fs::OpenOptionsExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub(super) struct Request {
    pub config: worker::Config,
    pub audio: PathBuf,
    pub name: String,
    pub transcript: String,
    pub direction: String,
}

pub(super) struct Enrollment {
    cancel: Cancellation,
    join: Option<thread::JoinHandle<Result<(String, VoiceChoice)>>>,
}
impl Enrollment {
    pub fn start(
        request: Request,
        previous: Option<worker::Bridge>,
        audition: Option<super::audition::Audition>,
    ) -> Self {
        let cancel = Cancellation::default();
        let token = cancel.clone();
        let join = thread::spawn(move || {
            if let Some(previous) = previous {
                previous.shutdown();
            }
            if let Some(audition) = audition {
                audition.shutdown();
            }
            anyhow::ensure!(!token.is_cancelled(), "Enrollment cancelled");
            let name = request.name.clone();
            let choice = enroll(request, &token)?;
            Ok((name, choice))
        });
        Self {
            cancel,
            join: Some(join),
        }
    }
    pub fn cancel(&self) {
        self.cancel.cancel();
    }
    pub fn finished(&self) -> bool {
        self.join.as_ref().is_some_and(|join| join.is_finished())
    }
    pub fn finish(mut self) -> Result<(String, VoiceChoice)> {
        let join = self.join.take().context("Enrollment worker missing")?;
        join.join()
            .map_err(|_| anyhow::anyhow!("Enrollment worker panicked"))?
    }
}
impl Drop for Enrollment {
    fn drop(&mut self) {
        self.cancel.cancel();
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn enroll(request: Request, cancel: &Cancellation) -> Result<VoiceChoice> {
    let Request {
        config,
        audio,
        name,
        transcript,
        direction,
    } = request;
    anyhow::ensure!(
        !config.worker.as_os_str().is_empty(),
        "Breeze worker is not configured"
    );
    let cli = config.worker.with_file_name("breeze-cli.exe");
    let source = OpenOptions::new().read(true).share_mode(1).open(&audio)?;
    let size = source.metadata()?.len();
    anyhow::ensure!(
        (44..=16 * 1024 * 1024).contains(&size),
        "Reference WAV must be under 16 MiB"
    );
    let mut original = Vec::with_capacity(size as usize);
    (&source).read_to_end(&mut original)?;
    validate_wav(&original)?;
    anyhow::ensure!(!cancel.is_cancelled(), "Enrollment cancelled");
    let bundle =
        Bundle::open_cancellable(&config.worker, &config.model, &config.dll_directory, cancel)?;
    let identity = bundle.identity("", 42, 1_440_000)?;
    // The encoder is a separate process; release this model before it claims VRAM.
    drop(bundle);
    let root = config.storage.join("voices");
    fs::create_dir_all(&root)?;
    let temporary = tempfile::tempdir_in(&root)?;
    let mut child = Command::new(&cli)
        .arg(&config.model)
        .args(["--ref-audio"])
        .arg(&audio)
        .args(["--ref-text"])
        .arg(&transcript)
        .args(["--save-voice", "reference", "--voices-dir"])
        .arg(temporary.path())
        .env(
            "PATH",
            format!(
                "{};{}",
                config.dll_directory.display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .creation_flags(0x08000000)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Start Breeze reference encoder")?;
    let deadline = Instant::now() + Duration::from_secs(240);
    let status = loop {
        if cancel.is_cancelled() || Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!(if cancel.is_cancelled() {
                "Enrollment cancelled"
            } else {
                "Reference encoding timed out"
            });
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
        thread::sleep(Duration::from_millis(40));
    };
    anyhow::ensure!(
        status.success(),
        "Breeze could not encode this WAV and transcript"
    );
    let encoded_path = temporary.path().join("reference.breeze");
    anyhow::ensure!(
        fs::metadata(&encoded_path)?.len() <= MAX_VOICE_BYTES,
        "Encoded voice too large"
    );
    let encoded_bytes = fs::read(&encoded_path)?;
    let encoded = *blake3::hash(&encoded_bytes).as_bytes();
    let asset = VoiceAsset::open(&encoded_path, encoded, identity.model, identity.codec)?;
    anyhow::ensure!(
        asset.transcript() == transcript,
        "Encoded transcript differs"
    );
    anyhow::ensure!(!cancel.is_cancelled(), "Enrollment cancelled");
    let destination = root.join(format!("{}.breeze", blake3::Hash::from(encoded).to_hex()));
    publish(&root, &destination, &encoded_bytes)?;
    let original_hash = *blake3::hash(&original).as_bytes();
    let source_copy = root.join(format!(
        "{}.wav",
        blake3::Hash::from(original_hash).to_hex()
    ));
    publish(&root, &source_copy, &original)?;
    anyhow::ensure!(!cancel.is_cancelled(), "Enrollment cancelled");
    let mut id = blake3::Hasher::new();
    id.update(b"phoenix.user-reference-profile/v1\0");
    id.update(name.as_bytes());
    id.update(&encoded);
    id.update(direction.as_bytes());
    let profile = VoiceProfile {
        id: *id.finalize().as_bytes(),
        revision: 1,
        name,
        description: "Reference voice created from your recording.".into(),
        reference: Some(VoiceReference {
            encoded,
            original_audio: original_hash,
            transcript: *blake3::hash(transcript.as_bytes()).as_bytes(),
            model: identity.model,
            codec: identity.codec,
        }),
        default_delivery: direction,
        seed: 42,
    };
    let library = VoiceLibrary::open(&root)?;
    Ok(library.save(&profile)?)
}

fn publish(root: &Path, path: &Path, bytes: &[u8]) -> Result<()> {
    if path.exists() {
        anyhow::ensure!(
            *blake3::hash(&fs::read(path)?).as_bytes() == *blake3::hash(bytes).as_bytes(),
            "Installed reference hash mismatch"
        );
        return Ok(());
    }
    let mut file = tempfile::NamedTempFile::new_in(root)?;
    file.write_all(bytes)?;
    file.as_file_mut().sync_all()?;
    file.persist(path).context("Publish reference asset")?;
    Ok(())
}

fn validate_wav(bytes: &[u8]) -> Result<()> {
    anyhow::ensure!(
        bytes.len() >= 44 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WAVE",
        "Select a RIFF WAV recording"
    );
    let mut offset = 12usize;
    let mut format = None;
    let mut data = None;
    while offset + 8 <= bytes.len() {
        let len = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into()?) as usize;
        let start = offset + 8;
        let end = start.checked_add(len).context("WAV chunk overflow")?;
        anyhow::ensure!(end <= bytes.len(), "Truncated WAV chunk");
        match &bytes[offset..offset + 4] {
            b"fmt " if len >= 16 => {
                let u16_at = |at| u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap());
                format = Some((
                    u16_at(start),
                    u16_at(start + 2),
                    u32::from_le_bytes(bytes[start + 4..start + 8].try_into()?),
                    u16_at(start + 14),
                ));
            }
            b"data" => data = Some(len),
            _ => {}
        }
        offset = end + (len & 1);
    }
    let (kind, channels, rate, bits) = format.context("WAV format missing")?;
    let data = data.context("WAV audio missing")?;
    anyhow::ensure!(
        (kind == 1 && matches!(bits, 16 | 32)) || (kind == 3 && bits == 32),
        "Use PCM16, PCM32, or float32 WAV"
    );
    anyhow::ensure!(
        (1..=2).contains(&channels) && (16_000..=96_000).contains(&rate),
        "Use mono or stereo WAV at 16–96 kHz"
    );
    let frame_bytes = usize::from(channels) * usize::from(bits / 8);
    anyhow::ensure!(data % frame_bytes == 0, "WAV frame alignment");
    let seconds = data as f64 / (frame_bytes as f64 * rate as f64);
    anyhow::ensure!(
        (3.0..=30.0).contains(&seconds),
        "Use a clean 3–30 second voice excerpt"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_wav;

    #[test]
    fn reference_wav_bounds_cover_common_stereo_recordings() {
        let frames = 44_100usize * 4;
        let data = frames * 2 * 2;
        let mut wav = Vec::with_capacity(44 + data);
        wav.extend(b"RIFF");
        wav.extend(((36 + data) as u32).to_le_bytes());
        wav.extend(b"WAVEfmt ");
        wav.extend(16u32.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(2u16.to_le_bytes());
        wav.extend(44_100u32.to_le_bytes());
        wav.extend(176_400u32.to_le_bytes());
        wav.extend(4u16.to_le_bytes());
        wav.extend(16u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend((data as u32).to_le_bytes());
        wav.resize(44 + data, 0);
        assert!(validate_wav(&wav).is_ok());
        wav.truncate(44 + data - 2);
        assert!(validate_wav(&wav).is_err());
    }
}
