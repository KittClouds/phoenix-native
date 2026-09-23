use super::super::settings::{LlamaCppSettings, LlamaPerformanceProfile};
use anyhow::{Context, Result};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const STARTUP_TIMEOUT: Duration = Duration::from_secs(180);
const STARTUP_POLL_INTERVAL: Duration = Duration::from_millis(125);

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ServerIdentity {
    executable: PathBuf,
    model: PathBuf,
    address: SocketAddr,
    context_size: u32,
    gpu_layers: u32,
    threads: u32,
    performance: LlamaPerformanceProfile,
}

pub(super) struct LlamaServerManager {
    active: Option<ManagedServer>,
}

struct ManagedServer {
    identity: ServerIdentity,
    child: Child,
    ready: bool,
}

impl LlamaServerManager {
    pub(super) fn new() -> Self {
        Self { active: None }
    }

    pub(super) async fn ensure_ready(
        &mut self,
        settings: &LlamaCppSettings,
        cancelled: &AtomicBool,
    ) -> Result<()> {
        let identity = validate_settings(settings)?;
        if self
            .active
            .as_ref()
            .is_some_and(|server| server.identity == identity)
        {
            if health_ready(identity.address).await {
                if let Some(server) = self.active.as_mut() {
                    server.ready = true;
                }
                return Ok(());
            }
            self.stop();
        } else if self.active.is_some() {
            self.stop();
        }

        anyhow::ensure!(
            TcpStream::connect(identity.address).await.is_err(),
            "llama.cpp port {} is already occupied by an unmanaged process",
            identity.address.port()
        );

        let mut command = Command::new(&identity.executable);
        command
            .arg("--model")
            .arg(&identity.model)
            .arg("--alias")
            .arg(settings.model_label())
            .arg("--host")
            .arg(identity.address.ip().to_string())
            .arg("--port")
            .arg(identity.address.port().to_string())
            .arg("--ctx-size")
            .arg(identity.context_size.to_string())
            .arg("--n-gpu-layers")
            .arg(identity.gpu_layers.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if identity.threads > 0 {
            command.arg("--threads").arg(identity.threads.to_string());
        }
        append_runtime_args(&mut command, &identity);
        hide_console_window(&mut command);

        let child = command.spawn().with_context(|| {
            format!(
                "start llama.cpp server at {} with model {}",
                identity.executable.display(),
                identity.model.display()
            )
        })?;
        self.active = Some(ManagedServer {
            identity: identity.clone(),
            child,
            ready: false,
        });

        let deadline = tokio::time::Instant::now() + STARTUP_TIMEOUT;
        loop {
            if cancelled.load(Ordering::Acquire) {
                anyhow::bail!("llama.cpp startup cancelled");
            }
            let server = self
                .active
                .as_mut()
                .context("llama.cpp server state lost")?;
            if let Some(status) = server.child.try_wait().context("poll llama.cpp server")? {
                self.active = None;
                anyhow::bail!("llama.cpp exited during startup with {status}");
            }
            if health_ready(identity.address).await {
                if let Some(server) = self.active.as_mut() {
                    server.ready = true;
                }
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                self.stop();
                anyhow::bail!(
                    "llama.cpp did not become ready on {} within {} seconds",
                    identity.address,
                    STARTUP_TIMEOUT.as_secs()
                );
            }
            tokio::time::sleep(STARTUP_POLL_INTERVAL).await;
        }
    }

    fn stop(&mut self) {
        if let Some(mut server) = self.active.take() {
            let _ = server.child.kill();
            let _ = server.child.wait();
        }
    }

    pub(super) fn stop_if_loading(&mut self) {
        if self.active.as_ref().is_some_and(|server| !server.ready) {
            self.stop();
        }
    }
}

fn append_runtime_args(command: &mut Command, identity: &ServerIdentity) {
    match identity.performance {
        LlamaPerformanceProfile::Auto => {}
        LlamaPerformanceProfile::SingleSlot => {
            command
                .arg("--parallel")
                .arg("1")
                .arg("--flash-attn")
                .arg("on");
        }
    }
}

async fn health_ready(address: SocketAddr) -> bool {
    tokio::time::timeout(Duration::from_secs(1), health_exchange(address))
        .await
        .unwrap_or(false)
}

async fn health_exchange(address: SocketAddr) -> bool {
    let Ok(mut stream) = TcpStream::connect(address).await else {
        return false;
    };
    let request = b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n";
    if stream.write_all(request).await.is_err() {
        return false;
    }
    let mut prefix = [0_u8; 64];
    let Ok(read) = stream.read(&mut prefix).await else {
        return false;
    };
    prefix[..read].starts_with(b"HTTP/1.1 200") || prefix[..read].starts_with(b"HTTP/1.0 200")
}

impl Drop for LlamaServerManager {
    fn drop(&mut self) {
        self.stop();
    }
}

pub(super) fn validate_settings(settings: &LlamaCppSettings) -> Result<ServerIdentity> {
    let executable = canonical_file(&settings.server_path, "llama-server executable")?;
    let model = canonical_file(&settings.model_path, "GGUF model")?;
    anyhow::ensure!(
        model
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("gguf")),
        "local model must have a .gguf extension"
    );
    let address = parse_loopback_endpoint(&settings.endpoint)?;
    anyhow::ensure!(
        (512..=1_048_576).contains(&settings.context_size),
        "llama.cpp context size must be between 512 and 1048576"
    );
    anyhow::ensure!(settings.gpu_layers <= 4_096, "GPU layer count is too large");
    anyhow::ensure!(settings.threads <= 1_024, "thread count is too large");

    Ok(ServerIdentity {
        executable,
        model,
        address,
        context_size: settings.context_size,
        gpu_layers: settings.gpu_layers,
        threads: settings.threads,
        performance: settings.performance,
    })
}

pub(super) fn parse_loopback_endpoint(endpoint: &str) -> Result<SocketAddr> {
    let endpoint = endpoint.trim().trim_end_matches('/');
    let authority = endpoint
        .strip_prefix("http://")
        .context("llama.cpp endpoint must use local http://")?
        .split('/')
        .next()
        .context("llama.cpp endpoint is missing a host")?;
    let (host, port) = authority
        .rsplit_once(':')
        .context("llama.cpp endpoint must include a port")?;
    anyhow::ensure!(
        matches!(host, "127.0.0.1" | "localhost"),
        "llama.cpp endpoint must use 127.0.0.1 or localhost"
    );
    let port = port
        .parse::<u16>()
        .context("invalid llama.cpp endpoint port")?;
    anyhow::ensure!(port != 0, "llama.cpp endpoint port cannot be zero");
    Ok(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port))
}

fn canonical_file(path: &str, label: &str) -> Result<PathBuf> {
    let path = Path::new(path.trim());
    anyhow::ensure!(!path.as_os_str().is_empty(), "{label} path is required");
    anyhow::ensure!(path.is_absolute(), "{label} path must be absolute");
    let canonical = path
        .canonicalize()
        .with_context(|| format!("resolve {label} at {}", path.display()))?;
    anyhow::ensure!(canonical.is_file(), "{label} is not a file");
    Ok(canonical)
}

#[cfg(windows)]
fn hide_console_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console_window(_command: &mut Command) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_is_fail_closed_to_loopback_http() {
        assert_eq!(
            parse_loopback_endpoint("http://127.0.0.1:8080/v1").unwrap(),
            "127.0.0.1:8080".parse().unwrap()
        );
        assert!(parse_loopback_endpoint("https://127.0.0.1:8080/v1").is_err());
        assert!(parse_loopback_endpoint("http://192.168.1.4:8080/v1").is_err());
        assert!(parse_loopback_endpoint("http://localhost/v1").is_err());
    }

    #[test]
    fn missing_files_are_rejected_before_process_launch() {
        let settings = LlamaCppSettings {
            server_path: r"C:\does-not-exist\llama-server.exe".into(),
            model_path: r"D:\does-not-exist\model.gguf".into(),
            ..LlamaCppSettings::default()
        };
        let error = validate_settings(&settings).unwrap_err().to_string();
        assert!(error.contains("resolve llama-server executable"));
    }

    #[test]
    fn single_slot_profile_adds_only_measured_runtime_flags() {
        let identity = ServerIdentity {
            executable: PathBuf::from(r"C:\llama-server.exe"),
            model: PathBuf::from(r"D:\model.gguf"),
            address: "127.0.0.1:8080".parse().unwrap(),
            context_size: 8_192,
            gpu_layers: 99,
            threads: 0,
            performance: LlamaPerformanceProfile::SingleSlot,
        };
        let mut command = Command::new("llama-server");
        append_runtime_args(&mut command, &identity);
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(args, ["--parallel", "1", "--flash-attn", "on"]);
    }
}
