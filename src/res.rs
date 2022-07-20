use std::str::FromStr;

use anyhow::{anyhow, Context};
use k8s_openapi::api::core::v1::Node as CoreNode;
use kube::ResourceExt;

use crate::*;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Service {
    pub name: String,
    pub namespace: String,
    pub external_port: ExternalPort,
}

impl Service {
    pub fn fqn(&self) -> String {
        format!("{}/{}", self.namespace, self.name)
    }
}

impl TryFrom<&CoreService> for Service {
    type Error = anyhow::Error;

    fn try_from(cs: &CoreService) -> Result<Self, Self::Error> {
        let metadata = cs.metadata.clone();
        let (name, namespace) = (metadata.name.unwrap(), metadata.namespace.unwrap());
        Ok(Service {
            external_port: ExternalPort::try_from(cs)?,
            name,
            namespace,
        })
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ExternalPort {
    pub host_port: u16,
    pub node_port: u16,
}

impl TryFrom<&CoreService> for ExternalPort {
    type Error = anyhow::Error;

    fn try_from(cs: &CoreService) -> Result<Self, Self::Error> {
        if let Some(anno) = cs.metadata.clone().annotations {
            if anno.contains_key(ANNOTATION) {
                return ExternalPort::from_str(&anno[ANNOTATION]);
            }
        }
        Err(anyhow!("missing annotation"))
    }
}

impl FromStr for ExternalPort {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts = s.split(':').collect::<Vec<_>>();
        match parts.len() {
            2 => Ok(Self {
                host_port: parts[0].parse()?,
                node_port: parts[1].parse()?,
            }),
            _ => Err(anyhow!("failed to parse annotation")),
        }
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone)]
pub struct Node {
    pub name: String,
    pub addr: String,
}

impl TryFrom<&CoreNode> for Node {
    type Error = anyhow::Error;

    fn try_from(cn: &CoreNode) -> Result<Self, Self::Error> {
        for add in cn
            .status
            .clone()
            .context("node missing status")?
            .addresses
            .context("node missing addresses")?
        {
            if add.type_ == "InternalIP" {
                return Ok(Self {
                    name: cn.name_any(),
                    addr: add.address,
                });
            }
        }
        Err(anyhow!("failed to extract node ip"))
    }
}
