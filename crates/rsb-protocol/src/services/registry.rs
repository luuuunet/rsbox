//! Service 类型注册 —— 由 `registry` 模块统一调度。

use super::{
    ApiService, DerpService, GenericService, HysteriaRealmService, MultiplexerService,
    ResolvedService, ServiceContext, ServiceHandle, ServiceInner, SsmApiService,
    UsbipClientService, UsbipServerService,
};
use anyhow::Result;
use serde_json::Value;

pub fn build_service(
    tag: String,
    kind: String,
    raw: Value,
    ctx: ServiceContext,
) -> Result<ServiceHandle> {
    use rsb_constant::*;
    let inner = match kind.as_str() {
        TYPE_SERVICE_API => ServiceInner::Api(ApiService::new(tag.clone(), raw, ctx)?),
        TYPE_SERVICE_DERP => ServiceInner::Derp(DerpService::new(tag.clone(), raw)?),
        TYPE_SERVICE_CCM => ServiceInner::Ccm(MultiplexerService::ccm(tag.clone(), raw)?),
        TYPE_SERVICE_OCM => ServiceInner::Ocm(MultiplexerService::ocm(tag.clone(), raw)?),
        TYPE_SERVICE_RESOLVED => {
            ServiceInner::Resolved(ResolvedService::new(tag.clone(), raw, ctx)?)
        }
        TYPE_SERVICE_SSM_API => ServiceInner::SsmApi(SsmApiService::new(tag.clone(), raw, ctx)?),
        TYPE_SERVICE_HYSTERIA_REALM => {
            ServiceInner::HysteriaRealm(HysteriaRealmService::new(tag.clone(), raw)?)
        }
        TYPE_SERVICE_USBIP_SERVER => {
            ServiceInner::UsbipServer(UsbipServerService::new(tag.clone(), raw)?)
        }
        TYPE_SERVICE_USBIP_CLIENT => {
            ServiceInner::UsbipClient(UsbipClientService::new(tag.clone(), raw)?)
        }
        _ => ServiceInner::Generic(GenericService::new(tag.clone(), kind.clone())),
    };
    Ok(ServiceHandle::from_inner(tag, kind, inner))
}
