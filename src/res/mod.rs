mod external_ports;
mod interface;
mod node;
mod pod;
mod service;

use std::any::TypeId;

use enum_dispatch::enum_dispatch;
use kube::ResourceExt;
use thiserror::Error;
pub use external_ports::*;
pub use interface::*;
pub use node::*;
pub use service::*;
pub use pod::*;

use crate::{
    CoreNode, CorePod, CoreService, ALLOW_RANGE_ANNOTATION,
    INTERNAL_ANNOTATION, NODE_EXCLUDE_ANNOTATION, NODE_EXCLUDE_LABEL,
};

#[enum_dispatch]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Resource {
    Interface,
    Node,
    Service,
    Pod,
}

#[enum_dispatch(Resource)]
pub trait ResourceLike {
    fn id(&self) -> String;
    fn type_id(&self) -> TypeId
    where
        Self: 'static,
    {
        TypeId::of::<Self>()
    }
    fn is_active(&self) -> bool;
}

#[derive(Debug, Error)]
pub enum Error {
    #[error(
        "invalid service (error: {inner}, annotation: {annotation}, service_id: {service_id})"
    )]
    ServiceParseError {
        #[source]
        inner: anyhow::Error,
        annotation: String,
        service_id: String,
    },
    #[error("invalid node (error: {inner}, node_id: {node_id})")]
    NodeParseError {
        #[source]
        inner: anyhow::Error,
        node_id: String,
    },
    #[error("skipping pod (reason: {inner}, pod_id: {pod_id})")]
    SkipPod {
        #[source]
        inner: anyhow::Error,
        pod_id: String,
    },
}

impl TryFrom<CoreService> for Resource {
    type Error = Error;

    fn try_from(cs: CoreService) -> Result<Self, Self::Error> {
        Ok(Service {
            external_ports: cs.clone().try_into()?,
            name: cs.name_any(),
            namespace: cs.namespace().unwrap_or_default(),
            is_internal: cs.annotations().contains_key(INTERNAL_ANNOTATION),
            allow_range: cs
                .annotations()
                .get(ALLOW_RANGE_ANNOTATION)
                .map(String::to_owned),
        }
        .into())
    }
}

impl TryFrom<CoreNode> for Resource {
    type Error = Error;

    fn try_from(cn: CoreNode) -> Result<Self, Self::Error> {
        let status = cn.status.clone().unwrap_or_default();
        let addr = node_ip(status.clone()).map_err(|e| {
            Error::NodeParseError { inner: e, node_id: cn.name_any() }
        })?;
        let is_active = node_ready(status)
            && !cn.annotations().contains_key(NODE_EXCLUDE_ANNOTATION)
            && !cn.labels().contains_key(NODE_EXCLUDE_LABEL);

        Ok(Node { name: cn.name_any(), addr, is_active }.into())
    }
}

impl TryFrom<CorePod> for Resource {
    type Error = Error;

    fn try_from(cp: CorePod) -> Result<Self, Self::Error> {
        let status = cp.status.clone().unwrap_or_default();
        let addr = pod_ip(status.clone())
            .map_err(|e| Error::SkipPod { inner: e, pod_id: cp.name_any() })?;
        let is_active = pod_ready(status);

        Ok(Pod {
            name: cp.name_any(),
            namespace: cp.namespace().unwrap_or_default(),
            external_ports: cp.annotations().try_into()?,
            addr,
            is_ready: is_active,
        }
        .into())
    }
}
