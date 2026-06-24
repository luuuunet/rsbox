use crate::build::BuildContext;
use crate::endpoints::{build_endpoints, EndpointHandle};
use crate::services::{build_services, ServiceContext, ServiceHandle};
use crate::{build_inbounds, build_outbounds, OutboundController};
use anyhow::{Context, Result};
use rsb_config::Options;
use rsb_core::{
    ConnectionManager, Dialer, InboundManager, OutboundManager, Router, SharedConnectionManager,
    SharedOutboundManager,
};
use rsb_dns::unregister_resolved_service;
use rsb_route::RuleRouter;
use std::sync::Arc;
use tracing::info;

pub struct RsBox {
    options: Options,
    inbound: InboundManager,
    outbound: Arc<OutboundManager>,
    router: Arc<dyn Router>,
    controller: Arc<OutboundController>,
    endpoints: Vec<EndpointHandle>,
    services: Vec<ServiceHandle>,
    connections: SharedConnectionManager,
}

impl RsBox {
    pub async fn new(options: Options) -> Result<Self> {
        let ctx = BuildContext::from_options(&options)?;
        let default_tag = options.default_outbound_tag()?;

        let shared = Arc::new(SharedOutboundManager::new());
        let controller = Arc::new(OutboundController::new(shared.clone()));
        let outbounds = build_outbounds(&options, ctx.clone(), shared.clone(), &controller)?;
        let outbound = Arc::new(OutboundManager::new(outbounds, default_tag.clone())?);
        shared.set(outbound.clone());

        let mut router = RuleRouter::new(options.route.clone().unwrap_or_default(), default_tag);
        router.load_rule_sets().await?;
        let router: Arc<dyn Router> = Arc::new(router);
        let connections: SharedConnectionManager = Arc::new(ConnectionManager::new());
        let dialer = Arc::new(Dialer::new(
            outbound.clone(),
            router.clone(),
            connections.clone(),
        ));
        let inbound = InboundManager::new(build_inbounds(&options, ctx.clone(), dialer)?);
        let endpoints = build_endpoints(&options)?;
        let service_ctx = ServiceContext {
            options: Arc::new(options.clone()),
            controller: controller.clone(),
            connections: connections.clone(),
            dns: ctx.dns.clone(),
        };
        let services = build_services(&options, service_ctx)?;

        Ok(Self {
            options,
            inbound,
            outbound,
            router,
            controller,
            endpoints,
            services,
            connections,
        })
    }

    pub async fn start(&self) -> Result<()> {
        info!(
            inbounds = self.inbound.inbounds().len(),
            endpoints = self.endpoints.len(),
            services = self.services.len(),
            "rsbox starting"
        );
        for svc in &self.services {
            svc.start().await.context("start service")?;
        }
        for ep in &self.endpoints {
            ep.start().await.context("start endpoint")?;
        }
        self.inbound.start_all().await.context("start inbounds")?;
        info!("rsbox started");
        Ok(())
    }

    pub async fn close(&self) -> Result<()> {
        self.inbound.close_all().await.ok();
        self.outbound.close_all().await.ok();
        for ep in &self.endpoints {
            ep.close().await.ok();
        }
        for svc in &self.services {
            svc.close().await.ok();
        }
        unregister_resolved_service("local");
        Ok(())
    }

    pub fn options(&self) -> &Options {
        &self.options
    }

    pub fn controller(&self) -> Arc<OutboundController> {
        self.controller.clone()
    }

    pub fn connections(&self) -> SharedConnectionManager {
        self.connections.clone()
    }
}
