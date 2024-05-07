use sha256::digest;
use itertools::Itertools;

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
    fn is_active(&self) -> bool { self.has_external_ports() }
}

impl Service {
    pub fn fqn(&self) -> String { format!("{}/{}", self.namespace, self.name) }

    pub fn has_external_ports(&self) -> bool {
        !self.external_ports.specs.is_empty()
    }

    pub fn internal(self) -> Self { Self { is_internal: true, ..self } }

    pub fn service_hash(&self) -> String {
        let mut fqn_hash = digest(self.fqn());
        fqn_hash.truncate(16);
        let port_hash = &self.external_ports.specs.iter().join("::");
        let mut service_hash = digest(format!(
            "{fqn_hash}{port_hash}{}{}",
            self.is_internal,
            self.allow_range.to_owned().unwrap_or_else(|| "".into())
        ));
        service_hash.truncate(16);
        service_hash
    }
}
