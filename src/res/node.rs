use std::any::TypeId;

use anyhow::{anyhow, Context};
use k8s_openapi::api::core::v1::NodeStatus;

use crate::ResourceLike;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Node {
    pub name: String,
    pub addr: String,
    pub is_active: bool,
}

impl ResourceLike for Node {
    fn id(&self) -> String {
        self.name.to_owned()
    }

    fn type_id(&self) -> TypeId {
        TypeId::of::<Node>()
    }

    fn is_active(&self) -> bool {
        self.is_active
    }
}

pub fn node_ip(status: NodeStatus) -> anyhow::Result<String> {
    for add in status.addresses.context("node missing addresses")? {
        if add.type_ == "InternalIP" {
            return Ok(add.address);
        }
    }
    Err(anyhow!("failed to extract node ip"))
}

pub fn node_ready(status: NodeStatus) -> bool {
    status
        .conditions
        .unwrap_or_default()
        .iter()
        .any(|c| c.type_ == "Ready" && c.status == "True")
}
