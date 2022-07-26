mod external_port;
mod interface;
mod node;
mod service;

use std::any::TypeId;

use enum_dispatch::enum_dispatch;
use kube::ResourceExt;

pub use external_port::*;
pub use interface::*;
pub use node::*;
pub use service::*;

use crate::{CoreNode, CoreService, NODE_EXCLUDE_ANNOTATION, NODE_EXCLUDE_LABEL};

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

impl TryFrom<CoreService> for Resource {
    type Error = anyhow::Error;

    fn try_from(cs: CoreService) -> Result<Self, Self::Error> {
        Ok(Service {
            external_port: cs.clone().try_into()?,
            name: cs.name_any(),
            namespace: cs.namespace().unwrap_or_default(),
        }
        .into())
    }
}

impl TryFrom<CoreNode> for Resource {
    type Error = anyhow::Error;

    fn try_from(cn: CoreNode) -> Result<Self, Self::Error> {
        let status = cn.status.clone().unwrap_or_default();
        let addr = node_ip(status.clone())?;
        let is_active = node_ready(status)
            && !cn.annotations().contains_key(NODE_EXCLUDE_ANNOTATION)
            && !cn.labels().contains_key(NODE_EXCLUDE_LABEL);

        Ok(Node {
            name: cn.name_any(),
            addr,
            is_active,
        }
        .into())
    }
}
