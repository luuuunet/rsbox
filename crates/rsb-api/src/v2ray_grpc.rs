//! v2ray-compatible gRPC StatsService.

pub mod pb {
    tonic::include_proto!("v2ray.core.app.stats.command");
}

use pb::stats_service_server::{StatsService, StatsServiceServer};
use pb::{GetStatsRequest, GetStatsResponse, QueryStatsRequest, QueryStatsResponse, Stat};
use rsb_core::SharedConnectionManager;
use std::net::SocketAddr;
use tonic::{transport::Server, Request, Response, Status};

#[derive(Clone)]
pub struct V2RayStatsGrpc {
    connections: SharedConnectionManager,
}

impl V2RayStatsGrpc {
    pub fn new(connections: SharedConnectionManager) -> Self {
        Self { connections }
    }

    pub async fn serve(self, listen: SocketAddr) -> Result<(), tonic::transport::Error> {
        Server::builder()
            .add_service(StatsServiceServer::new(self))
            .serve(listen)
            .await
    }
}

#[tonic::async_trait]
impl StatsService for V2RayStatsGrpc {
    async fn get_stats(
        &self,
        req: Request<GetStatsRequest>,
    ) -> Result<Response<GetStatsResponse>, Status> {
        let r = req.into_inner();
        let value = self
            .connections
            .query_v2ray_stats(&r.name, r.reset)
            .into_iter()
            .find(|(name, _)| name == &r.name)
            .map(|(_, v)| v)
            .unwrap_or(0);
        Ok(Response::new(GetStatsResponse {
            stat: Some(Stat {
                name: r.name,
                value: i64::try_from(value).unwrap_or(i64::MAX),
            }),
        }))
    }

    async fn query_stats(
        &self,
        req: Request<QueryStatsRequest>,
    ) -> Result<Response<QueryStatsResponse>, Status> {
        let r = req.into_inner();
        let stat = self
            .connections
            .query_v2ray_stats(&r.pattern, r.reset)
            .into_iter()
            .map(|(name, value)| Stat {
                name,
                value: i64::try_from(value).unwrap_or(i64::MAX),
            })
            .collect();
        Ok(Response::new(QueryStatsResponse { stat }))
    }
}

pub async fn spawn_v2ray_stats_grpc(
    connections: SharedConnectionManager,
    listen: SocketAddr,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    let api = V2RayStatsGrpc::new(connections);
    tokio::select! {
        _ = shutdown.changed() => {}
        r = api.serve(listen) => { let _ = r; }
    }
}
