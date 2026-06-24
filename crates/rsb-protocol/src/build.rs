pub use crate::build_context::BuildContext;
use crate::registry::{
    build_inbound as registry_build_inbound, build_outbound as registry_build_outbound,
};
use rsb_config::Options;
use rsb_core::{Dialer, SharedOutboundManager};
use std::sync::Arc;

pub use crate::group::{OutboundController, SelectorControl};

pub fn build_outbounds(
    options: &Options,
    ctx: BuildContext,
    shared: Arc<SharedOutboundManager>,
    controller: &OutboundController,
) -> anyhow::Result<Vec<Box<dyn rsb_core::Outbound>>> {
    let mut outbounds = Vec::new();
    for (i, ob) in options.outbounds.iter().enumerate() {
        let tag = options.outbound_tag(ob, i);
        outbounds.push(registry_build_outbound(
            ob,
            tag,
            &ctx,
            shared.clone(),
            controller,
        )?);
    }
    if outbounds.is_empty() {
        anyhow::bail!("no outbounds configured");
    }
    Ok(outbounds)
}

pub fn build_inbounds(
    options: &Options,
    ctx: BuildContext,
    dialer: Arc<Dialer>,
) -> anyhow::Result<Vec<Box<dyn rsb_core::Inbound>>> {
    let mut inbounds = Vec::new();
    for (i, ib) in options.inbounds.iter().enumerate() {
        let tag = options.inbound_tag(ib, i);
        inbounds.push(registry_build_inbound(ib, tag, &ctx, dialer.clone())?);
    }
    Ok(inbounds)
}
