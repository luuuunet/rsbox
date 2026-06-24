use rsb_config::Options;
use rsb_dns::{register_resolved_service, DnsRouter};
use std::sync::Arc;

#[derive(Clone)]
pub struct BuildContext {
    pub dns: Arc<DnsRouter>,
    pub default_outbound_tag: String,
    pub bind_interface: Option<String>,
}

impl BuildContext {
    pub fn from_options(options: &Options) -> anyhow::Result<Self> {
        let route = options.route.clone().unwrap_or_default();
        let bind_interface = if route.auto_detect_interface {
            rsb_core::detect_default_interface().ok()
        } else {
            route.default_interface.clone()
        };
        if let Some(ref iface) = bind_interface {
            tracing::info!(interface = %iface, "route bind interface");
        }
        let dns = Arc::new(DnsRouter::new(options.dns.clone()));
        register_resolved_service("local", dns.clone());
        Ok(Self {
            dns,
            default_outbound_tag: options.default_outbound_tag()?,
            bind_interface,
        })
    }
}
