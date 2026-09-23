use crate::{wire::*, Bundle, Error, Result, VoiceAsset};
use phoenix_reader_session::AudioCache;
use phoenix_tts_contract::{
    AlignmentLevel, AudioFormat, Binding, Capabilities, Digest, Envelope, Event, FinishReason,
    SynthesisRequest, BLOCK_FRAMES,
};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    os::windows::process::CommandExt,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Default)]
pub struct Cancellation(Arc<AtomicBool>);
impl Cancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}
pub struct Request<'a> {
    pub epoch: u64,
    pub plan: Digest,
    pub segment: u32,
    pub text: &'a str,
    pub instruction: &'a str,
    pub seed: u32,
    pub max_frames: u64,
}
pub struct PcmChunk<'a> {
    pub binding: Binding,
    pub first_frame: u64,
    pub pcm: &'a [u8],
}
struct Worker {
    child: Child,
    stream: TcpStream,
}
pub struct NativeProvider {
    bundle: Bundle,
    worker: Option<Worker>,
    unreaped: Option<Child>,
    next_request: u64,
    last_epoch: u64,
    startup_timeout: Duration,
    request_timeout: Duration,
    unavailable: bool,
}
impl NativeProvider {
    pub fn new(
        bundle: Bundle,
        startup_timeout: Duration,
        request_timeout: Duration,
    ) -> Result<Self> {
        if startup_timeout.is_zero()
            || request_timeout.is_zero()
            || startup_timeout > Duration::from_secs(600)
            || request_timeout > Duration::from_secs(600)
        {
            return Err(Error::Invalid("deadline bounds"));
        }
        Ok(Self {
            bundle,
            worker: None,
            unreaped: None,
            next_request: 1,
            last_epoch: 0,
            startup_timeout,
            request_timeout,
            unavailable: false,
        })
    }
    pub fn bundle(&self) -> &Bundle {
        &self.bundle
    }
    pub fn pid(&self) -> Option<u32> {
        self.worker.as_ref().map(|w| w.child.id())
    }
    pub fn generate(
        &mut self,
        request: Request<'_>,
        cache: &mut AudioCache,
        cancel: &Cancellation,
    ) -> Result<Digest> {
        self.generate_streamed(request, cache, cancel, |_| Ok(()))
    }
    /// The sink borrows a bounded PCM block and must return promptly. It is
    /// ephemeral audio: a sink error/cancel aborts publication and kills the worker.
    /// Playback consumers must fence this stream with the request's epoch.
    pub fn generate_streamed<F>(
        &mut self,
        r: Request<'_>,
        cache: &mut AudioCache,
        cancel: &Cancellation,
        sink: F,
    ) -> Result<Digest>
    where
        F: FnMut(PcmChunk<'_>) -> Result<()>,
    {
        self.generate_voiced_streamed(r, None, cache, cancel, sink)
    }
    pub fn generate_voiced_streamed<F>(
        &mut self,
        r: Request<'_>,
        voice: Option<&VoiceAsset>,
        cache: &mut AudioCache,
        cancel: &Cancellation,
        mut sink: F,
    ) -> Result<Digest>
    where
        F: FnMut(PcmChunk<'_>) -> Result<()>,
    {
        if self.unavailable {
            return Err(Error::Invalid("worker exit not confirmed"));
        }
        if r.epoch == 0
            || r.epoch < self.last_epoch
            || r.plan == [0; 32]
            || r.text.trim().is_empty()
            || r.text.len() > 16_384
            || r.instruction.len() > 4096
            || r.max_frames < 1920
            || r.max_frames > 1_440_000
        {
            return Err(Error::Invalid("request bounds or stale epoch"));
        }
        check(cancel, Instant::now() + self.request_timeout)?;
        self.last_epoch = r.epoch;
        let id = self.next_request;
        self.next_request = id
            .checked_add(1)
            .ok_or(Error::Invalid("request counter exhausted"))?;
        let identity = match voice {
            Some(voice) => voice.identity(&self.bundle, r.instruction, r.seed, r.max_frames)?,
            None => self.bundle.identity(r.instruction, r.seed, r.max_frames)?,
        };
        let key = identity.audio_key(r.text)?;
        // Existing hits are validated by AudioCache::get; no model work is required.
        if cache.contains(key) {
            cache.get(key)?;
            return Ok(key);
        }
        let result = (|| {
            self.start(cancel)?;
            let deadline = Instant::now() + self.request_timeout;
            let stream = &mut self.worker.as_mut().unwrap().stream;
            send(
                stream,
                &Header {
                    kind: if voice.is_some() {
                        VOICED_REQUEST
                    } else {
                        REQUEST
                    },
                    request: id,
                    sequence: r.seed.into(),
                    value: r.max_frames,
                }
                .encode(),
                cancel,
                deadline,
            )?;
            let mut lengths = [0u8; 8];
            lengths[..4].copy_from_slice(&(r.text.len() as u32).to_le_bytes());
            lengths[4..].copy_from_slice(&(r.instruction.len() as u32).to_le_bytes());
            send(stream, &lengths, cancel, deadline)?;
            send(stream, r.text.as_bytes(), cancel, deadline)?;
            send(stream, r.instruction.as_bytes(), cancel, deadline)?;
            if let Some(voice) = voice {
                send(
                    stream,
                    &(voice.bytes().len() as u32).to_le_bytes(),
                    cancel,
                    deadline,
                )?;
                send(stream, voice.bytes(), cancel, deadline)?;
            }
            let started = header(stream, cancel, deadline)?;
            if started.kind != STARTED
                || started.request != id
                || started.sequence != 0
                || started.value == 0
                || started.value > 2048
            {
                return Err(Error::Invalid("worker admission failed"));
            }
            let binding = Binding {
                request: id,
                epoch: r.epoch,
                plan: r.plan,
                segment: r.segment,
                audio_key: key,
            };
            SynthesisRequest::new(
                binding,
                &identity,
                r.text,
                Capabilities {
                    provider: identity.provider,
                    format: AudioFormat::PCM24,
                    max_context_tokens: 2048,
                    max_output_frames: 1_440_000,
                    concurrency: 1,
                    cancellation: true,
                    normal_finish_reason: true,
                    alignment: AlignmentLevel::Segment,
                },
                started.value as u32,
                r.max_frames,
            )?;
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
            let mut sequence = 1;
            let mut frames = 0u64;
            let mut pcm = [0u8; BLOCK_FRAMES * 2];
            loop {
                let h = header(stream, cancel, deadline)?;
                if h.request != id || h.sequence != sequence {
                    return Err(Error::Invalid("stale or out-of-order worker event"));
                }
                match h.kind {
                    AUDIO
                        if h.value > 0
                            && h.value <= BLOCK_FRAMES as u64
                            && h.value <= r.max_frames - frames =>
                    {
                        let bytes = &mut pcm[..h.value as usize * 2];
                        receive(stream, bytes, cancel, deadline)?;
                        writer.push(
                            Envelope {
                                binding,
                                sequence,
                                event: Event::AudioChunk {
                                    first_frame: frames,
                                    frames: h.value as u32,
                                },
                            },
                            bytes,
                        )?;
                        sink(PcmChunk {
                            binding,
                            first_frame: frames,
                            pcm: bytes,
                        })?;
                        check(cancel, deadline)?;
                        frames += h.value;
                        sequence += 1;
                    }
                    EOS if h.value == frames && frames > 0 => {
                        send(
                            stream,
                            &Header {
                                kind: BARRIER,
                                request: id,
                                sequence,
                                value: frames,
                            }
                            .encode(),
                            cancel,
                            deadline,
                        )?;
                        let quiet = header(stream, cancel, deadline)?;
                        if quiet
                            != (Header {
                                kind: QUIET,
                                request: id,
                                sequence: sequence + 1,
                                value: frames,
                            })
                        {
                            return Err(Error::Invalid("missing quiescence receipt"));
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
                        return Ok(key);
                    }
                    _ => return Err(Error::Invalid("non-normal or malformed generation")),
                }
            }
        })();
        if result.is_err() {
            self.stop()?;
        }
        result
    }
    fn start(&mut self, cancel: &Cancellation) -> Result<()> {
        if let Some(worker) = &mut self.worker {
            if worker.child.try_wait()?.is_none() {
                return Ok(());
            }
            self.worker = None;
        }
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        listener.set_nonblocking(true)?;
        let nonce = *uuid::Uuid::new_v4().as_bytes();
        let nonce_hex = nonce.iter().map(|b| format!("{b:02x}")).collect::<String>();
        let system = std::env::var_os("SystemRoot").ok_or(Error::Invalid("missing SystemRoot"))?;
        let path = std::env::join_paths([
            self.bundle.dll_directory.clone(),
            std::path::PathBuf::from(&system).join("System32"),
        ])
        .map_err(|_| Error::Invalid("DLL search path"))?;
        let mut child = Command::new(&self.bundle.executable)
            .current_dir(&self.bundle.dll_directory)
            .arg(&self.bundle.model_path)
            .arg(listener.local_addr()?.port().to_string())
            .arg(nonce_hex)
            .env_clear()
            .env("SystemRoot", system)
            .env("PATH", path)
            .env("GGML_VK_DISABLE_HOST_VISIBLE_VIDMEM", "1")
            .env("GGML_VK_DISABLE_COOPMAT2", "1")
            .creation_flags(0x08000000)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let deadline = Instant::now() + self.startup_timeout;
        let accepted = (|| loop {
            check(cancel, deadline)?;
            if child.try_wait()?.is_some() {
                return Err(Error::Invalid("worker exited during startup"));
            }
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(true)?;
                    stream.set_nodelay(true)?;
                    let mut auth = [0; 16];
                    receive(&mut stream, &mut auth, cancel, deadline)?;
                    if auth != nonce {
                        return Err(Error::Invalid("worker authentication"));
                    }
                    if header(&mut stream, cancel, deadline)?
                        != (Header {
                            kind: READY,
                            request: 0,
                            sequence: 0,
                            value: 24000,
                        })
                    {
                        return Err(Error::Invalid("worker readiness"));
                    }
                    return Ok(stream);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(2))
                }
                Err(e) => return Err(e.into()),
            }
        })();
        match accepted {
            Ok(stream) => {
                self.worker = Some(Worker { child, stream });
                Ok(())
            }
            Err(e) => {
                if let Err(exit_error) = terminate(&mut child) {
                    self.unavailable = true;
                    self.unreaped = Some(child);
                    return Err(exit_error);
                }
                Err(e)
            }
        }
    }
    pub fn stop(&mut self) -> Result<()> {
        if let Some(mut child) = self.unreaped.take() {
            if let Err(e) = terminate(&mut child) {
                self.unreaped = Some(child);
                self.unavailable = true;
                return Err(e);
            }
        }
        if let Some(mut worker) = self.worker.take() {
            let _ = worker.stream.shutdown(std::net::Shutdown::Both);
            if let Err(e) = terminate(&mut worker.child) {
                self.unavailable = true;
                self.worker = Some(worker);
                return Err(e);
            }
        }
        self.unavailable = false;
        Ok(())
    }
}
impl Drop for NativeProvider {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
fn terminate(child: &mut Child) -> Result<()> {
    if child.try_wait()?.is_none() {
        if let Err(e) = child.kill() {
            if child.try_wait()?.is_none() {
                return Err(e.into());
            }
        }
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    while child.try_wait()?.is_none() {
        if Instant::now() >= deadline {
            return Err(Error::Invalid("worker exit not confirmed"));
        }
        thread::sleep(Duration::from_millis(2));
    }
    Ok(())
}
fn check(cancel: &Cancellation, deadline: Instant) -> Result<()> {
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(Error::Timeout);
    }
    Ok(())
}
fn send(
    stream: &mut TcpStream,
    mut bytes: &[u8],
    cancel: &Cancellation,
    deadline: Instant,
) -> Result<()> {
    while !bytes.is_empty() {
        check(cancel, deadline)?;
        match stream.write(bytes) {
            Ok(0) => return Err(Error::Invalid("worker write closed")),
            Ok(n) => bytes = &bytes[n..],
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(2))
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
fn receive(
    stream: &mut TcpStream,
    mut bytes: &mut [u8],
    cancel: &Cancellation,
    deadline: Instant,
) -> Result<()> {
    while !bytes.is_empty() {
        check(cancel, deadline)?;
        match stream.read(bytes) {
            Ok(0) => return Err(Error::Invalid("worker transport EOF")),
            Ok(n) => bytes = &mut bytes[n..],
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(2))
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
fn header(stream: &mut TcpStream, cancel: &Cancellation, deadline: Instant) -> Result<Header> {
    let mut bytes = [0; HEADER];
    receive(stream, &mut bytes, cancel, deadline)?;
    Header::decode(bytes)
}
