//! One generation job and one completed mmap lease in flight. Never share the
//! mutable cache or provider with the device-control thread.
use super::voices::PreparedVoice;
use phoenix_reader_session::{AudioCache, CachedAudio, Digest};
use phoenix_tts_native::{Cancellation, Request};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::{
    sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender},
    thread,
    time::{Duration, Instant},
};

// A brief pause keeps the warm model; a longer pause gives the GPU back to
// graph and analysis work. The bundle remains hash-pinned for the next request.
const BREEZE_IDLE_RELEASE: Duration = Duration::from_secs(20);

pub struct Job {
    pub epoch: u64,
    pub segment: u32,
    pub plan: Digest,
    pub key: Digest,
    pub text: String,
    pub voice: Arc<PreparedVoice>,
    pub cancel: Cancellation,
}
pub struct Receipt {
    pub epoch: u64,
    pub segment: u32,
    pub seconds: f64,
    pub generated: bool,
    pub result: Result<CachedAudio, String>,
}
pub struct Generator {
    tx: Option<SyncSender<Job>>,
    pub rx: Receiver<Receipt>,
    token: Option<Cancellation>,
    active: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
}
impl Generator {
    pub fn new(mut provider: super::engines::Providers, mut cache: AudioCache) -> Self {
        let (tx, requests) = mpsc::sync_channel::<Job>(1);
        let (results, rx) = mpsc::sync_channel(1);
        let active = Arc::new(AtomicBool::new(false));
        let thread_active = Arc::clone(&active);
        let join = thread::spawn(move || {
            let mut last_request = Instant::now();
            let mut parked = false;
            loop {
                let job = match requests.recv_timeout(Duration::from_millis(100)) {
                    Ok(job) => job,
                    Err(RecvTimeoutError::Timeout) => {
                        if !parked
                            && !thread_active.load(Ordering::Acquire)
                            && last_request.elapsed() >= BREEZE_IDLE_RELEASE
                        {
                            if let Some(breeze) = provider.breeze.as_mut() {
                                if breeze.pid().is_some() {
                                    if let Err(error) = breeze.stop() {
                                        tracing::warn!(%error, "Breeze idle release failed");
                                    }
                                }
                            }
                            parked = true;
                        }
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                };
                parked = false;
                let start = Instant::now();
                let generated = !cache.contains(job.key);
                let result = (|| -> anyhow::Result<CachedAudio> {
                    if job.cancel.is_cancelled() {
                        anyhow::bail!("superseded request");
                    }
                    if generated {
                        let request = Request {
                            epoch: job.epoch,
                            plan: job.plan,
                            segment: job.segment,
                            text: &job.text,
                            instruction: &job.voice.instruction,
                            seed: job.voice.seed,
                            max_frames: 1_440_000,
                        };
                        if let Some(style) = &job.voice.supertonic_style {
                            provider
                                .cpu
                                .as_mut()
                                .ok_or_else(|| anyhow::anyhow!("CPU provider unavailable"))?
                                .generate(request, style, &mut cache, &job.cancel, |_| Ok(()))?;
                        } else {
                            provider
                                .breeze
                                .as_mut()
                                .ok_or_else(|| anyhow::anyhow!("Breeze provider unavailable"))?
                                .generate_voiced_streamed(
                                    request,
                                    job.voice.asset.as_ref(),
                                    &mut cache,
                                    &job.cancel,
                                    |_| Ok(()),
                                )?;
                        }
                    }
                    if job.cancel.is_cancelled() {
                        anyhow::bail!("superseded request");
                    }
                    Ok(cache.get(job.key)?)
                })()
                .map_err(|e| format!("{e:#}"));
                last_request = Instant::now();
                // There is at most one outstanding job; the result slot is empty.
                if results
                    .send(Receipt {
                        epoch: job.epoch,
                        segment: job.segment,
                        seconds: start.elapsed().as_secs_f64(),
                        generated,
                        result,
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            tx: Some(tx),
            rx,
            token: None,
            active,
            join: Some(join),
        }
    }
    pub fn set_active(&self, active: bool) {
        self.active.store(active, Ordering::Release);
    }
    pub fn submit(&mut self, job: Job) -> anyhow::Result<()> {
        self.token = Some(job.cancel.clone());
        self.tx
            .as_ref()
            .unwrap()
            .try_send(job)
            .map_err(|e| anyhow::anyhow!("prefetch queue: {e}"))
    }
    pub fn cancel(&self) {
        if let Some(token) = &self.token {
            token.cancel();
        }
    }
}
impl Drop for Generator {
    fn drop(&mut self) {
        self.cancel();
        self.tx.take();
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}
