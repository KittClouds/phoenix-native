use crate::{
    CancellationProbe, ContextPacket, CoordinatorError, CoordinatorState, DualFaceProducer,
    GenerationPublication, IngestDocumentRevision, IngestTurn, ModelIdentityInputV3,
    ProducerProductV3, ProducerRegistrationV3, RecallTurn,
};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

pub const DEFAULT_COMMAND_CAPACITY: usize = 4;
pub const MAX_COMMAND_CAPACITY: usize = 64;
pub const DEFAULT_CONTEXT_ITEMS: usize = 24;
pub const DEFAULT_CONTEXT_BYTES: usize = 32 * 1024;
pub const MAX_CONTEXT_ITEMS: usize = 128;
pub const MAX_CONTEXT_BYTES: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LexicalRecallConfig {
    pub top_k: usize,
    pub maximum_items: usize,
    pub maximum_candidate_pool: usize,
}

impl Default for LexicalRecallConfig {
    fn default() -> Self {
        Self {
            top_k: DEFAULT_CONTEXT_ITEMS,
            maximum_items: 2_000_000,
            maximum_candidate_pool: 256,
        }
    }
}

#[derive(Clone, Debug)]
pub struct CoordinatorConfig {
    pub namespace_external_identity: Arc<[u8]>,
    pub artifact_dir: PathBuf,
    pub registry_revision: u64,
    pub queue_capacity: usize,
    pub max_context_items: usize,
    pub max_context_bytes: usize,
    pub registrations: Arc<[ProducerRegistrationV3]>,
    pub model_identities: Arc<[ModelIdentityInputV3]>,
    /// Required production lexical recall. Failure rejects the mutation rather
    /// than silently falling back to a full scan.
    pub lexical_recall: LexicalRecallConfig,
    /// Explicitly non-authoritative lexical evaluation. Disabled by default.
    pub qps_shadow: crate::QpsShadowConfig,
}

impl CoordinatorConfig {
    pub fn new(
        namespace_external_identity: impl Into<Arc<[u8]>>,
        artifact_dir: impl Into<PathBuf>,
        registrations: impl Into<Arc<[ProducerRegistrationV3]>>,
        model_identities: impl Into<Arc<[ModelIdentityInputV3]>>,
    ) -> Self {
        Self {
            namespace_external_identity: namespace_external_identity.into(),
            artifact_dir: artifact_dir.into(),
            registry_revision: 0,
            queue_capacity: DEFAULT_COMMAND_CAPACITY,
            max_context_items: DEFAULT_CONTEXT_ITEMS,
            max_context_bytes: DEFAULT_CONTEXT_BYTES,
            registrations: registrations.into(),
            model_identities: model_identities.into(),
            lexical_recall: LexicalRecallConfig::default(),
            qps_shadow: crate::QpsShadowConfig::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CoordinatorMetrics {
    pub queue_capacity: u64,
    pub queue_high_water: u64,
    pub cancellation_epoch: u64,
    pub submitted: u64,
    pub completed: u64,
}

struct RuntimeMetrics {
    queue_depth: AtomicU64,
    queue_high_water: AtomicU64,
    cancellation_epoch: Arc<AtomicU64>,
    submitted: AtomicU64,
    completed: AtomicU64,
    shutting_down: AtomicBool,
}

impl RuntimeMetrics {
    fn snapshot(&self, capacity: usize) -> CoordinatorMetrics {
        CoordinatorMetrics {
            queue_capacity: capacity as u64,
            queue_high_water: self.queue_high_water.load(Ordering::Acquire),
            cancellation_epoch: self.cancellation_epoch.load(Ordering::Acquire),
            submitted: self.submitted.load(Ordering::Acquire),
            completed: self.completed.load(Ordering::Acquire),
        }
    }
}

enum Work {
    Document {
        request: IngestDocumentRevision,
        epoch: u64,
        reply: SyncSender<Result<GenerationPublication, CoordinatorError>>,
    },
    Recall {
        request: RecallTurn,
        epoch: u64,
        reply: SyncSender<Result<ContextPacket, CoordinatorError>>,
    },
    Turn {
        request: IngestTurn,
        epoch: u64,
        reply: SyncSender<Result<GenerationPublication, CoordinatorError>>,
    },
    Verify {
        reply: SyncSender<Result<Option<[u8; 32]>, CoordinatorError>>,
    },
    Shutdown,
}

pub struct CoordinatorTicket<T> {
    receiver: Receiver<Result<T, CoordinatorError>>,
}

impl<T> CoordinatorTicket<T> {
    pub fn wait(self) -> Result<T, CoordinatorError> {
        self.receiver
            .recv()
            .map_err(|_| CoordinatorError::Shutdown)?
    }
}

pub struct DualFaceIngestionCoordinator<P> {
    sender: SyncSender<Work>,
    metrics: Arc<RuntimeMetrics>,
    queue_capacity: usize,
    worker: Option<JoinHandle<()>>,
    _producer: std::marker::PhantomData<P>,
}

impl<P: DualFaceProducer> DualFaceIngestionCoordinator<P> {
    pub fn new(config: CoordinatorConfig, producer: Arc<P>) -> Result<Self, CoordinatorError> {
        let queue_capacity = config.queue_capacity;
        let state = CoordinatorState::new(config, producer)?;
        let (sender, receiver) = mpsc::sync_channel(queue_capacity);
        let metrics = Arc::new(RuntimeMetrics {
            queue_depth: AtomicU64::new(0),
            queue_high_water: AtomicU64::new(0),
            cancellation_epoch: Arc::new(AtomicU64::new(0)),
            submitted: AtomicU64::new(0),
            completed: AtomicU64::new(0),
            shutting_down: AtomicBool::new(false),
        });
        let worker_metrics = metrics.clone();
        let worker = thread::Builder::new()
            .name("phoenix-memory-ingest".to_owned())
            .spawn(move || run_worker(state, receiver, worker_metrics))
            .map_err(|source| CoordinatorError::io("phoenix-memory-ingest", source))?;
        Ok(Self {
            sender,
            metrics,
            queue_capacity,
            worker: Some(worker),
            _producer: std::marker::PhantomData,
        })
    }

    pub fn try_ingest_document(
        &self,
        request: IngestDocumentRevision,
    ) -> Result<CoordinatorTicket<GenerationPublication>, CoordinatorError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        let epoch = self.metrics.cancellation_epoch.load(Ordering::Acquire);
        self.submit(Work::Document {
            request,
            epoch,
            reply,
        })?;
        Ok(CoordinatorTicket { receiver })
    }

    pub fn try_recall(
        &self,
        request: RecallTurn,
    ) -> Result<CoordinatorTicket<ContextPacket>, CoordinatorError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        let epoch = self.metrics.cancellation_epoch.load(Ordering::Acquire);
        self.submit(Work::Recall {
            request,
            epoch,
            reply,
        })?;
        Ok(CoordinatorTicket { receiver })
    }

    pub fn try_ingest_turn(
        &self,
        request: IngestTurn,
    ) -> Result<CoordinatorTicket<GenerationPublication>, CoordinatorError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        let epoch = self.metrics.cancellation_epoch.load(Ordering::Acquire);
        self.submit(Work::Turn {
            request,
            epoch,
            reply,
        })?;
        Ok(CoordinatorTicket { receiver })
    }

    pub fn try_verify_current(
        &self,
    ) -> Result<CoordinatorTicket<Option<[u8; 32]>>, CoordinatorError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.submit(Work::Verify { reply })?;
        Ok(CoordinatorTicket { receiver })
    }

    pub fn cancel(&self) -> u64 {
        self.metrics
            .cancellation_epoch
            .fetch_add(1, Ordering::AcqRel)
            .saturating_add(1)
    }

    pub fn metrics(&self) -> CoordinatorMetrics {
        self.metrics.snapshot(self.queue_capacity)
    }

    pub fn shutdown(mut self) -> Result<(), CoordinatorError> {
        self.stop()
    }

    fn submit(&self, work: Work) -> Result<(), CoordinatorError> {
        if self.metrics.shutting_down.load(Ordering::Acquire) {
            return Err(CoordinatorError::Shutdown);
        }
        let depth = self
            .metrics
            .queue_depth
            .fetch_add(1, Ordering::AcqRel)
            .saturating_add(1);
        match self.sender.try_send(work) {
            Ok(()) => {
                self.metrics
                    .queue_high_water
                    .fetch_max(depth, Ordering::AcqRel);
                self.metrics.submitted.fetch_add(1, Ordering::AcqRel);
                Ok(())
            }
            Err(TrySendError::Full(_)) => {
                self.metrics.queue_depth.fetch_sub(1, Ordering::AcqRel);
                Err(CoordinatorError::QueueFull)
            }
            Err(TrySendError::Disconnected(_)) => {
                self.metrics.queue_depth.fetch_sub(1, Ordering::AcqRel);
                Err(CoordinatorError::Shutdown)
            }
        }
    }

    fn stop(&mut self) -> Result<(), CoordinatorError> {
        if self.worker.is_none() {
            return Ok(());
        }
        self.metrics.shutting_down.store(true, Ordering::Release);
        self.metrics
            .cancellation_epoch
            .fetch_add(1, Ordering::AcqRel);
        self.sender
            .send(Work::Shutdown)
            .map_err(|_| CoordinatorError::Shutdown)?;
        let Some(worker) = self.worker.take() else {
            return Ok(());
        };
        worker.join().map_err(|_| CoordinatorError::Shutdown)
    }
}

impl<P> Drop for DualFaceIngestionCoordinator<P> {
    fn drop(&mut self) {
        if self.worker.is_some() {
            self.metrics.shutting_down.store(true, Ordering::Release);
            self.metrics
                .cancellation_epoch
                .fetch_add(1, Ordering::AcqRel);
            let _ = self.sender.send(Work::Shutdown);
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        }
    }
}

fn run_worker<P: DualFaceProducer>(
    mut state: CoordinatorState<P>,
    receiver: Receiver<Work>,
    metrics: Arc<RuntimeMetrics>,
) {
    while let Ok(work) = receiver.recv() {
        if matches!(work, Work::Shutdown) {
            break;
        }
        metrics.queue_depth.fetch_sub(1, Ordering::AcqRel);
        match work {
            Work::Document {
                request,
                epoch,
                reply,
            } => {
                let cancellation =
                    CancellationProbe::new(metrics.cancellation_epoch.clone(), epoch);
                let _ = reply.send(state.ingest_document(request, &cancellation));
            }
            Work::Recall {
                request,
                epoch,
                reply,
            } => {
                let result = if metrics.cancellation_epoch.load(Ordering::Acquire) == epoch {
                    state.recall(request)
                } else {
                    Err(CoordinatorError::Cancelled)
                };
                let _ = reply.send(result);
            }
            Work::Turn {
                request,
                epoch,
                reply,
            } => {
                let cancellation =
                    CancellationProbe::new(metrics.cancellation_epoch.clone(), epoch);
                let _ = reply.send(state.ingest_turn(request, &cancellation));
            }
            Work::Verify { reply } => {
                let _ = reply.send(state.verify_current());
            }
            Work::Shutdown => break,
        }
        metrics.completed.fetch_add(1, Ordering::AcqRel);
    }
}

pub(crate) fn validate_registrations(config: &CoordinatorConfig) -> Result<(), CoordinatorError> {
    if config.namespace_external_identity.is_empty()
        || config.queue_capacity == 0
        || config.queue_capacity > MAX_COMMAND_CAPACITY
        || config.max_context_items == 0
        || config.max_context_items > MAX_CONTEXT_ITEMS
        || config.max_context_bytes == 0
        || config.max_context_bytes > MAX_CONTEXT_BYTES
        || config.lexical_recall.top_k == 0
        || config.lexical_recall.top_k > config.max_context_items
        || config.lexical_recall.maximum_items == 0
        || config.lexical_recall.maximum_candidate_pool < config.lexical_recall.top_k
        || config.registrations.len() != ProducerProductV3::ALL.len()
    {
        return Err(CoordinatorError::InvalidCapabilityMatrix);
    }
    for (registration, expected) in config.registrations.iter().zip(ProducerProductV3::ALL) {
        if registration.product != expected
            || registration.producer.trim().is_empty()
            || registration.producer_binary_hash == [0; 32]
            || registration.config_hash == [0; 32]
            || registration
                .model_identity_index
                .is_some_and(|index| index as usize >= config.model_identities.len())
            || registration.model_identity_index.is_some_and(|index| {
                config.model_identities[index as usize].semantic_role
                    == phoenix_memory_contract::ModelSemanticRoleV3::DedicatedNliObserver
            })
            || (crate::authoritative_product(registration.product)
                && registration.support != crate::RegistrationSupport::Supported)
        {
            return Err(CoordinatorError::InvalidCapabilityMatrix);
        }
    }
    if config.model_identities.iter().any(|model| {
        model.name.trim().is_empty()
            || model.runtime.trim().is_empty()
            || model.artifact_hash == [0; 32]
            || model.config_hash == [0; 32]
    }) {
        return Err(CoordinatorError::InvalidCapabilityMatrix);
    }
    Ok(())
}
