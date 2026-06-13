use anyhow::{anyhow, bail, Context, Result};
use std::fs;
use std::hash::Hasher;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::time::{timeout, Duration};

const SOCKET_DIR_NAME: &str = "temad";
const SOCKET_PREFIX: &str = "control-";
const SOCKET_SUFFIX: &str = ".sock";
const CONTROL_READ_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ControlCommand {
    Reload,
}

pub(crate) fn control_socket_path_for(runtime_dir: &Path, config_path: &Path) -> PathBuf {
    let id = stable_config_id(config_path);
    runtime_dir
        .join(SOCKET_DIR_NAME)
        .join(format!("{SOCKET_PREFIX}{id:016x}{SOCKET_SUFFIX}"))
}

pub(crate) fn resolve_control_socket_path(config_path: &Path) -> Result<PathBuf> {
    let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("XDG_RUNTIME_DIR is not set; cannot locate temad control socket"))?;
    Ok(control_socket_path_for(&runtime_dir, config_path))
}

pub(crate) async fn request_reload(config_path: &Path) -> Result<String> {
    let socket_path = resolve_control_socket_path(config_path)?;
    request_command(&socket_path, "reload").await
}

async fn request_command(socket_path: &Path, command: &str) -> Result<String> {
    let mut stream = match UnixStream::connect(&socket_path).await {
        Ok(stream) => stream,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            bail!(
                "No reload-capable temad daemon is listening at {}. If you recently upgraded temad or use a Hyprland exec-once daemon, restart `temad daemon` and try again.",
                socket_path.display()
            );
        }
        Err(error) => {
            return Err(error).with_context(|| {
                format!(
                    "Failed to connect to running temad daemon at {}",
                    socket_path.display()
                )
            });
        }
    };

    let request = format!("{command}\n");
    stream
        .write_all(request.as_bytes())
        .await
        .with_context(|| {
            format!(
                "Failed to send control request to temad daemon at {}",
                socket_path.display()
            )
        })?;
    stream.flush().await.with_context(|| {
        format!(
            "Failed to flush control request to temad daemon at {}",
            socket_path.display()
        )
    })?;

    let mut response = String::new();
    let mut reader = BufReader::new(stream);
    let read = reader.read_line(&mut response).await.with_context(|| {
        format!(
            "Failed to read reload response from temad daemon at {}",
            socket_path.display()
        )
    })?;

    if read == 0 {
        bail!(
            "temad daemon at {} closed the control socket without responding",
            socket_path.display()
        );
    }

    parse_response(&response)
}

pub(crate) struct ControlSocket {
    listener: UnixListener,
    socket_path: PathBuf,
}

impl ControlSocket {
    pub(crate) fn bind(config_path: &Path) -> Result<Self> {
        let socket_path = resolve_control_socket_path(config_path)?;

        if let Some(parent) = socket_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "Failed to create temad control socket directory: {}",
                    parent.display()
                )
            })?;
        }

        cleanup_stale_socket(&socket_path)?;

        let listener = UnixListener::bind(&socket_path).with_context(|| {
            format!(
                "Failed to bind temad control socket at {}",
                socket_path.display()
            )
        })?;

        Ok(Self {
            listener,
            socket_path,
        })
    }

    pub(crate) async fn accept(&self) -> Result<(UnixStream, tokio::net::unix::SocketAddr)> {
        self.listener.accept().await.with_context(|| {
            format!(
                "Failed to accept temad control connection at {}",
                self.socket_path.display()
            )
        })
    }

    pub(crate) fn socket_path(&self) -> &Path {
        &self.socket_path
    }
}

impl Drop for ControlSocket {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.socket_path);
    }
}

pub(crate) async fn read_command(stream: &mut UnixStream) -> Result<ControlCommand> {
    let mut line = String::new();
    let mut reader = BufReader::new(stream);
    let read = reader
        .read_line(&mut line)
        .await
        .context("Failed to read temad control command")?;

    if read == 0 {
        bail!("temad control client closed connection without sending a command");
    }

    parse_command(&line)
}

pub(crate) async fn read_command_with_timeout(stream: &mut UnixStream) -> Result<ControlCommand> {
    read_command_within(stream, CONTROL_READ_TIMEOUT).await
}

async fn read_command_within(
    stream: &mut UnixStream,
    duration: Duration,
) -> Result<ControlCommand> {
    timeout(duration, read_command(stream))
        .await
        .map_err(|_| anyhow!("Timed out while waiting for a temad control command"))?
}

pub(crate) async fn respond_ok(stream: &mut UnixStream, message: &str) -> Result<()> {
    let response = format!("ok {message}\n");
    stream
        .write_all(response.as_bytes())
        .await
        .context("Failed to write temad control success response")?;
    stream
        .flush()
        .await
        .context("Failed to flush temad control success response")
}

pub(crate) async fn respond_error(stream: &mut UnixStream, message: &str) -> Result<()> {
    let sanitized = message.replace('\n', " ");
    let response = format!("error {sanitized}\n");
    stream
        .write_all(response.as_bytes())
        .await
        .context("Failed to write temad control error response")?;
    stream
        .flush()
        .await
        .context("Failed to flush temad control error response")
}

fn parse_command(line: &str) -> Result<ControlCommand> {
    match line.trim() {
        "reload" => Ok(ControlCommand::Reload),
        other => bail!("Unsupported temad control command: {other}"),
    }
}

fn parse_response(line: &str) -> Result<String> {
    let trimmed = line.trim();
    if let Some(message) = trimmed.strip_prefix("ok ") {
        return Ok(message.to_string());
    }
    if let Some(message) = trimmed.strip_prefix("error ") {
        bail!(message.to_string());
    }
    bail!("Unexpected temad control response: {trimmed}")
}

fn cleanup_stale_socket(socket_path: &Path) -> Result<()> {
    if !socket_path.exists() {
        return Ok(());
    }

    match std::os::unix::net::UnixStream::connect(socket_path) {
        Ok(_) => bail!(
            "Another temad daemon is already listening on {}",
            socket_path.display()
        ),
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::ConnectionRefused | ErrorKind::NotFound | ErrorKind::AddrNotAvailable
            ) =>
        {
            fs::remove_file(socket_path).with_context(|| {
                format!(
                    "Failed to remove stale temad control socket at {}",
                    socket_path.display()
                )
            })?;
            Ok(())
        }
        Err(error) => Err(error).with_context(|| {
            format!(
                "Failed to probe existing temad control socket at {}",
                socket_path.display()
            )
        }),
    }
}

fn stable_config_id(config_path: &Path) -> u64 {
    let mut hasher = Fnv1aHasher::default();
    hasher.write(config_path.to_string_lossy().as_bytes());
    hasher.finish()
}

#[derive(Default)]
struct Fnv1aHasher(u64);

impl Hasher for Fnv1aHasher {
    fn finish(&self) -> u64 {
        if self.0 == 0 {
            0xcbf29ce484222325
        } else {
            self.0
        }
    }

    fn write(&mut self, bytes: &[u8]) {
        let mut hash = if self.0 == 0 {
            0xcbf29ce484222325
        } else {
            self.0
        };
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        self.0 = hash;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_socket_path_is_deterministic() {
        let runtime_dir = Path::new("/run/user/1000");
        let config_path = Path::new("/home/user/.config/temad/config.toml");

        let first = control_socket_path_for(runtime_dir, config_path);
        let second = control_socket_path_for(runtime_dir, config_path);

        assert_eq!(first, second);
        assert!(first.to_string_lossy().contains("control-"));
    }

    #[test]
    fn control_socket_path_changes_for_different_configs() {
        let runtime_dir = Path::new("/run/user/1000");
        let first = control_socket_path_for(runtime_dir, Path::new("/tmp/a.toml"));
        let second = control_socket_path_for(runtime_dir, Path::new("/tmp/b.toml"));

        assert_ne!(first, second);
    }

    #[test]
    fn parse_command_accepts_reload() {
        assert_eq!(
            parse_command("reload\n").expect("reload parses"),
            ControlCommand::Reload
        );
    }

    #[test]
    fn parse_response_accepts_ok() {
        let response = parse_response("ok reloaded\n").expect("ok response parses");
        assert_eq!(response, "reloaded");
    }

    #[test]
    fn parse_response_rejects_error() {
        let err = parse_response("error invalid config\n").expect_err("error response fails");
        assert!(err.to_string().contains("invalid config"));
    }

    #[tokio::test]
    async fn request_reload_round_trips_over_socket() {
        let runtime_dir = tempfile::tempdir().expect("tempdir");
        let config_dir = tempfile::tempdir().expect("tempdir");
        let config_path = config_dir.path().join("config.toml");
        let socket_path = control_socket_path_for(runtime_dir.path(), &config_path);

        fs::create_dir_all(socket_path.parent().expect("socket parent"))
            .expect("create socket dir");

        let listener = UnixListener::bind(&socket_path).expect("bind socket");
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept connection");
            let command = read_command(&mut stream).await.expect("read command");
            assert_eq!(command, ControlCommand::Reload);
            respond_ok(&mut stream, "reloaded")
                .await
                .expect("write response");
        });

        let response = request_command(&socket_path, "reload")
            .await
            .expect("reload request succeeds");
        assert_eq!(response, "reloaded");

        server.await.expect("server task joins");
    }

    #[tokio::test]
    async fn request_command_missing_socket_has_actionable_error() {
        let socket_path = tempfile::tempdir()
            .expect("tempdir")
            .path()
            .join("missing.sock");

        let err = request_command(&socket_path, "reload")
            .await
            .expect_err("missing socket should fail");

        let message = err.to_string();
        assert!(message.contains("No reload-capable temad daemon is listening"));
        assert!(message.contains("restart `temad daemon`"));
    }

    #[tokio::test]
    async fn read_command_with_timeout_rejects_stalled_clients() {
        let socket_dir = tempfile::tempdir().expect("tempdir");
        let socket_path = socket_dir.path().join("control.sock");
        let listener = UnixListener::bind(&socket_path).expect("bind socket");

        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept connection");
            read_command_within(&mut stream, Duration::from_millis(10))
                .await
                .expect_err("stalled client should time out")
                .to_string()
        });

        let _client = UnixStream::connect(&socket_path)
            .await
            .expect("connect client");

        let message = server.await.expect("server joins");
        assert!(message.contains("Timed out while waiting for a temad control command"));
    }
}
