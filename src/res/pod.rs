use anyhow::anyhow;
use itertools::Itertools;
use k8s_openapi::api::core::v1::PodStatus;
use sha256::digest;

use crate::{ExternalPorts, ResourceLike};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pod {
    pub name: String,
    pub namespace: String,
    pub addr: String,
    pub external_ports: ExternalPorts,
    pub is_internal: bool,
    pub is_external: bool,
    pub is_ready: bool,
}

impl ResourceLike for Pod {
    fn id(&self) -> String { self.name.to_owned() }
    fn is_active(&self) -> bool { self.is_ready && self.has_external_ports() }
}

pub fn pod_ip(status: PodStatus) -> anyhow::Result<String> {
    status.pod_ip.ok_or(anyhow!("missing pod ip"))
}

pub fn pod_ready(status: PodStatus) -> bool {
    status
        .conditions
        .unwrap_or_default()
        .iter()
        .any(|c| c.type_ == "Ready" && c.status == "True")
}

impl Pod {
    pub fn fqn(&self) -> String { format!("{}/{}", self.namespace, self.name) }

    pub fn has_external_ports(&self) -> bool {
        !self.external_ports.specs.is_empty()
    }

    pub fn pod_hash(&self) -> String {
        let mut pod_hash = digest(format!(
            "{}::{}",
            self.fqn(),
            self.external_ports.specs.iter().join("::")
        ));
        pod_hash.truncate(32);
        pod_hash
    }
}
