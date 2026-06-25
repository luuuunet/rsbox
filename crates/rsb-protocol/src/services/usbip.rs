//! USB/IP server (OP_REQ_DEVLIST / OP_REQ_IMPORT / OP_REQ_EXPORT).

use super::listen::parse_listen;
use anyhow::Result;
use serde_json::Value;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::info;

const USBIP_OP_REQ_DEVLIST: u16 = 0x8005;
const USBIP_OP_REP_DEVLIST: u16 = 0x8006;
const USBIP_OP_REQ_IMPORT: u16 = 0x8003;
const USBIP_OP_REP_IMPORT: u16 = 0x8004;
const USBIP_OP_REQ_EXPORT: u16 = 0x8001;
const USBIP_OP_REP_EXPORT: u16 = 0x8002;
const USBIP_VERSION: u16 = 0x0111;
const USBIP_ST_OK: u32 = 0;
const USBIP_ST_NA: u32 = 1;
const USBIP_CMD_SUBMIT: u32 = 0x0001;
const USBIP_CMD_UNLINK: u32 = 0x0002;
const USBIP_RET_SUBMIT: u32 = 0x0003;
const USBIP_RET_UNLINK: u32 = 0x0004;
const USBIP_HEADER_LEN: usize = 48;

#[derive(Clone, Default)]
struct UsbDevice {
    busid: String,
    vendor: u16,
    product: u16,
    class: u8,
    subclass: u8,
    protocol: u8,
}

pub struct UsbipServerService {
    tag: String,
    listen: SocketAddr,
    devices: Arc<Mutex<HashMap<String, UsbDevice>>>,
    shutdown: tokio::sync::watch::Sender<bool>,
    handle: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl UsbipServerService {
    pub fn new(tag: String, raw: Value) -> Result<Self> {
        let (shutdown, _) = tokio::sync::watch::channel(false);
        let mut devices = HashMap::new();
        if let Some(list) = raw.get("devices").and_then(|v| v.as_array()) {
            for (i, d) in list.iter().enumerate() {
                let busid = d
                    .get("busid")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&format!("1-{i}"))
                    .to_string();
                devices.insert(
                    busid.clone(),
                    UsbDevice {
                        busid,
                        vendor: d.get("vendor").and_then(|v| v.as_u64()).unwrap_or(0) as u16,
                        product: d.get("product").and_then(|v| v.as_u64()).unwrap_or(0) as u16,
                        class: d.get("class").and_then(|v| v.as_u64()).unwrap_or(0) as u8,
                        subclass: d.get("subclass").and_then(|v| v.as_u64()).unwrap_or(0) as u8,
                        protocol: d.get("protocol").and_then(|v| v.as_u64()).unwrap_or(0) as u8,
                    },
                );
            }
        }
        Ok(Self {
            tag,
            listen: parse_listen(&raw)?,
            devices: Arc::new(Mutex::new(devices)),
            shutdown,
            handle: tokio::sync::Mutex::new(None),
        })
    }

    pub async fn start(&self) -> Result<()> {
        let listener = tokio::net::TcpListener::bind(self.listen).await?;
        info!(tag = %self.tag, %self.listen, "usbip-server listening");
        let devices = self.devices.clone();
        let mut shutdown = self.shutdown.subscribe();
        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown.changed() => { if *shutdown.borrow() { break; } }
                    accept = listener.accept() => {
                        let Ok((mut stream, peer)) = accept else { break };
                        let devices = devices.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_usbip_client(&mut stream, &devices).await {
                                tracing::debug!(%peer, error = %e, "usbip session ended");
                            }
                        });
                    }
                }
            }
        });
        *self.handle.lock().await = Some(handle);
        Ok(())
    }

    pub async fn close(&self) -> Result<()> {
        let _ = self.shutdown.send(true);
        if let Some(h) = self.handle.lock().await.take() {
            h.abort();
        }
        Ok(())
    }
}

async fn handle_usbip_client(
    stream: &mut tokio::net::TcpStream,
    devices: &Arc<Mutex<HashMap<String, UsbDevice>>>,
) -> Result<()> {
    let mut hdr = [0u8; 8];
    stream.read_exact(&mut hdr).await?;
    let version = u16::from_be_bytes([hdr[0], hdr[1]]);
    let opcode = u16::from_be_bytes([hdr[2], hdr[3]]);
    if version != USBIP_VERSION {
        anyhow::bail!("unsupported usbip version {version:#x}");
    }
    match opcode {
        USBIP_OP_REQ_DEVLIST => reply_devlist(stream, devices).await?,
        USBIP_OP_REQ_IMPORT => {
            if reply_import(stream, devices).await? {
                usbip_data_loop(stream).await?;
            }
        },
        USBIP_OP_REQ_EXPORT => reply_export(stream).await?,
        other => anyhow::bail!("unsupported usbip opcode {other:#x}"),
    }
    Ok(())
}

async fn usbip_data_loop(stream: &mut tokio::net::TcpStream) -> Result<()> {
    let mut hdr = [0u8; USBIP_HEADER_LEN];
    loop {
        match stream.read_exact(&mut hdr).await {
            Ok(_) => {},
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.into()),
        }
        let cmd = u32::from_be_bytes(hdr[0..4].try_into()?);
        match cmd {
            USBIP_CMD_SUBMIT => {
                let data_len = u32::from_be_bytes(hdr[24..28].try_into()?) as usize;
                if data_len > 0 {
                    let mut payload = vec![0u8; data_len];
                    stream.read_exact(&mut payload).await?;
                }
                hdr[0..4].copy_from_slice(&USBIP_RET_SUBMIT.to_be_bytes());
                stream.write_all(&hdr).await?;
            },
            USBIP_CMD_UNLINK => {
                hdr[0..4].copy_from_slice(&USBIP_RET_UNLINK.to_be_bytes());
                stream.write_all(&hdr).await?;
            },
            _ => break,
        }
    }
    Ok(())
}

async fn reply_devlist(
    stream: &mut tokio::net::TcpStream,
    devices: &Arc<Mutex<HashMap<String, UsbDevice>>>,
) -> Result<()> {
    let list = devices.lock().unwrap().clone();
    let mut hdr = [0u8; 8];
    hdr[0..2].copy_from_slice(&USBIP_VERSION.to_be_bytes());
    hdr[2..4].copy_from_slice(&USBIP_OP_REP_DEVLIST.to_be_bytes());
    hdr[4..8].copy_from_slice(&(list.len() as u32).to_be_bytes());
    stream.write_all(&hdr).await?;
    for dev in list.values() {
        let mut rec = vec![0u8; 256];
        let path = format!("/sys/devices/pci0000:00/{}", dev.busid);
        let path_bytes = path.as_bytes();
        let path_len = path_bytes.len().min(255) as u8;
        rec[0] = path_len;
        rec[1..1 + path_len as usize].copy_from_slice(&path_bytes[..path_len as usize]);
        let off = 256 - 32;
        rec[off..off + 2].copy_from_slice(&dev.vendor.to_be_bytes());
        rec[off + 2..off + 4].copy_from_slice(&dev.product.to_be_bytes());
        rec[off + 4] = dev.class;
        rec[off + 5] = dev.subclass;
        rec[off + 6] = dev.protocol;
        stream.write_all(&rec).await?;
    }
    Ok(())
}

async fn reply_import(
    stream: &mut tokio::net::TcpStream,
    devices: &Arc<Mutex<HashMap<String, UsbDevice>>>,
) -> Result<bool> {
    let mut busid = [0u8; 32];
    stream.read_exact(&mut busid).await?;
    let busid_str = String::from_utf8_lossy(&busid)
        .trim_end_matches('\0')
        .to_string();
    let status = if devices.lock().unwrap().contains_key(&busid_str) {
        USBIP_ST_OK
    } else {
        USBIP_ST_NA
    };
    let mut hdr = [0u8; 8];
    hdr[0..2].copy_from_slice(&USBIP_VERSION.to_be_bytes());
    hdr[2..4].copy_from_slice(&USBIP_OP_REP_IMPORT.to_be_bytes());
    hdr[4..8].copy_from_slice(&status.to_be_bytes());
    stream.write_all(&hdr).await?;
    Ok(status == USBIP_ST_OK)
}

async fn reply_export(stream: &mut tokio::net::TcpStream) -> Result<()> {
    let mut hdr = [0u8; 8];
    hdr[0..2].copy_from_slice(&USBIP_VERSION.to_be_bytes());
    hdr[2..4].copy_from_slice(&USBIP_OP_REP_EXPORT.to_be_bytes());
    hdr[4..8].copy_from_slice(&USBIP_ST_OK.to_be_bytes());
    stream.write_all(&hdr).await?;
    Ok(())
}

pub struct UsbipClientService {
    tag: String,
    remote: String,
    busid: String,
    shutdown: tokio::sync::watch::Sender<bool>,
    handle: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl UsbipClientService {
    pub fn new(tag: String, raw: Value) -> Result<Self> {
        let (shutdown, _) = tokio::sync::watch::channel(false);
        let remote = raw
            .get("server")
            .and_then(|v| v.as_str())
            .unwrap_or("127.0.0.1")
            .to_string();
        let port = raw
            .get("server_port")
            .and_then(|v| v.as_u64())
            .unwrap_or(3240);
        let busid = raw
            .get("busid")
            .and_then(|v| v.as_str())
            .unwrap_or("1-1")
            .to_string();
        Ok(Self {
            tag,
            remote: format!("{remote}:{port}"),
            busid,
            shutdown,
            handle: tokio::sync::Mutex::new(None),
        })
    }

    pub async fn start(&self) -> Result<()> {
        info!(tag = %self.tag, remote = %self.remote, busid = %self.busid, "usbip-client starting");
        let remote = self.remote.clone();
        let busid = self.busid.clone();
        let mut shutdown = self.shutdown.subscribe();
        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown.changed() => { if *shutdown.borrow() { break; } }
                    _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => {
                        if let Ok(mut stream) = tokio::net::TcpStream::connect(&remote).await {
                            let _ = usbip_import(&mut stream, &busid).await;
                        }
                    }
                }
            }
        });
        *self.handle.lock().await = Some(handle);
        Ok(())
    }

    pub async fn close(&self) -> Result<()> {
        let _ = self.shutdown.send(true);
        if let Some(h) = self.handle.lock().await.take() {
            h.abort();
        }
        Ok(())
    }
}

async fn usbip_import(stream: &mut tokio::net::TcpStream, busid: &str) -> Result<()> {
    let mut hdr = [0u8; 8];
    hdr[0..2].copy_from_slice(&USBIP_VERSION.to_be_bytes());
    hdr[2..4].copy_from_slice(&USBIP_OP_REQ_IMPORT.to_be_bytes());
    hdr[4..8].copy_from_slice(&0u32.to_be_bytes());
    stream.write_all(&hdr).await?;
    let mut bus = [0u8; 32];
    bus[..busid.len().min(32)].copy_from_slice(busid.as_bytes());
    stream.write_all(&bus).await?;
    let mut resp = [0u8; 8];
    stream.read_exact(&mut resp).await?;
    let status = u32::from_be_bytes(resp[4..8].try_into()?);
    if status != USBIP_ST_OK {
        anyhow::bail!("usbip import status {status}");
    }
    Ok(())
}
