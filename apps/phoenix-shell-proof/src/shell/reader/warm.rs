//! One warm Breeze worker shared across Reader sessions.
//!
//! Opening the Reader starts the worker and speaks a short throwaway line, so
//! model upload and GPU pipeline setup are finished before Listen. Sessions
//! (and voice samples) adopt the parked worker and hand it back when they
//! end. It is released after `IDLE_RELEASE` without work, or when the Reader
//! closes, so the GPU memory returns to the graph and analysis.
use super::Config;
use phoenix_reader_session::{AudioCache, VoiceChoice};
use phoenix_tts_native::{Bundle, Cancellation, NativeProvider, Request, VoiceAsset};
use std::{
    path::PathBuf,
    sync::{Condvar, Mutex},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// How long an idle Breeze worker keeps its GPU memory, parked or in a session.
pub const IDLE_RELEASE: Duration = Duration::from_secs(180);
pub const STARTUP_TIMEOUT: Duration = Duration::from_secs(240);
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

struct State {
    parked: Option<NativeProvider>,
    /// A warm-up is in progress; adopters wait for it instead of starting a
    /// second worker (two would not fit beside the atlas on a 10 GB card).
    warming: bool,
    /// Providers currently owned by sessions or samples.
    borrowed: usize,
    /// The Reader is open; parked workers are worth keeping.
    wanted: bool,
    /// Invalidates pending idle releases when the slot changes.
    ticket: u64,
}

static STATE: Mutex<State> = Mutex::new(State {
    parked: None,
    warming: false,
    borrowed: 0,
    wanted: false,
    ticket: 0,
});
static WARMED: Condvar = Condvar::new();

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Starts warming a worker in the background unless one is parked, warming
/// or already serving a session. A CPU (Supertonic) narrator needs no Breeze
/// worker, so none is started for it.
pub fn prewarm(workspace: PathBuf, narrator: Option<VoiceChoice>) {
    {
        let mut s = state();
        s.wanted = true;
        if s.parked.is_some() || s.warming || s.borrowed > 0 {
            return;
        }
        s.warming = true;
    }
    thread::spawn(move || {
        let warmed = warm_up(workspace, narrator);
        if let Err(error) = &warmed {
            eprintln!("PHOENIX_READER_WARM skipped: {error:#}");
        }
        let mut s = state();
        s.warming = false;
        let surplus = match warmed {
            Ok(provider) if s.wanted && s.parked.is_none() => {
                s.parked = Some(provider);
                schedule_release(&mut s);
                None
            }
            Ok(provider) => Some(provider),
            Err(_) => None,
        };
        WARMED.notify_all();
        drop(s);
        drop(surplus);
    });
}

/// Adopts the parked worker when it serves `bundle`; otherwise the caller
/// starts its own. Every call must be matched by `give_back`.
pub fn take(bundle: &Bundle) -> Option<NativeProvider> {
    let mut s = state();
    while s.warming {
        s = WARMED.wait(s).unwrap_or_else(|e| e.into_inner());
    }
    s.borrowed += 1;
    s.ticket += 1;
    let parked = s.parked.take()?;
    drop(s);
    if parked.serves(bundle) && parked.pid().is_some() {
        let mut provider = parked;
        provider.begin_session();
        Some(provider)
    } else {
        None
    }
}

/// Undoes a `take` whose caller could not start its own worker.
pub fn cancel_take() {
    let mut s = state();
    s.borrowed = s.borrowed.saturating_sub(1);
}

/// Returns a borrowed worker. A live one is parked while the Reader is open;
/// otherwise it is stopped.
pub fn give_back(provider: NativeProvider) {
    let mut s = state();
    s.borrowed = s.borrowed.saturating_sub(1);
    if s.wanted && s.parked.is_none() && provider.pid().is_some() {
        s.parked = Some(provider);
        schedule_release(&mut s);
    } else {
        drop(s);
        drop(provider);
    }
}

/// The Reader closed: stop the parked worker off the UI thread.
pub fn release() {
    let parked = {
        let mut s = state();
        s.wanted = false;
        s.ticket += 1;
        s.parked.take()
    };
    if let Some(provider) = parked {
        thread::spawn(move || drop(provider));
    }
}

/// App shutdown: stop the parked worker before the process exits, since a
/// static is never dropped.
pub fn release_now() {
    let parked = {
        let mut s = state();
        s.wanted = false;
        s.ticket += 1;
        s.parked.take()
    };
    drop(parked);
}

fn schedule_release(s: &mut State) {
    s.ticket += 1;
    let ticket = s.ticket;
    thread::spawn(move || {
        thread::sleep(IDLE_RELEASE);
        let parked = {
            let mut s = state();
            if s.ticket != ticket {
                return;
            }
            s.parked.take()
        };
        drop(parked);
    });
}

fn warm_up(workspace: PathBuf, narrator: Option<VoiceChoice>) -> anyhow::Result<NativeProvider> {
    let mut config = Config::read(&workspace)?;
    anyhow::ensure!(
        !config.worker.as_os_str().is_empty() && !config.model.as_os_str().is_empty(),
        "no Breeze runtime configured"
    );
    config.load_voices()?;
    let narrator = narrator.or_else(|| config.cast.as_ref().map(|cast| cast.narrator));
    let spec = match narrator {
        Some(choice) => config
            .voices
            .iter()
            .find(|voice| VoiceChoice::of(&voice.profile).ok() == Some(choice)),
        None => config.voices.first(),
    };
    let spec = spec
        .filter(|voice| voice.supertonic_style.is_none())
        .ok_or_else(|| anyhow::anyhow!("the narrator runs on the CPU"))?;
    phoenix_tts_native::use_digest_memo(config.storage.join("pinned-digests.memo"));
    let cancel = Cancellation::default();
    let bundle = if spec.qwen {
        config
            .qwen
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no Qwen runtime configured"))?
            .open(&cancel)?
    } else {
        Bundle::open_cancellable(&config.worker, &config.model, &config.dll_directory, &cancel)?
    };
    // Qwen only clones: warm it with the narrator's own reference.
    let voice = match (&spec.profile.reference, &spec.asset) {
        (Some(reference), Some(path)) if spec.qwen => Some(VoiceAsset::open(
            path,
            reference.encoded,
            reference.model,
            reference.codec,
        )?),
        _ => None,
    };
    let mut provider = NativeProvider::new(bundle, STARTUP_TIMEOUT, REQUEST_TIMEOUT)?;
    // A fresh seed misses the cache, so the worker really runs every stage.
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(1, |t| t.subsec_nanos() | 1);
    let mut cache = AudioCache::open(config.storage.join("warmup-cache"), 16 * 1024 * 1024)?;
    provider.generate_voiced_streamed(
        Request {
            epoch: 1,
            plan: *blake3::hash(b"phoenix.reader-warmup/v1").as_bytes(),
            segment: 0,
            text: "Ready.",
            instruction: "",
            seed,
            max_frames: 1920 * 125,
        },
        voice.as_ref(),
        &mut cache,
        &cancel,
        |_| Ok(()),
    )?;
    Ok(provider)
}
