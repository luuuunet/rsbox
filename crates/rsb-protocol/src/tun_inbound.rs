// TUN 入站实现
use anyhow::{Context, Result};
use async_trait::async_trait;
use rsb_core::{BoxError, Inbound, ServiceContext};
use serde_json::Value;
use std::net::IpAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tun::{Configuration, Device};

pub struct TunInbound {
    tag: String,
    name: String,
    address: Vec<IpAddr>,
    mtu: u32,
    auto_route: bool,
    strict_route: bool,
    stack: String,
}

impl TunInbound {
    pub fn parse(tag: String, raw: &Value) -> Result<Self> {
        let address = raw
            .get("address")
            .and_then(|v| v.as_array())
            .context("tun: address required")?
            .iter()
            .filter_map(|v| v.as_str()?.parse().ok())
            .collect::<Vec<IpAddr>>();

        if address.is_empty() {
            anyhow::bail!("tun: at least one address required");
        }

        Ok(Self {
            tag,
            name: raw
                .get("interface_name")
                .and_then(|v| v.as_str())
                .unwrap_or("rsbox-tun")
                .to_string(),
            address,
            mtu: raw
                .get("mtu")
                .and_then(|v| v.as_u64())
                .unwrap_or(9000) as u32,
            auto_route: raw
                .get("auto_route")
                .and_then(|v| v.as_bool())
                .unwrap_or(true),
            strict_route: raw
                .get("strict_route")
                .and_then(|v| v.as_bool())
                .unwrap_or(true),
            stack: raw
                .get("stack")
                .and_then(|v| v.as_str())
                .unwrap_or("system")
                .to_string(),
        })
    }

    async fn setup_routes(&self) -> Result<()> {
        #[cfg(target_os = "linux")]
        {
            use std::process::Command;

            // 添加默认路由
            if self.auto_route {
                Command::new("ip")
                    .args(&["route", "add", "default", "dev", &self.name, "table", "100"])
                    .output()
                    .context("Failed to add default route")?;

                Command::new("ip")
                    .args(&["rule", "add", "fwmark", "0x100", "table", "100"])
                    .output()
                    .context("Failed to add routing rule")?;
            }

            // 设置接口为 UP
            Command::new("ip")
                .args(&["link", "set", &self.name, "up"])
                .output()
                .context("Failed to bring up interface")?;
        }

        #[cfg(target_os = "macos")]
        {
            use std::process::Command;

            if self.auto_route {
                Command::new("route")
                    .args(&["add", "-net", "0.0.0.0/1", "-interface", &self.name])
                    .output()?;
                Command::new("route")
                    .args(&["add", "-net", "128.0.0.0/1", "-interface", &self.name])
                    .output()?;
            }
        }

        #[cfg(target_os = "windows")]
        {
            // Windows 路由设置
            if self.auto_route {
                use std::process::Command;
                Command::new("netsh")
                    .args(&["interface", "ip", "add", "route", "0.0.0.0/0", &self.name])
                    .output()?;
            }
        }

        Ok(())
    }
}

#[async_trait]
impl Inbound for TunInbound {
    fn tag(&self) -> &str {
        &self.tag
    }

    fn kind(&self) -> &str {
        "tun"
    }

    async fn start(&self, ctx: ServiceContext) -> Result<(), BoxError> {
        tracing::info!(
            tag = %self.tag,
            name = %self.name,
            addresses = ?self.address,
            mtu = self.mtu,
            "Starting TUN inbound"
        );

        // 创建 TUN 设备
        let mut config = Configuration::default();
        config
            .name(&self.name)
            .mtu(self.mtu as i32)
            .up();

        // 设置地址
        for addr in &self.address {
            match addr {
                IpAddr::V4(v4) => {
                    config.address(*v4).netmask(std::net::Ipv4Addr::new(255, 255, 255, 0));
                }
                IpAddr::V6(v6) => {
                    config.address(*v6);
                }
            }
        }

        #[cfg(target_os = "linux")]
        config.platform(|config| {
            config.packet_information(true);
        });

        let device = tun::create_as_async(&config).context("Failed to create TUN device")?;

        // 设置路由
        self.setup_routes().await?;

        tracing::info!(tag = %self.tag, "TUN device created and routes configured");

        // 使用 ipstack 处理 IP 数据包
        let mut stack = ipstack::IpStack::new(device, self.mtu as u16);

        loop {
            match stack.accept().await {
                Ok(ipstack::stream::IpStackStream::Tcp(mut tcp)) => {
                    let ctx = ctx.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_tcp_stream(&mut tcp, ctx).await {
                            tracing::debug!("TUN TCP error: {}", e);
                        }
                    });
                }
                Ok(ipstack::stream::IpStackStream::Udp(udp)) => {
                    let ctx = ctx.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_udp_stream(udp, ctx).await {
                            tracing::debug!("TUN UDP error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    tracing::error!("TUN accept error: {}", e);
                    break;
                }
            }
        }

        Ok(())
    }

    async fn close(&self) -> Result<(), BoxError> {
        tracing::info!(tag = %self.tag, "Closing TUN inbound");
        Ok(())
    }
}

async fn handle_tcp_stream(
    tcp: &mut ipstack::stream::IpStackTcpStream,
    ctx: ServiceContext,
) -> Result<()> {
    let dest = tcp.peer_addr();

    tracing::debug!("TUN TCP connection to {}", dest);

    // 路由决策
    let outbound = ctx.router.route_tcp(&dest, None).await?;

    // 连接到目标
    let mut remote = outbound.dial_tcp(dest, None).await?;

    // 双向转发
    let (mut tcp_read, mut tcp_write) = tokio::io::split(tcp);
    let (mut remote_read, mut remote_write) = tokio::io::split(&mut remote);

    let client_to_server = async {
        tokio::io::copy(&mut tcp_read, &mut remote_write).await
    };

    let server_to_client = async {
        tokio::io::copy(&mut remote_read, &mut tcp_write).await
    };

    tokio::try_join!(client_to_server, server_to_client)?;

    Ok(())
}

async fn handle_udp_stream(
    mut udp: ipstack::stream::IpStackUdpStream,
    ctx: ServiceContext,
) -> Result<()> {
    let dest = udp.peer_addr();

    tracing::debug!("TUN UDP connection to {}", dest);

    // 路由决策
    let outbound = ctx.router.route_udp(&dest, None).await?;

    // 连接到目标
    let remote = outbound.dial_udp().await?;

    // 双向转发
    let mut buf = vec![0u8; 65535];
    loop {
        tokio::select! {
            result = udp.read(&mut buf) => {
                let n = result?;
                if n == 0 {
                    break;
                }
                remote.send_to(&buf[..n], dest).await?;
            }
            result = remote.recv_from(&mut buf) => {
                let (n, _) = result?;
                udp.write_all(&buf[..n]).await?;
            }
        }
    }

    Ok(())
}
