//! 统一类型注册表 —— 新增 inbound/outbound/service 只需改这一处。

use crate::build_context::BuildContext;
use crate::group::OutboundController;
use crate::{
    direct, dns_inbound, dns_outbound, group, http_outbound, hysteria2, inbound_proxy, legacy,
    legacy_inbound, shadowsocks, socks, trojan, tuic, tun_mode, vless, vmess, wireguard_outbound,
};
use anyhow::{bail, Result};
use rsb_config::{Inbound, Outbound};
use rsb_core::{Dialer, SharedOutboundManager};
use std::sync::Arc;

pub use rsb_constant::{
    ALL_ENDPOINT_TYPES, ALL_INBOUND_TYPES, ALL_OUTBOUND_TYPES, ALL_SERVICE_TYPES,
};

pub fn is_known_inbound(kind: &str) -> bool {
    ALL_INBOUND_TYPES.contains(&kind)
}

pub fn is_known_outbound(kind: &str) -> bool {
    ALL_OUTBOUND_TYPES.contains(&kind)
}

pub fn is_known_service(kind: &str) -> bool {
    ALL_SERVICE_TYPES.contains(&kind)
}

pub fn is_known_endpoint(kind: &str) -> bool {
    ALL_ENDPOINT_TYPES.contains(&kind)
}

pub fn build_outbound(
    ob: &Outbound,
    tag: String,
    ctx: &BuildContext,
    shared: Arc<SharedOutboundManager>,
    controller: &OutboundController,
) -> Result<Box<dyn rsb_core::Outbound>> {
    use rsb_constant::*;
    Ok(match ob.kind.as_str() {
        TYPE_DIRECT => Box::new(direct::DirectOutbound::new(tag, ctx.bind_interface.clone())),
        TYPE_BLOCK => Box::new(direct::BlockOutbound::new(tag)),
        TYPE_SOCKS => Box::new(socks::SocksOutbound::new(tag, ob.raw.clone())?),
        TYPE_HTTP => Box::new(http_outbound::HttpOutbound::new(tag, ob.raw.clone())?),
        TYPE_SHADOWSOCKS => Box::new(shadowsocks::ShadowsocksOutbound::new(tag, ob.raw.clone())?),
        TYPE_HYSTERIA2 => Box::new(hysteria2::Hysteria2Outbound::new(tag, ob.raw.clone())?),
        TYPE_TROJAN => Box::new(trojan::TrojanOutbound::new(tag, ob.raw.clone())?),
        TYPE_VLESS => Box::new(vless::VlessOutbound::new(tag, ob.raw.clone())?),
        TYPE_VMESS => Box::new(vmess::VmessOutbound::new(tag, ob.raw.clone())?),
        TYPE_TUIC => Box::new(tuic::TuicOutbound::new(tag, ob.raw.clone())?),
        TYPE_HYSTERIA => Box::new(legacy::HysteriaOutbound::new(tag, ob.raw.clone())?),
        TYPE_SHADOWTLS => Box::new(legacy::ShadowTlsOutbound::new(tag, ob.raw.clone())?),
        TYPE_ANYTLS => Box::new(legacy::AnyTlsOutbound::new(tag, ob.raw.clone())?),
        TYPE_NAIVE => Box::new(legacy::NaiveOutbound::new(tag, ob.raw.clone())?),
        TYPE_SSH => Box::new(legacy::SshOutbound::new(tag, ob.raw.clone())?),
        TYPE_TOR => Box::new(legacy::TorOutbound::new(tag, ob.raw.clone())?),
        TYPE_WIREGUARD => Box::new(wireguard_outbound::WireGuardOutbound::new(
            tag,
            ob.raw.clone(),
        )?),
        TYPE_DNS => Box::new(dns_outbound::DnsOutbound::new(
            tag,
            ob.raw.clone(),
            ctx.dns.clone(),
        )?),
        TYPE_SELECTOR => {
            let sel = group::SelectorOutbound::new(tag, ob.raw.clone(), shared.clone())?;
            controller.register_selector(sel.control());
            Box::new(sel)
        },
        TYPE_URLTEST => {
            let ut = group::UrlTestOutbound::new(tag, ob.raw.clone(), shared.clone())?;
            controller.register_urltest(ut.control());
            Box::new(ut)
        },
        other => bail!("unknown outbound type: {other}"),
    })
}

pub fn build_inbound(
    ib: &Inbound,
    tag: String,
    ctx: &BuildContext,
    dialer: Arc<Dialer>,
) -> Result<Box<dyn rsb_core::Inbound>> {
    use rsb_constant::*;
    Ok(match ib.kind.as_str() {
        TYPE_MIXED | TYPE_HTTP | TYPE_SOCKS => Box::new(inbound_proxy::MixedInbound::new(
            tag,
            ib.kind.clone(),
            ib.raw.clone(),
            dialer,
            ctx.dns.clone(),
        )?),
        TYPE_DIRECT => Box::new(direct::DirectInbound::new(tag, ib.raw.clone())?),
        TYPE_SHADOWSOCKS => Box::new(shadowsocks::ShadowsocksInbound::new(tag, ib.raw.clone())?),
        TYPE_HYSTERIA2 => Box::new(hysteria2::Hysteria2Inbound::new(tag, ib.raw.clone())?),
        TYPE_TROJAN => Box::new(trojan::TrojanInbound::new(tag, ib.raw.clone())?),
        TYPE_VLESS => Box::new(vless::VlessInbound::new(tag, ib.raw.clone())?),
        TYPE_VMESS => Box::new(vmess::VmessInbound::new(tag, ib.raw.clone())?),
        TYPE_TUIC => Box::new(tuic::TuicInbound::new(tag, ib.raw.clone())?),
        TYPE_TUN => Box::new(tun_mode::TunInbound::new(
            tag,
            ib.raw.clone(),
            dialer.clone(),
            ctx.dns.clone(),
        )?),
        TYPE_REDIRECT => Box::new(tun_mode::RedirectInbound::new(
            tag,
            ib.raw.clone(),
            dialer.clone(),
            ctx.dns.clone(),
        )?),
        TYPE_TPROXY => Box::new(tun_mode::TproxyInbound::new(
            tag,
            ib.raw.clone(),
            dialer.clone(),
            ctx.dns.clone(),
        )?),
        TYPE_HYSTERIA => Box::new(legacy_inbound::HysteriaInbound::new(tag, ib.raw.clone())?),
        TYPE_SHADOWTLS => Box::new(legacy_inbound::ShadowTlsInbound::new(tag, ib.raw.clone())?),
        TYPE_ANYTLS => Box::new(legacy_inbound::AnyTlsInbound::new(tag, ib.raw.clone())?),
        TYPE_NAIVE => Box::new(legacy_inbound::NaiveInbound::new(tag, ib.raw.clone())?),
        TYPE_DNS => Box::new(dns_inbound::DnsInbound::new(
            tag,
            ib.raw.clone(),
            ctx.dns.clone(),
        )?),
        other => bail!("unknown inbound type: {other}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_types_lists_are_non_empty() {
        assert!(!ALL_INBOUND_TYPES.is_empty());
        assert!(!ALL_OUTBOUND_TYPES.is_empty());
        assert!(!ALL_SERVICE_TYPES.is_empty());
        assert!(is_known_outbound("direct"));
        assert!(is_known_outbound("urltest"));
        assert!(is_known_inbound("mixed"));
        assert!(is_known_service("api"));
        assert!(is_known_service("derp"));
        assert!(is_known_endpoint("tailscale"));
        assert!(!is_known_inbound("not-a-protocol"));
    }
}
