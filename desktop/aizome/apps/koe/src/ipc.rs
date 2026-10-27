use crate::protocol::{MAX_MESSAGE, Request, Response};
use anyhow::{Context, Result, bail, ensure};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    fs::{self, OpenOptions},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
};

pub async fn read_line<R: AsyncBufRead + Unpin>(reader: &mut R) -> Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    loop {
        let buffer = reader.fill_buf().await?;
        if buffer.is_empty() {
            if line.is_empty() {
                return Ok(None);
            }
            bail!("incomplete IPC frame");
        }
        let end = buffer.iter().position(|b| *b == b'\n');
        let n = end.map_or(buffer.len(), |i| i + 1);
        ensure!(line.len() + n <= MAX_MESSAGE, "IPC frame exceeds 64 KiB");
        line.extend_from_slice(&buffer[..n]);
        reader.consume(n);
        if end.is_some() {
            return Ok(Some(line));
        }
    }
}
pub async fn write_json<W: AsyncWrite + Unpin, T: Serialize>(writer: &mut W, value: &T) -> Result<()> {
    let mut line = serde_json::to_vec(value)?;
    ensure!(line.len() < MAX_MESSAGE, "IPC response exceeds 64 KiB");
    line.push(b'\n');
    tokio::time::timeout(Duration::from_secs(3), writer.write_all(&line)).await.context("IPC write timed out")??;
    Ok(())
}
pub async fn request(path: &Path, request: &Request) -> Result<Response> {
    let timeout = if matches!(request.vocabulary, Some(crate::protocol::VocabularyRequest::ExportExamples { .. })) {
        Duration::from_secs(120)
    } else {
        Duration::from_secs(5)
    };
    exchange_with_timeout(path, request, timeout).await
}
pub async fn exchange<T: Serialize, R: DeserializeOwned>(path: &Path, value: &T) -> Result<R> {
    exchange_with_timeout(path, value, Duration::from_secs(5)).await
}
async fn exchange_with_timeout<T: Serialize, R: DeserializeOwned>(
    path: &Path,
    value: &T,
    timeout: Duration,
) -> Result<R> {
    tokio::time::timeout(timeout, async {
        let mut stream = UnixStream::connect(path)
            .await
            .with_context(|| format!("cannot connect to {}; is the service running?", path.display()))?;
        write_json(&mut stream, value).await?;
        let line = read_line(&mut BufReader::new(stream)).await?.context("IPC peer closed without a response")?;
        Ok(serde_json::from_slice(&line)?)
    })
    .await
    .context("IPC request timed out")?
}
/// One request and its response without a runtime: for the window's
/// workers and the service switches. `timeout` bounds each read and write.
pub fn request_blocking(path: &Path, request: &Request, timeout: Duration) -> Result<Response> {
    use std::io::{BufRead, Read, Write};
    let mut stream = std::os::unix::net::UnixStream::connect(path)
        .with_context(|| format!("cannot connect to {}; is the service running?", path.display()))?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    let mut line = serde_json::to_vec(request)?;
    ensure!(line.len() < MAX_MESSAGE, "IPC request exceeds 64 KiB");
    line.push(b'\n');
    stream.write_all(&line)?;
    let mut reply = Vec::new();
    std::io::BufReader::new(stream).take(MAX_MESSAGE as u64).read_until(b'\n', &mut reply)?;
    ensure!(reply.ends_with(b"\n"), "IPC peer closed without a response");
    Ok(serde_json::from_slice(&reply)?)
}
pub fn socket_path() -> Result<PathBuf> {
    Ok(crate::config::runtime_dir()?.join("control.sock"))
}

pub struct SocketGuard {
    path: PathBuf,
    _lock: fs::File,
}
impl SocketGuard {
    pub fn bind(path: &Path) -> Result<(Self, UnixListener)> {
        let dir = path.parent().context("socket has no parent")?;
        match fs::DirBuilder::new().recursive(false).mode(0o700).create(dir) {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(e) => return Err(e.into()),
        }
        let meta = fs::symlink_metadata(dir)?;
        // SAFETY: geteuid has no preconditions and changes no state.
        let uid = unsafe { libc::geteuid() };
        ensure!(
            meta.is_dir() && meta.uid() == uid && meta.mode() & 0o077 == 0,
            "Koe runtime directory must be owned by you with mode 0700"
        );
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(dir.join("daemon.lock"))?;
        lock.try_lock().map_err(|_| anyhow::anyhow!("Koe is already running"))?;
        if path.exists() {
            fs::remove_file(path)?;
        }
        let listener = UnixListener::bind(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        Ok((Self { path: path.to_owned(), _lock: lock }, listener))
    }
}
use std::os::unix::fs::DirBuilderExt;
impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}
