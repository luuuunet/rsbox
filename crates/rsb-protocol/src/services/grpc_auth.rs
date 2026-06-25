// gRPC 鉴权实现
use tonic::{Request, Status, metadata::MetadataValue};

pub struct GrpcAuthInterceptor {
    tokens: Vec<String>,
}

impl GrpcAuthInterceptor {
    pub fn new(tokens: Vec<String>) -> Self {
        Self { tokens }
    }

    pub fn check_auth<T>(&self, req: Request<T>) -> Result<Request<T>, Status> {
        // 如果没有配置 token，允许所有请求
        if self.tokens.is_empty() {
            tracing::warn!("gRPC API: No authentication tokens configured");
            return Ok(req);
        }

        // 检查 Authorization header
        let token = req
            .metadata()
            .get("authorization")
            .ok_or_else(|| {
                tracing::warn!("gRPC API: Missing authorization header");
                Status::unauthenticated("Missing authorization header")
            })?
            .to_str()
            .map_err(|_| {
                tracing::warn!("gRPC API: Invalid authorization header");
                Status::unauthenticated("Invalid authorization header")
            })?;

        // 验证 token
        let token = token.strip_prefix("Bearer ").unwrap_or(token);

        if self.tokens.iter().any(|t| t == token) {
            tracing::debug!("gRPC API: Authentication successful");
            Ok(req)
        } else {
            tracing::warn!(token = %token, "gRPC API: Invalid token");
            Err(Status::unauthenticated("Invalid token"))
        }
    }
}

// 使用示例：
//
// impl GrpcApi {
//     pub async fn serve_with_auth(
//         self,
//         listen: SocketAddr,
//         tokens: Vec<String>,
//     ) -> Result<(), tonic::transport::Error> {
//         let auth = GrpcAuthInterceptor::new(tokens);
//
//         Server::builder()
//             .add_service(
//                 ExperimentalServiceServer::with_interceptor(
//                     self.clone(),
//                     move |req| auth.check_auth(req),
//                 )
//             )
//             .serve(listen)
//             .await
//     }
// }
