//! SSH direct-tcpip client via libssh2 (ssh2 crate).

use anyhow::{Context, Result};
use base64::Engine;
use rsb_core::{proxy_box, ProxyConn};
use ssh2::Session;
use std::io::{Read, Write};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::mpsc;

pub struct SshConfig {
    pub server: String,
    pub port: u16,
    pub username: String,
    pub password: Option<String>,
    pub private_key: Option<String>,
    pub private_key_path: Option<String>,
    pub private_key_passphrase: Option<String>,
    pub host_keys: Vec<String>,
}

pub struct SshSessionPool {
    config: SshConfig,
    session: tokio::sync::Mutex<Option<Arc<Session>>>,
}

impl SshSessionPool {
    pub fn new(config: SshConfig) -> Self {
        Self {
            config,
            session: tokio::sync::Mutex::new(None),
        }
    }

    pub async fn dial_tcp(&self, destination: SocketAddr, _domain: Option<&str>) -> Result<ProxyConn> {
        let mut guard = self.session.lock().await;
        if guard.is_none() {
            *guard = Some(connect(&self.config).await?);
        }
        let session = guard.as_ref().unwrap().clone();
        let host = match destination.ip() {
            std::net::IpAddr::V4(v4) => v4.to_string(),
            std::net::IpAddr::V6(v6) => v6.to_string(),
        };
        let port = destination.port();
        let stream = tokio::task::spawn_blocking(move || open_channel(session, &host, port))
            .await
            .context("ssh channel task")??;
        Ok(proxy_box(stream))
    }
}

fn open_channel(session: Arc<Session>, host: &str, port: u16) -> Result<SshAsyncStream> {
    let channel = session
        .channel_direct_tcpip(host, port, None)
        .context("ssh direct-tcpip")?;
    let mut stream = channel.stream(0);
    let (read_tx, read_rx) = mpsc::unbounded_channel();
    let (write_tx, mut write_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    std::thread::Builder::new()
        .name("ssh-channel".into())
        .spawn(move || {
            let mut buf = [0u8; 65535];
            loop {
                while let Ok(data) = write_rx.try_recv() {
                    if stream.write_all(&data).is_err() {
                        return;
                    }
                }
                match stream.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if read_tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    },
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    },
                    Err(_) => break,
                }
            }
        })
        .context("spawn ssh relay thread")?;
    Ok(SshAsyncStream {
        read_rx,
        write_tx,
        read_buf: Vec::new(),
    })
}

struct SshAsyncStream {
    read_rx: mpsc::UnboundedReceiver<Vec<u8>>,
    write_tx: mpsc::UnboundedSender<Vec<u8>>,
    read_buf: Vec<u8>,
}

impl AsyncRead for SshAsyncStream {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        if self.read_buf.is_empty() {
            match self.read_rx.poll_recv(cx) {
                std::task::Poll::Ready(Some(chunk)) => self.read_buf = chunk,
                std::task::Poll::Ready(None) => return std::task::Poll::Ready(Ok(())),
                std::task::Poll::Pending => return std::task::Poll::Pending,
            }
        }
        let n = self.read_buf.len().min(buf.remaining());
        buf.put_slice(&self.read_buf[..n]);
        self.read_buf.drain(..n);
        std::task::Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for SshAsyncStream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        match self.write_tx.send(buf.to_vec()) {
            Ok(()) => std::task::Poll::Ready(Ok(buf.len())),
            Err(_) => std::task::Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "ssh channel closed",
            ))),
        }
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
}

async fn connect(config: &SshConfig) -> Result<Arc<Session>> {
    let server = config.server.clone();
    let port = config.port;
    let username = config.username.clone();
    let password = config.password.clone();
    let private_key = config.private_key.clone();
    let private_key_path = config.private_key_path.clone();
    let private_key_passphrase = config.private_key_passphrase.clone();
    let host_keys = config.host_keys.clone();

    tokio::task::spawn_blocking(move || {
        let tcp = std::net::TcpStream::connect(format!("{server}:{port}"))
            .with_context(|| format!("ssh tcp connect {server}:{port}"))?;
        let mut sess = Session::new().context("ssh session new")?;
        sess.set_tcp_stream(tcp);
        sess.handshake().context("ssh handshake")?;
        if !host_keys.is_empty() {
            let (key_bytes, _) = sess.host_key().context("ssh host key missing")?;
            let hostkey = base64::engine::general_purpose::STANDARD.encode(key_bytes);
            if !host_keys.iter().any(|k| k == &hostkey) {
                anyhow::bail!("ssh host key verification failed");
            }
        }
        let authed = if let Some(ref pw) = password {
            sess.userauth_password(&username, pw)
                .context("ssh password auth")?;
            sess.authenticated()
        } else if let Some(ref path) = private_key_path {
            sess.userauth_pubkey_file(
                &username,
                None,
                path.as_ref(),
                private_key_passphrase.as_deref(),
            )
            .context("ssh publickey auth")?;
            sess.authenticated()
        } else if let Some(ref inline) = private_key {
            let path = std::env::temp_dir().join(format!("rsbox-ssh-{}.key", uuid::Uuid::new_v4()));
            std::fs::write(&path, inline).context("write temp ssh key")?;
            sess.userauth_pubkey_file(&username, None, &path, private_key_passphrase.as_deref())
                .context("ssh publickey auth")?;
            let authed = sess.authenticated();
            let _ = std::fs::remove_file(path);
            authed
        } else {
            anyhow::bail!("ssh outbound requires password or private_key")
        };
        if !authed {
            anyhow::bail!("ssh authentication failed");
        }
        Ok(Arc::new(sess))
    })
    .await
    .context("ssh connect task")?
}

static POOLS: parking_lot::Mutex<Option<dashmap::DashMap<String, Arc<SshSessionPool>>>> =
    parking_lot::Mutex::new(None);

pub fn pool_for(config: SshConfig) -> Arc<SshSessionPool> {
    let mut guard = POOLS.lock();
    if guard.is_none() {
        *guard = Some(dashmap::DashMap::new());
    }
    let map = guard.as_ref().unwrap();
    let key = format!("{}:{}:{}", config.server, config.port, config.username);
    if let Some(p) = map.get(&key) {
        return p.clone();
    }
    let pool = Arc::new(SshSessionPool::new(config));
    map.insert(key, pool.clone());
    pool
}
