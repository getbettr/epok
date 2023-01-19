use std::any::TypeId;

use anyhow::anyhow;

use super::ExternalPort;
use crate::ResourceLike;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Service {
    pub name: String,
    pub namespace: String,
    pub external_port: ExternalPort,
    pub is_internal: bool,
}

impl ResourceLike for Service {
    fn id(&self) -> String { self.fqn() }

    fn type_id(&self) -> TypeId { TypeId::of::<Service>() }

    fn is_active(&self) -> bool { self.has_external_port() }
}

impl Service {
    pub fn fqn(&self) -> String { format!("{}/{}", self.namespace, self.name) }

    pub fn has_external_port(&self) -> bool {
        !matches!(self.external_port, ExternalPort::Absent)
    }

    pub fn get_ports(&self) -> Result<(u16, u16), anyhow::Error> {
        match self.external_port {
            ExternalPort::Spec { host_port, node_port } => {
                Ok((host_port, node_port))
            }
            ExternalPort::Absent => Err(anyhow!("invalid service")),
        }
    }

    pub fn internal(self) -> Self { Self { is_internal: true, ..self } }
}
