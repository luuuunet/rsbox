use crate::build::BuildContext;
use crate::endpoints::{build_endpoints, EndpointHandle};
use crate::reload::ConfigReload;
use crate::services::{build_services, ServiceContext, ServiceHandle};
use crate::{build_inbounds, build_outbounds, OutboundController};
use anyhow::{Context, Result};
use rsb_config::Options;
use rsb_core::{
    ConnectionManager, Dialer, InboundManager, OutboundManager, Router, SharedConnectionManager,
    SharedOutboundManager, UserRegistry,
};
use rsb_dns::unregister_resolved_service;
use rsb_route::RuleRouter;
use std::sync::{Arc, RwLock};
use tracing::info;

pub struct RsBox {
    options: Arc<RwLock<Options>>,
    inbound: Arc<tokio::sync::Mutex<InboundManager>>,
    dialer: Arc<Dialer>,
    outbound: Arc<OutboundManager>,
    router: Arc<dyn Router>,
    controller: Arc<OutboundController>,
    endpoints: Vec<EndpointHandle>,
    services: Vec<ServiceHandle>,
    connections: SharedConnectionManager,
    reload: Arc<ConfigReload>,
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
        let connections: SharedConnectionManager = Arc::new(ConnectionManager::with_registry(
            Arc::new(UserRegistry::from_options(&options)),
        ));
        let dialer = Arc::new(Dialer::new(
            outbound.clone(),
            router.clone(),
            connections.clone(),
        ));
        let inbound = Arc::new(tokio::sync::Mutex::new(InboundManager::new(
            build_inbounds(&options, ctx.clone(), dialer.clone())?,
        )));
        let options = Arc::new(RwLock::new(options));
        let reload = Arc::new(ConfigReload::new(
            inbound.clone(),
            dialer.clone(),
            connections.clone(),
            options.clone(),
            ctx.dns.clone(),
        ));
        let opts_snapshot = options
            .read()
            .map(|o| o.clone())
            .unwrap_or_else(|e| e.into_inner().clone());
        let endpoints = build_endpoints(&opts_snapshot)?;
        let service_ctx = ServiceContext {
            options: options.clone(),
            controller: controller.clone(),
            connections: connections.clone(),
            dns: ctx.dns.clone(),
            reload: Some(reload.clone()),
        };
        let services = build_services(&opts_snapshot, service_ctx)?;

        Ok(Self {
            options,
            inbound,
            dialer,
            outbound,
            router,
            controller,
            endpoints,
            services,
            connections,
            reload,
        })
    }

    pub async fn start(&self) -> Result<()> {
        let opts = self.options.read().map_err(|e| anyhow::anyhow!(e.to_string()))?;
        info!(
            inbounds = self.inbound.lock().await.inbounds().len(),
            endpoints = self.endpoints.len(),
            services = self.services.len(),
            "rsbox starting"
        );
        drop(opts);
        for svc in &self.services {
            svc.start().await.context("start service")?;
        }
        for ep in &self.endpoints {
            ep.start().await.context("start endpoint")?;
        }
        self.inbound
            .lock()
            .await
            .start_all()
            .await
            .context("start inbounds")?;
        info!("rsbox started");
        Ok(())
    }

    pub async fn close(&self) -> Result<()> {
        self.inbound.lock().await.close_all().await.ok();
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

    pub fn options(&self) -> Options {
        self.options
            .read()
            .map(|o| o.clone())
            .unwrap_or_else(|e| e.into_inner().clone())
    }

    pub fn controller(&self) -> Arc<OutboundController> {
        self.controller.clone()
    }

    pub fn connections(&self) -> SharedConnectionManager {
        self.connections.clone()
    }

    pub fn reload_handle(&self) -> Arc<ConfigReload> {
        self.reload.clone()
    }

    pub fn reload_users(&self) {
        self.reload.reload_users();
    }

    pub async fn reload_config(&self, options: Options) -> Result<()> {
        self.reload.reload(options).await
    }
}
