//! Type registry — supported inbounds/outbounds for the slim rsbox build.

use crate::build_context::BuildContext;
use crate::group::OutboundController;
#[cfg(any(feature = "desktop", feature = "mobile"))]
use crate::tun_mode;
use crate::{
    direct, dns_inbound, dns_outbound, fallback, group, hysteria2, inbound_proxy, rsq, rst,
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
        TYPE_HYSTERIA2 => Box::new(hysteria2::Hysteria2Outbound::new(tag, ob.raw.clone())?),
        TYPE_RSQ => Box::new(rsq::RsqOutbound::new(tag, ob.raw.clone())?),
        TYPE_RST => Box::new(rst::RstOutbound::new(tag, ob.raw.clone())?),
        TYPE_DNS => Box::new(dns_outbound::DnsOutbound::new(
            tag,
            ob.raw.clone(),
            ctx.dns.clone(),
        )?),
        TYPE_SELECTOR => {
            let sel = group::SelectorOutbound::new(tag, ob.raw.clone(), shared.clone())?;
            controller.register_selector(sel.control());
            Box::new(sel)
        }
        TYPE_URLTEST => {
            let ut = group::UrlTestOutbound::new(tag, ob.raw.clone(), shared.clone())?;
            controller.register_urltest(ut.control());
            Box::new(ut)
        }
        TYPE_FALLBACK => {
            let fb = fallback::FallbackOutbound::new(tag, ob.raw.clone(), shared.clone())?;
            controller.register_fallback(fb.control());
            Box::new(fb)
        }
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
        TYPE_HYSTERIA2 => Box::new(hysteria2::Hysteria2Inbound::new(
            tag,
            ib.raw.clone(),
            dialer.connections(),
        )?),
        TYPE_RSQ => Box::new(rsq::RsqInbound::new(
            tag,
            ib.raw.clone(),
            dialer.connections(),
        )?),
        TYPE_RST => Box::new(rst::RstInbound::new(
            tag,
            ib.raw.clone(),
            dialer.connections(),
        )?),
        #[cfg(any(feature = "desktop", feature = "mobile"))]
        TYPE_TUN => Box::new(tun_mode::TunInbound::new(
            tag,
            ib.raw.clone(),
            dialer.clone(),
            ctx.dns.clone(),
        )?),
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
        assert!(is_known_outbound("direct"));
        assert!(is_known_outbound("rsq"));
        assert!(is_known_outbound("rst"));
        assert!(is_known_outbound("hysteria2"));
        assert!(is_known_outbound("fallback"));
        assert!(is_known_inbound("mixed"));
        assert!(is_known_inbound("rsq"));
        assert!(!is_known_inbound("shadowsocks"));
        assert!(!is_known_outbound("vless"));
        assert!(is_known_service("api"));
        assert!(!is_known_endpoint("wireguard"));
        assert!(!is_known_inbound("not-a-protocol"));
    }
}
