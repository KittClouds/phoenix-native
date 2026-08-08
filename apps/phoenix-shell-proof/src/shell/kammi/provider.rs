use super::session::KammiRole;
use super::settings::{load_openrouter_key, validate_model_id};
use anyhow::{anyhow, Context, Result};
use futures_util::StreamExt;
use openrouter_rs::{
    api::chat::{ChatCompletionRequest, Message},
    types::Role,
    OpenRouterClient,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const COMMAND_CAPACITY: usize = 16;
const EVENT_CAPACITY: usize = 256;
const MAX_REQUEST_MESSAGES: usize = 256;
const MAX_REQUEST_BYTES: usize = 1024 * 1024;
const MAX_OUTPUT_TOKENS: u32 = 4_096;

#[derive(Clone, Debug)]
pub enum ProviderCommand {
    Generate(ProviderRequest),
    Cancel { request_id: u64 },
}

#[derive(Clone, Debug)]
pub struct ProviderRequest {
    pub request_id: u64,
    pub model: String,
    pub messages: Vec<ProviderMessage>,
}

#[derive(Clone, Debug)]
pub struct ProviderMessage {
    pub role: KammiRole,
    pub content: String,
}

#[derive(Clone, Debug)]
pub enum ProviderEvent {
    Started { request_id: u64 },
    Delta { request_id: u64, text: String },
    Finished { request_id: u64 },
    Cancelled { request_id: u64 },
    Failed { request_id: u64, error: String },
    Fatal { error: String },
}

pub struct KammiProviderRuntime {
    commands: async_channel::Sender<ProviderCommand>,
    events: async_channel::Receiver<ProviderEvent>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl KammiProviderRuntime {
    pub fn try_generate(&self, request: ProviderRequest) -> Result<()> {
        self.try_command(ProviderCommand::Generate(request), "generation")
    }

    pub fn events(&self) -> &async_channel::Receiver<ProviderEvent> {
        &self.events
    }

    pub fn try_cancel(&self, request_id: u64) -> Result<()> {
        self.try_command(ProviderCommand::Cancel { request_id }, "cancellation")
    }

    fn try_command(&self, command: ProviderCommand, label: &'static str) -> Result<()> {
        self.commands
            .try_send(command)
            .map_err(|error| match error {
                async_channel::TrySendError::Full(_) => {
                    anyhow!("OpenRouter {label} queue is busy")
                }
                async_channel::TrySendError::Closed(_) => {
                    anyhow!("OpenRouter provider is unavailable")
                }
            })
    }
}

impl Drop for KammiProviderRuntime {
    fn drop(&mut self) {
        // Closing wakes `recv` and any producer blocked on the bounded event
        // channel. Never block the GPUI thread trying to enqueue shutdown.
        self.commands.close();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub fn spawn_provider_runtime() -> Result<KammiProviderRuntime> {
    let (commands_tx, commands_rx) = async_channel::bounded(COMMAND_CAPACITY);
    let (events_tx, events_rx) = async_channel::bounded(EVENT_CAPACITY);
    let thread_events = events_tx.clone();

    let thread = std::thread::Builder::new()
        .name("phoenix-kammi-openrouter".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = thread_events.try_send(ProviderEvent::Fatal {
                        error: format!("failed to create Kammi runtime: {error}"),
                    });
                    return;
                }
            };
            runtime.block_on(provider_loop(commands_rx, thread_events));
        })
        .context("spawn Kammi provider thread")?;

    Ok(KammiProviderRuntime {
        commands: commands_tx,
        events: events_rx,
        thread: Some(thread),
    })
}

async fn provider_loop(
    commands: async_channel::Receiver<ProviderCommand>,
    events: async_channel::Sender<ProviderEvent>,
) {
    let mut active_task: Option<(u64, tokio::task::JoinHandle<()>, Arc<AtomicBool>)> = None;

    while let Ok(cmd) = commands.recv().await {
        match cmd {
            ProviderCommand::Generate(request) => {
                let req_id = request.request_id;
                if let Some((old_id, handle, cancel_flag)) = active_task.take() {
                    cancel_flag.store(true, Ordering::SeqCst);
                    handle.abort();
                    let _ = events.try_send(ProviderEvent::Cancelled { request_id: old_id });
                }

                let cancel_flag = Arc::new(AtomicBool::new(false));
                let cancel_flag_task = cancel_flag.clone();
                let events_task = events.clone();

                let handle = tokio::spawn(async move {
                    if let Err(err) =
                        run_generation(request, events_task.clone(), cancel_flag_task).await
                    {
                        let err_msg = err.to_string();
                        let _ = events_task
                            .send(ProviderEvent::Failed {
                                request_id: req_id,
                                error: err_msg,
                            })
                            .await;
                    }
                });

                active_task = Some((req_id, handle, cancel_flag));
            }
            ProviderCommand::Cancel { request_id } => {
                if let Some((active_id, handle, cancel_flag)) = active_task.take() {
                    if active_id == request_id {
                        cancel_flag.store(true, Ordering::SeqCst);
                        handle.abort();
                        let _ = events.try_send(ProviderEvent::Cancelled { request_id });
                    } else {
                        active_task = Some((active_id, handle, cancel_flag));
                    }
                }
            }
        }
    }

    if let Some((_id, handle, cancel_flag)) = active_task.take() {
        cancel_flag.store(true, Ordering::SeqCst);
        handle.abort();
    }
}

async fn run_generation(
    request: ProviderRequest,
    events: async_channel::Sender<ProviderEvent>,
    cancel_flag: Arc<AtomicBool>,
) -> Result<()> {
    let model = validate_model_id(&request.model)?;
    anyhow::ensure!(
        request.messages.len() <= MAX_REQUEST_MESSAGES,
        "OpenRouter request exceeds {MAX_REQUEST_MESSAGES} messages"
    );
    let request_bytes = request.messages.iter().try_fold(0usize, |total, message| {
        total.checked_add(message.content.len())
    });
    anyhow::ensure!(
        request_bytes.is_some_and(|bytes| bytes <= MAX_REQUEST_BYTES),
        "OpenRouter request exceeds {MAX_REQUEST_BYTES} UTF-8 bytes"
    );

    let api_key = load_openrouter_key()?.context("OpenRouter API key is not configured")?;

    let client = OpenRouterClient::builder()
        .api_key(api_key)
        .x_title("Phoenix Native")
        .app_categories(["writing"])
        .build()
        .context("build OpenRouter client")?;

    let messages = request.messages.iter().map(to_openrouter_message).collect();

    let completion = ChatCompletionRequest::builder()
        .model(model.as_str())
        .messages(messages)
        .max_tokens(MAX_OUTPUT_TOKENS)
        .build()
        .context("build OpenRouter chat request")?;

    events
        .send(ProviderEvent::Started {
            request_id: request.request_id,
        })
        .await?;

    let mut stream = client
        .chat()
        .stream(&completion)
        .await
        .context("start OpenRouter stream")?;

    while let Some(chunk) = stream.next().await {
        if cancel_flag.load(Ordering::SeqCst) {
            events
                .send(ProviderEvent::Cancelled {
                    request_id: request.request_id,
                })
                .await?;
            return Ok(());
        }

        let chunk = match chunk {
            Ok(c) => c,
            Err(e) => {
                let msg = e.to_string();
                let friendly_err = if msg.contains("401") || msg.contains("Unauthorized") {
                    "OpenRouter rejected the API key (401 Unauthorized).".to_string()
                } else if msg.contains("404") || msg.contains("Not Found") {
                    format!("Model '{}' was not found or is unavailable.", request.model)
                } else if msg.contains("429") || msg.contains("Too Many Requests") {
                    "OpenRouter rate limit reached (429 Too Many Requests).".to_string()
                } else {
                    format!("OpenRouter stream failed: {msg}")
                };
                anyhow::bail!("{friendly_err}");
            }
        };

        for choice in &chunk.choices {
            let Some(content) = choice.content() else {
                continue;
            };
            if content.is_empty() {
                continue;
            }
            events
                .send(ProviderEvent::Delta {
                    request_id: request.request_id,
                    text: content.to_owned(),
                })
                .await?;
        }
    }

    events
        .send(ProviderEvent::Finished {
            request_id: request.request_id,
        })
        .await?;

    Ok(())
}

fn to_openrouter_message(message: &ProviderMessage) -> Message {
    let role = match message.role {
        KammiRole::System => Role::System,
        KammiRole::User => Role::User,
        KammiRole::Assistant => Role::Assistant,
    };
    Message::new(role, message.content.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(messages: Vec<ProviderMessage>) -> ProviderRequest {
        ProviderRequest {
            request_id: 1,
            model: "openai/gpt-4o".to_string(),
            messages,
        }
    }

    fn detached_runtime(
        command_capacity: usize,
    ) -> (
        KammiProviderRuntime,
        async_channel::Receiver<ProviderCommand>,
    ) {
        let (commands, command_receiver) = async_channel::bounded(command_capacity);
        let (_events, receiver) = async_channel::bounded(1);
        (
            KammiProviderRuntime {
                commands,
                events: receiver,
                thread: None,
            },
            command_receiver,
        )
    }

    #[test]
    fn ui_dispatch_fails_fast_when_command_queue_is_full() {
        let (runtime, _receiver) = detached_runtime(1);
        runtime
            .commands
            .try_send(ProviderCommand::Cancel { request_id: 9 })
            .expect("fill command queue");

        let error = runtime
            .try_generate(request(Vec::new()))
            .expect_err("full queue must reject without blocking");

        assert!(error.to_string().contains("queue is busy"));
    }

    #[test]
    fn dropping_a_runtime_with_a_full_command_queue_does_not_block() {
        let (runtime, _receiver) = detached_runtime(1);
        runtime
            .commands
            .try_send(ProviderCommand::Cancel { request_id: 9 })
            .expect("fill command queue");
        drop(runtime);
    }

    #[test]
    fn oversized_request_is_rejected_before_credential_or_network_access() {
        let messages = (0..=MAX_REQUEST_MESSAGES)
            .map(|_| ProviderMessage {
                role: KammiRole::User,
                content: String::new(),
            })
            .collect();
        let (events, _receiver) = async_channel::bounded(1);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("test runtime");

        let error = runtime
            .block_on(run_generation(
                request(messages),
                events,
                Arc::new(AtomicBool::new(false)),
            ))
            .expect_err("oversized request must fail");

        assert!(error.to_string().contains("exceeds 256 messages"));
    }
}
