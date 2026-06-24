//! gRPC control plane (sing-box experimental.api compatible subset).

pub mod pb {
    tonic::include_proto!("rsbox.api");
}

use super::context::ServiceContext;
use pb::{
    experimental_service_server::{ExperimentalService, ExperimentalServiceServer},
    group_service_server::{GroupService, GroupServiceServer},
    outbound_service_server::{OutboundService, OutboundServiceServer},
    CloseConnectionRequest, Connection, ConnectionList, Empty, Outbound, OutboundList,
    SelectRequest, Stats, UrlTestRequest, UrlTestResult, Version,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tonic::{transport::Server, Request, Response, Status};

#[derive(Clone)]
pub struct GrpcApi {
    ctx: ServiceContext,
}

impl GrpcApi {
    pub fn new(ctx: ServiceContext) -> Self {
        Self { ctx }
    }

    pub async fn serve(self, listen: SocketAddr) -> Result<(), tonic::transport::Error> {
        Server::builder()
            .add_service(ExperimentalServiceServer::new(self.clone()))
            .add_service(GroupServiceServer::new(self.clone()))
            .add_service(OutboundServiceServer::new(self))
            .serve(listen)
            .await
    }
}

#[tonic::async_trait]
impl ExperimentalService for GrpcApi {
    async fn get_version(&self, _req: Request<Empty>) -> Result<Response<Version>, Status> {
        Ok(Response::new(Version {
            version: env!("CARGO_PKG_VERSION").into(),
            experimental: true,
        }))
    }

    async fn get_connections(
        &self,
        _req: Request<Empty>,
    ) -> Result<Response<ConnectionList>, Status> {
        let connections = self
            .ctx
            .connections
            .list()
            .into_iter()
            .map(|c| Connection {
                id: c.id,
                inbound: c.inbound_tag,
                outbound: c.outbound_tag,
                network: c.network,
                source: c.source.map(|a| a.to_string()).unwrap_or_default(),
                destination: c.destination.map(|a| a.to_string()).unwrap_or_default(),
                domain: c.domain.unwrap_or_default(),
                started_at: c.started_at,
            })
            .collect();
        Ok(Response::new(ConnectionList { connections }))
    }

    async fn close_connection(
        &self,
        req: Request<CloseConnectionRequest>,
    ) -> Result<Response<Empty>, Status> {
        self.ctx.connections.untrack(req.into_inner().id);
        Ok(Response::new(Empty {}))
    }

    async fn close_all_connections(&self, _req: Request<Empty>) -> Result<Response<Empty>, Status> {
        for c in self.ctx.connections.list() {
            self.ctx.connections.untrack(c.id);
        }
        Ok(Response::new(Empty {}))
    }

    async fn get_stats(&self, _req: Request<Empty>) -> Result<Response<Stats>, Status> {
        Ok(Response::new(Stats {
            connections: self.ctx.connections.list().len() as u64,
            outbounds: self.ctx.options.outbounds.len() as u64,
        }))
    }
}

#[tonic::async_trait]
impl GroupService for GrpcApi {
    async fn list(&self, _req: Request<Empty>) -> Result<Response<OutboundList>, Status> {
        Ok(Response::new(outbound_list(&self.ctx)))
    }

    async fn select(&self, req: Request<SelectRequest>) -> Result<Response<Empty>, Status> {
        let r = req.into_inner();
        self.ctx
            .controller
            .select(&r.group_tag, &r.outbound_tag)
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(Empty {}))
    }

    async fn url_test(
        &self,
        req: Request<UrlTestRequest>,
    ) -> Result<Response<UrlTestResult>, Status> {
        let group = req.into_inner().group_tag;
        let (selected, delays) = self
            .ctx
            .controller
            .run_url_test(&group)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(UrlTestResult {
            selected,
            delays: delays
                .into_iter()
                .map(|(tag, ms)| pb::OutboundDelay { tag, delay_ms: ms })
                .collect(),
        }))
    }
}

#[tonic::async_trait]
impl OutboundService for GrpcApi {
    async fn list(&self, _req: Request<Empty>) -> Result<Response<OutboundList>, Status> {
        Ok(Response::new(outbound_list(&self.ctx)))
    }
}

fn outbound_list(ctx: &ServiceContext) -> OutboundList {
    let outbounds = ctx
        .options
        .outbounds
        .iter()
        .enumerate()
        .map(|(i, ob)| Outbound {
            tag: ctx.options.outbound_tag(ob, i),
            r#type: ob.kind.clone(),
        })
        .collect();
    OutboundList { outbounds }
}

pub async fn spawn_grpc(
    ctx: ServiceContext,
    listen: SocketAddr,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    let api = GrpcApi::new(ctx);
    tokio::select! {
        _ = shutdown.changed() => {}
        r = api.serve(listen) => { let _ = r; }
    }
}
