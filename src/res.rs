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

    pub fn has_external_port(&self) -> bool {
        !matches!(self.external_port, ExternalPort::Absent)
    }

    pub fn get_ports(&self) -> Result<(u16, u16), anyhow::Error> {
        match self.external_port {
            ExternalPort::Spec {
                host_port,
                node_port,
            } => Ok((host_port, node_port)),
            ExternalPort::Absent => Err(anyhow!("invalid service")),
        }
    }
}

impl TryFrom<&CoreService> for Service {
    type Error = anyhow::Error;

    fn try_from(cs: &CoreService) -> Result<Self, Self::Error> {
        Ok(Service {
            external_port: ExternalPort::try_from(cs)?,
            name: cs.name_any(),
            namespace: cs.namespace().unwrap_or_default(),
        })
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExternalPort {
    Spec { host_port: u16, node_port: u16 },
    Absent,
}

impl TryFrom<&CoreService> for ExternalPort {
    type Error = anyhow::Error;

    fn try_from(cs: &CoreService) -> Result<Self, Self::Error> {
        let anno = cs.annotations();
        if anno.contains_key(ANNOTATION) {
            return ExternalPort::from_str(&anno[ANNOTATION]);
        }
        Ok(ExternalPort::Absent)
    }
}

impl FromStr for ExternalPort {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts = s.split(':').collect::<Vec<_>>();
        match parts.len() {
            2 => Ok(Self::Spec {
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
    pub is_ready: bool,
}

impl TryFrom<&CoreNode> for Node {
    type Error = anyhow::Error;

    fn try_from(cn: &CoreNode) -> Result<Self, Self::Error> {
        let status = cn.status.clone().context("node missing status")?;
        let meta = cn.metadata.clone();
        let anno = meta.annotations.unwrap_or_default();
        let labels = meta.labels.unwrap_or_default();

        for add in status.addresses.context("node missing addresses")? {
            if add.type_ == "InternalIP" {
                let is_ready = status
                    .conditions
                    .context("node missing conditions")?
                    .iter()
                    .any(|r| r.type_ == "Ready" && r.status == "True");
                let is_excluded = anno.contains_key(NODE_EXCLUDE_ANNOTATION)
                    || labels.contains_key(NODE_EXCLUDE_LABEL);
                return Ok(Self {
                    name: cn.name_any(),
                    addr: add.address,
                    is_ready: is_ready && !is_excluded,
                });
            }
        }
        Err(anyhow!("failed to extract node ip"))
    }
}
