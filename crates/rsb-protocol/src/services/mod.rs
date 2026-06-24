mod api;
mod api_grpc;
mod context;
mod derp;
mod hysteria_realm;
mod listen;
mod multiplexer;
mod registry;
mod resolved;
mod ssm_api;
mod tls_util;
mod usbip;

use anyhow::Result;
use api::ApiService;
use derp::DerpService;
use hysteria_realm::HysteriaRealmService;
use multiplexer::MultiplexerService;
use resolved::ResolvedService;
use serde_json::Value;
use ssm_api::SsmApiService;
use usbip::{UsbipClientService, UsbipServerService};

pub use context::ServiceContext;
pub use registry::build_service;

pub struct ServiceHandle {
    pub(crate) tag: String,
    pub(crate) kind: String,
    pub(crate) inner: ServiceInner,
}

pub(crate) enum ServiceInner {
    Generic(GenericService),
    Api(ApiService),
    Derp(DerpService),
    Ccm(MultiplexerService),
    Ocm(MultiplexerService),
    Resolved(ResolvedService),
    SsmApi(SsmApiService),
    HysteriaRealm(HysteriaRealmService),
    UsbipServer(UsbipServerService),
    UsbipClient(UsbipClientService),
}

pub(crate) struct GenericService {
    tag: String,
    kind: String,
    shutdown: tokio::sync::watch::Sender<bool>,
    handle: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl GenericService {
    pub(crate) fn new(tag: String, kind: String) -> Self {
        let (shutdown, _) = tokio::sync::watch::channel(false);
        Self {
            tag,
            kind,
            shutdown,
            handle: tokio::sync::Mutex::new(None),
        }
    }

    async fn start(&self) -> Result<()> {
        tracing::info!(tag = %self.tag, kind = %self.kind, "service started (generic)");
        let tag = self.tag.clone();
        let kind = self.kind.clone();
        let mut shutdown = self.shutdown.subscribe();
        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown.changed() => { if *shutdown.borrow() { break; } }
                    _ = tokio::time::sleep(std::time::Duration::from_secs(3600)) => {
                        tracing::trace!(tag = %tag, kind = %kind, "service heartbeat");
                    }
                }
            }
        });
        *self.handle.lock().await = Some(handle);
        Ok(())
    }

    async fn close(&self) -> Result<()> {
        let _ = self.shutdown.send(true);
        if let Some(h) = self.handle.lock().await.take() {
            h.abort();
        }
        Ok(())
    }
}

impl ServiceHandle {
    pub(crate) fn from_inner(tag: String, kind: String, inner: ServiceInner) -> Self {
        Self { tag, kind, inner }
    }

    pub async fn start(&self) -> Result<()> {
        match &self.inner {
            ServiceInner::Generic(s) => s.start().await,
            ServiceInner::Api(s) => s.start().await,
            ServiceInner::Derp(s) => s.start().await,
            ServiceInner::Ccm(s) => s.start().await,
            ServiceInner::Ocm(s) => s.start().await,
            ServiceInner::Resolved(s) => s.start().await,
            ServiceInner::SsmApi(s) => s.start().await,
            ServiceInner::HysteriaRealm(s) => s.start().await,
            ServiceInner::UsbipServer(s) => s.start().await,
            ServiceInner::UsbipClient(s) => s.start().await,
        }
    }

    pub async fn close(&self) -> Result<()> {
        match &self.inner {
            ServiceInner::Generic(s) => s.close().await,
            ServiceInner::Api(s) => s.close().await,
            ServiceInner::Derp(s) => s.close().await,
            ServiceInner::Ccm(s) => s.close().await,
            ServiceInner::Ocm(s) => s.close().await,
            ServiceInner::Resolved(s) => s.close().await,
            ServiceInner::SsmApi(s) => s.close().await,
            ServiceInner::HysteriaRealm(s) => s.close().await,
            ServiceInner::UsbipServer(s) => s.close().await,
            ServiceInner::UsbipClient(s) => s.close().await,
        }
    }
}

pub fn build_services(
    options: &rsb_config::Options,
    ctx: ServiceContext,
) -> Result<Vec<ServiceHandle>> {
    let mut out = Vec::new();
    for (i, svc) in options.services.iter().enumerate() {
        let tag = svc.tag.clone().unwrap_or_else(|| format!("service-{i}"));
        out.push(build_service(
            tag,
            svc.kind.clone(),
            svc.raw.clone(),
            ctx.clone(),
        )?);
    }
    Ok(out)
}
