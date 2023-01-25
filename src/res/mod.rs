mod external_port;
mod interface;
mod node;
mod service;

use std::any::TypeId;

use enum_dispatch::enum_dispatch;
use kube::ResourceExt;
use thiserror::Error;
pub use external_port::*;
pub use interface::*;
pub use node::*;
pub use service::*;

use crate::{
    CoreNode, CoreService, ALLOW_RANGE_ANNOTATION, INTERNAL_ANNOTATION,
    NODE_EXCLUDE_ANNOTATION, NODE_EXCLUDE_LABEL,
};

#[enum_dispatch]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Resource {
    Interface,
    Node,
    Service,
}

#[enum_dispatch(Resource)]
pub trait ResourceLike {
    fn id(&self) -> String;
    fn type_id(&self) -> TypeId;
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
}

impl TryFrom<CoreService> for Resource {
    type Error = Error;

    fn try_from(cs: CoreService) -> Result<Self, Self::Error> {
        Ok(Service {
            external_port: cs.clone().try_into()?,
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
