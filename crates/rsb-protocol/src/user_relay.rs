//! Helpers to start panel-aware relay sessions from protocol handlers.

use crate::inbound_proxy::UserRelaySession;
use anyhow::Result;
use rsb_core::{SharedConnectionManager, UserLimits};
use std::net::SocketAddr;
use uuid::Uuid;

pub fn begin_for_uuid(
    connections: &SharedConnectionManager,
    inbound_tag: &str,
    uuid: &Uuid,
    dest: Option<SocketAddr>,
    domain: Option<String>,
) -> Result<UserRelaySession> {
    let (name, limits) = connections
        .resolve_user(uuid)
        .map(|r| (r.name.clone(), r.limits.clone()))
        .unwrap_or_else(|| (uuid.to_string(), UserLimits::default()));
    UserRelaySession::begin(
        connections.clone(),
        inbound_tag,
        &name,
        limits,
        dest,
        domain,
    )
}

pub fn begin_for_trojan_hash(
    connections: &SharedConnectionManager,
    inbound_tag: &str,
    trojan_hash: &str,
    dest: Option<SocketAddr>,
    domain: Option<String>,
) -> Result<UserRelaySession> {
    let (name, limits) = connections
        .users()
        .lookup_trojan_hash(trojan_hash)
        .map(|r| (r.name.clone(), r.limits.clone()))
        .unwrap_or_else(|| {
            let short = trojan_hash.chars().take(8).collect::<String>();
            (short, UserLimits::default())
        });
    UserRelaySession::begin(
        connections.clone(),
        inbound_tag,
        &name,
        limits,
        dest,
        domain,
    )
}

pub fn begin_for_password(
    connections: &SharedConnectionManager,
    inbound_tag: &str,
    password: &str,
    dest: Option<SocketAddr>,
    domain: Option<String>,
) -> Result<UserRelaySession> {
    let (name, limits) = connections
        .users()
        .lookup_password(password)
        .map(|r| (r.name.clone(), r.limits.clone()))
        .unwrap_or_else(|| (password.chars().take(8).collect(), UserLimits::default()));
    UserRelaySession::begin(
        connections.clone(),
        inbound_tag,
        &name,
        limits,
        dest,
        domain,
    )
}

pub fn begin_for_inbound(
    connections: &SharedConnectionManager,
    inbound_tag: &str,
    dest: Option<SocketAddr>,
    domain: Option<String>,
) -> Result<Option<UserRelaySession>> {
    let Some(rec) = connections.users().first_for_inbound(inbound_tag) else {
        return Ok(None);
    };
    Ok(Some(UserRelaySession::begin(
        connections.clone(),
        inbound_tag,
        &rec.name,
        rec.limits.clone(),
        dest,
        domain,
    )?))
}
