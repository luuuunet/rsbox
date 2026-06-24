//! Default network interface detection and socket binding.

use anyhow::{Context, Result};
use std::net::{IpAddr, SocketAddr};
use tokio::net::{TcpStream, UdpSocket};

pub fn detect_default_interface() -> Result<String> {
    crate::platform::detect_default_interface()
}

pub async fn tcp_connect_via(
    destination: SocketAddr,
    interface: Option<&str>,
) -> Result<TcpStream> {
    let Some(iface) = interface else {
        return Ok(TcpStream::connect(destination).await?);
    };
    let iface = iface.to_string();
    tokio::task::spawn_blocking(move || blocking_tcp_connect(destination, &iface))
        .await
        .context("join blocking connect")?
}

fn blocking_tcp_connect(destination: SocketAddr, iface: &str) -> Result<TcpStream> {
    let local_ip = interface_local_ip(iface)?;
    let domain = if destination.is_ipv4() {
        socket2::Domain::IPV4
    } else {
        socket2::Domain::IPV6
    };
    let socket = socket2::Socket::new(domain, socket2::Type::STREAM, None)?;
    let bind_addr = socket2::SockAddr::from(local_ip);
    socket.bind(&bind_addr)?;
    socket.set_nonblocking(false)?;
    let dest_addr = socket2::SockAddr::from(destination);
    socket.connect(&dest_addr)?;
    socket.set_nonblocking(true)?;
    Ok(TcpStream::from_std(std::net::TcpStream::from(socket))?)
}

pub async fn udp_bind_via(interface: Option<&str>) -> Result<UdpSocket> {
    if let Some(iface) = interface {
        let iface = iface.to_string();
        tokio::task::spawn_blocking(move || {
            let local_ip = interface_local_ip(&iface)?;
            let s = socket2::Socket::new(socket2::Domain::IPV4, socket2::Type::DGRAM, None)?;
            let bind_addr = socket2::SockAddr::from(local_ip);
            s.bind(&bind_addr)?;
            s.set_nonblocking(true)?;
            Ok(UdpSocket::from_std(std::net::UdpSocket::from(s))?)
        })
        .await
        .context("join udp bind")?
    } else {
        Ok(UdpSocket::bind("0.0.0.0:0").await?)
    }
}

fn interface_local_ip(name: &str) -> Result<SocketAddr> {
    let addrs = if_addrs::get_if_addrs().context("list interfaces")?;
    for iface in addrs {
        if iface.name == name && !iface.is_loopback() {
            if let if_addrs::IfAddr::V4(v4) = iface.addr {
                return Ok(SocketAddr::new(IpAddr::V4(v4.ip), 0));
            }
        }
    }
    anyhow::bail!("interface `{name}` has no usable IPv4 address")
}

pub fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            o[0] == 10
                || (o[0] == 172 && (16..=31).contains(&o[1]))
                || (o[0] == 192 && o[1] == 168)
                || o[0] == 127
                || (o[0] == 169 && o[1] == 254)
        }
        IpAddr::V6(v6) => {
            let o = v6.octets();
            o == [0u8; 16]
                || o[0] == 0
                    && o[1] == 0
                    && o[2] == 0
                    && o[3] == 0
                    && o[4] == 0
                    && o[5] == 0
                    && o[6] == 0
                    && o[7] == 0
                    && o[8] == 0
                    && o[9] == 0
                    && o[10] == 0
                    && o[11] == 0
                    && o[12] == 0
                    && o[13] == 0
                    && o[14] == 0
                    && o[15] == 1
                || (o[0] & 0xfe) == 0xfc
                || (o[0] == 0xfe && (o[1] & 0xc0) == 0x80)
        }
    }
}
