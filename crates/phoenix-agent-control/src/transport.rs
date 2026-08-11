use crate::{
    parse_command, AgentControlRequestV1, AgentControlResponseV1, AgentControlStatusV1,
    HostRequest, AGENT_CONTROL_SCHEMA_V1, MAX_INLINE_RESPONSE_BYTES, MAX_REQUEST_BYTES,
};
use anyhow::{anyhow, Context, Result};
use async_channel::{Receiver, Sender};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::time::timeout;

const HOST_TIMEOUT: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const DESCRIPTOR_SUFFIX: &str = ".agent-control-v1.json";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EndpointDescriptorV1 {
    pub schema: String,
    pub pipe_name: String,
    pub process_id: u32,
    pub instance_id: String,
}

pub struct AgentControlRuntime {
    shutdown: Sender<()>,
    thread: Option<JoinHandle<()>>,
    descriptor_path: PathBuf,
}

impl Drop for AgentControlRuntime {
    fn drop(&mut self) {
        let _ = self.shutdown.try_send(());
        self.shutdown.close();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = fs::remove_file(&self.descriptor_path);
    }
}

pub fn endpoint_descriptor_path(workspace_path: &Path) -> PathBuf {
    let mut path = workspace_path.as_os_str().to_owned();
    path.push(DESCRIPTOR_SUFFIX);
    PathBuf::from(path)
}

pub fn spawn_control_runtime(
    workspace_path: &Path,
    host: Sender<HostRequest>,
) -> Result<AgentControlRuntime> {
    let descriptor_path = endpoint_descriptor_path(workspace_path);
    let instance_id = uuid::Uuid::new_v4().simple().to_string();
    let workspace_key = blake3::hash(workspace_path.as_os_str().to_string_lossy().as_bytes());
    let pipe_name = format!(
        r"\\.\pipe\phoenix-native-agent-{}-{}",
        &workspace_key.to_hex()[..16],
        instance_id
    );
    let descriptor = EndpointDescriptorV1 {
        schema: AGENT_CONTROL_SCHEMA_V1.to_string(),
        pipe_name,
        process_id: std::process::id(),
        instance_id,
    };
    let (shutdown_tx, shutdown_rx) = async_channel::bounded(1);
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
    let thread_descriptor_path = descriptor_path.clone();
    let thread = std::thread::Builder::new()
        .name("phoenix-agent-control".into())
        .spawn(move || {
            let result = run_server_thread(
                descriptor,
                thread_descriptor_path,
                host,
                shutdown_rx,
                ready_tx,
            );
            if let Err(error) = result {
                eprintln!("PHOENIX_AGENT_CONTROL_FAILED {error:#}");
            }
        })
        .context("spawn Phoenix agent control thread")?;
    ready_rx
        .recv_timeout(Duration::from_secs(5))
        .context("Phoenix agent control startup timed out")?
        .map_err(|error| anyhow!(error))?;
    Ok(AgentControlRuntime {
        shutdown: shutdown_tx,
        thread: Some(thread),
        descriptor_path,
    })
}

fn run_server_thread(
    descriptor: EndpointDescriptorV1,
    descriptor_path: PathBuf,
    host: Sender<HostRequest>,
    shutdown: Receiver<()>,
    ready: std::sync::mpsc::SyncSender<Result<(), String>>,
) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("build Phoenix agent control runtime")?;
    runtime.block_on(async move {
        let first = create_server(&descriptor.pipe_name, true)?;
        publish_descriptor(&descriptor_path, &descriptor)?;
        let _ = ready.send(Ok(()));
        server_loop(first, &descriptor.pipe_name, host, shutdown).await
    })
}

#[cfg(windows)]
fn create_server(
    pipe_name: &str,
    first: bool,
) -> Result<tokio::net::windows::named_pipe::NamedPipeServer> {
    tokio::net::windows::named_pipe::ServerOptions::new()
        .first_pipe_instance(first)
        .reject_remote_clients(true)
        .create(pipe_name)
        .with_context(|| format!("create local named pipe {pipe_name}"))
}

#[cfg(not(windows))]
fn create_server(_pipe_name: &str, _first: bool) -> Result<()> {
    Err(anyhow!(
        "Phoenix agent control requires Windows named pipes"
    ))
}

#[cfg(windows)]
async fn server_loop(
    mut server: tokio::net::windows::named_pipe::NamedPipeServer,
    pipe_name: &str,
    host: Sender<HostRequest>,
    shutdown: Receiver<()>,
) -> Result<()> {
    loop {
        tokio::select! {
            _ = shutdown.recv() => return Ok(()),
            connected = server.connect() => connected.context("accept Phoenix agent client")?,
        }
        handle_connection(server, &host).await?;
        server = create_server(pipe_name, false)?;
    }
}

#[cfg(not(windows))]
async fn server_loop(
    _server: (),
    _pipe_name: &str,
    _host: Sender<HostRequest>,
    _shutdown: Receiver<()>,
) -> Result<()> {
    unreachable!()
}

#[cfg(windows)]
async fn handle_connection(
    server: tokio::net::windows::named_pipe::NamedPipeServer,
    host: &Sender<HostRequest>,
) -> Result<()> {
    let mut reader = BufReader::new(server);
    let mut line = String::with_capacity(4096);
    let bytes = timeout(CONNECT_TIMEOUT, reader.read_line(&mut line))
        .await
        .context("agent request read timed out")??;
    if bytes == 0 || bytes > MAX_REQUEST_BYTES {
        return Ok(());
    }
    let mut response = match serde_json::from_str::<AgentControlRequestV1>(&line) {
        Ok(request) if request.schema == AGENT_CONTROL_SCHEMA_V1 => {
            match parse_command(&request.command) {
                Ok(command) => {
                    let canonical = command.canonical();
                    let (reply_tx, reply_rx) = async_channel::bounded(1);
                    let host_request = HostRequest {
                        request: request.clone(),
                        command,
                        reply: reply_tx,
                    };
                    match host.try_send(host_request) {
                        Ok(()) => match timeout(HOST_TIMEOUT, reply_rx.recv()).await {
                            Ok(Ok(response)) => response,
                            Ok(Err(_)) => {
                                AgentControlResponseV1::error(&request, "agent host unavailable")
                            }
                            Err(_) => {
                                AgentControlResponseV1::error(&request, "agent host timed out")
                            }
                        },
                        Err(_) => AgentControlResponseV1 {
                            canonical_command: canonical,
                            status: AgentControlStatusV1::Busy,
                            error: Some("agent command queue is busy".into()),
                            ..AgentControlResponseV1::error(&request, "agent command queue is busy")
                        },
                    }
                }
                Err(error) => AgentControlResponseV1::error(&request, error.to_string()),
            }
        }
        Ok(request) => AgentControlResponseV1::error(&request, "unsupported agent control schema"),
        Err(error) => AgentControlResponseV1::error(
            &AgentControlRequestV1::new(""),
            format!("invalid agent control request: {error}"),
        ),
    };
    let mut encoded = serde_json::to_vec(&response)?;
    if encoded.len() > MAX_INLINE_RESPONSE_BYTES {
        response.status = AgentControlStatusV1::Error;
        response.payload = serde_json::Value::Null;
        response.error = Some("agent response exceeded the inline budget".into());
        encoded = serde_json::to_vec(&response)?;
    }
    encoded.push(b'\n');
    let stream = reader.get_mut();
    stream.write_all(&encoded).await?;
    stream.flush().await?;
    Ok(())
}

pub fn call_running_app(
    workspace_path: &Path,
    request: &AgentControlRequestV1,
) -> Result<AgentControlResponseV1> {
    let descriptor_path = endpoint_descriptor_path(workspace_path);
    let descriptor: EndpointDescriptorV1 = serde_json::from_slice(
        &fs::read(&descriptor_path)
            .with_context(|| format!("read agent endpoint at {}", descriptor_path.display()))?,
    )?;
    if descriptor.schema != AGENT_CONTROL_SCHEMA_V1 {
        return Err(anyhow!("agent endpoint schema is incompatible"));
    }
    if !process_is_alive(descriptor.process_id) {
        let _ = fs::remove_file(&descriptor_path);
        return Err(anyhow!(
            "agent endpoint is stale; Phoenix process {} is not running",
            descriptor.process_id
        ));
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(call_pipe(&descriptor.pipe_name, request))
}

#[cfg(windows)]
fn process_is_alive(process_id: u32) -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    const STILL_ACTIVE_EXIT_CODE: u32 = 259;

    let Ok(process) =
        (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) })
    else {
        return false;
    };
    let mut exit_code = 0;
    let live = unsafe { GetExitCodeProcess(process, &mut exit_code) }.is_ok()
        && exit_code == STILL_ACTIVE_EXIT_CODE;
    let _ = unsafe { CloseHandle(process) };
    live
}

#[cfg(not(windows))]
fn process_is_alive(_process_id: u32) -> bool {
    false
}

#[cfg(windows)]
async fn call_pipe(
    pipe_name: &str,
    request: &AgentControlRequestV1,
) -> Result<AgentControlResponseV1> {
    use tokio::net::windows::named_pipe::ClientOptions;
    let deadline = tokio::time::Instant::now() + CONNECT_TIMEOUT;
    let client = loop {
        match ClientOptions::new().open(pipe_name) {
            Ok(client) => break client,
            Err(error) if tokio::time::Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(10)).await;
                let _ = error;
            }
            Err(error) => return Err(error).context("connect to running Phoenix app"),
        }
    };
    let mut encoded = serde_json::to_vec(request)?;
    if encoded.len() > MAX_REQUEST_BYTES {
        return Err(anyhow!("agent request exceeds {MAX_REQUEST_BYTES} bytes"));
    }
    encoded.push(b'\n');
    let mut reader = BufReader::new(client);
    reader.get_mut().write_all(&encoded).await?;
    reader.get_mut().flush().await?;
    let mut line = String::with_capacity(4096);
    timeout(HOST_TIMEOUT, reader.read_line(&mut line))
        .await
        .context("agent response timed out")??;
    if line.len() > MAX_INLINE_RESPONSE_BYTES {
        return Err(anyhow!("agent response exceeded inline budget"));
    }
    Ok(serde_json::from_str(&line)?)
}

#[cfg(not(windows))]
async fn call_pipe(
    _pipe_name: &str,
    _request: &AgentControlRequestV1,
) -> Result<AgentControlResponseV1> {
    Err(anyhow!(
        "Phoenix agent control requires Windows named pipes"
    ))
}

fn publish_descriptor(path: &Path, descriptor: &EndpointDescriptorV1) -> Result<()> {
    let parent = path
        .parent()
        .context("agent descriptor path has no parent")?;
    fs::create_dir_all(parent)?;
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(format!(".{}.tmp", std::process::id()));
    let temporary = PathBuf::from(temporary);
    fs::write(&temporary, serde_json::to_vec(descriptor)?)?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(&temporary, path)?;
    Ok(())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn local_named_pipe_round_trip_reaches_typed_host() {
        let directory = tempfile::tempdir().expect("temp directory");
        let workspace = directory.path().join("workspace.json");
        let (host_tx, host_rx) = async_channel::bounded(2);
        let runtime = spawn_control_runtime(&workspace, host_tx).expect("control runtime");
        let host = std::thread::spawn(move || {
            let request = host_rx.recv_blocking().expect("host request");
            let response = AgentControlResponseV1 {
                schema: AGENT_CONTROL_SCHEMA_V1.to_string(),
                command_id: request.request.command_id,
                canonical_command: request.command.canonical(),
                status: AgentControlStatusV1::Ok,
                sequence: 7,
                kernel_revision: 3,
                workspace_revision: 2,
                document_revision: Some(1),
                replayed: false,
                payload: json!({ "round_trip": true }),
                error: None,
            };
            request.reply.send_blocking(response).expect("host reply");
        });
        let response = call_running_app(&workspace, &AgentControlRequestV1::new("phx app status"))
            .expect("client response");
        assert_eq!(response.status, AgentControlStatusV1::Ok);
        assert_eq!(response.payload, json!({ "round_trip": true }));
        host.join().expect("host thread");
        drop(runtime);
        assert!(!endpoint_descriptor_path(&workspace).exists());
    }

    #[test]
    fn stale_descriptor_is_rejected_and_removed() {
        let directory = tempfile::tempdir().expect("temp directory");
        let workspace = directory.path().join("workspace.json");
        let descriptor_path = endpoint_descriptor_path(&workspace);
        let descriptor = EndpointDescriptorV1 {
            schema: AGENT_CONTROL_SCHEMA_V1.to_string(),
            pipe_name: r"\\.\pipe\phoenix-native-agent-dead-test".into(),
            process_id: u32::MAX,
            instance_id: "dead-test".into(),
        };
        fs::write(
            &descriptor_path,
            serde_json::to_vec(&descriptor).expect("encode descriptor"),
        )
        .expect("write descriptor");
        let error = call_running_app(&workspace, &AgentControlRequestV1::new("phx app status"))
            .expect_err("stale endpoint must fail closed");
        assert!(error.to_string().contains("stale"));
        assert!(!descriptor_path.exists());
    }
}
