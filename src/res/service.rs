use std::any::TypeId;

use super::ExternalPorts;
use crate::ResourceLike;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Service {
    pub name: String,
    pub namespace: String,
    pub external_ports: ExternalPorts,
    pub is_internal: bool,
    pub allow_range: Option<String>,
}

impl ResourceLike for Service {
    fn id(&self) -> String { self.fqn() }

    fn type_id(&self) -> TypeId { TypeId::of::<Service>() }

    fn is_active(&self) -> bool { self.has_external_ports() }
}

impl Service {
    pub fn fqn(&self) -> String { format!("{}/{}", self.namespace, self.name) }

    pub fn has_external_ports(&self) -> bool {
        !self.external_ports.specs.is_empty()
    }

    pub fn internal(self) -> Self { Self { is_internal: true, ..self } }
}
