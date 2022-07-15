use anyhow::{anyhow, Context};
use k8s_openapi::api::core::v1::{Node, Service as CoreService};
use kube::{api::ListParams, Api, Client};
use sha256::digest;
use std::str::FromStr;
use tracing::error;

#[derive(Debug, Copy, Clone)]
pub struct ExternalPort {
    pub host_port: u16,
    pub node_port: u16,
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
            _ => Err(anyhow::Error::msg("nope")),
        }
    }
}

#[derive(Debug)]
pub struct Service {
    pub iface: Option<String>,
    pub name: String,
    pub namespace: String,
}

impl Service {
    pub fn id(&self) -> String {
        let hash = digest(format!(
            "{}::{}::{}",
            self.iface.as_deref().unwrap_or(""),
            self.namespace,
            self.name
        ));
        format!("pf-{}-{}-{}", self.namespace, self.name, hash)
    }

    pub fn with_iface(&self, iface: &str) -> Self {
        Self {
            iface: Some(iface.to_owned()),
            name: self.name.to_owned(),
            namespace: self.namespace.to_owned(),
        }
    }
}

impl From<&CoreService> for Service {
    fn from(cs: &CoreService) -> Self {
        let metadata = cs.metadata.clone();
        let (name, namespace) = (metadata.name.unwrap(), metadata.namespace.unwrap());
        Service {
            iface: None,
            name,
            namespace,
        }
    }
}

#[derive(Debug)]
pub struct ServiceExternalPort {
    pub external_port: ExternalPort,
    pub service: Service,
}

impl ServiceExternalPort {
    pub fn id(&self) -> String {
        let hash = digest(format!(
            "{}:{}",
            self.external_port.host_port, self.external_port.node_port
        ));
        format!("{}-{}", self.service.id(), hash)
    }
}

pub async fn first_node_address(client: Client) -> anyhow::Result<String> {
    let nodes: Api<Node> = Api::all(client);
    let n = nodes.list(&ListParams::default()).await?.items;

    // TODO - load balancing
    if n.is_empty() {
        let err = "could not find any active nodes; bailing...";
        error!(err);
        return Err(anyhow!(err));
    }
    assert!(!n.is_empty());
    node_ip(&n[0])
}

fn node_ip(n: &Node) -> anyhow::Result<String> {
    for add in n
        .status
        .clone()
        .context("node missing status")?
        .addresses
        .context("node missing addresses")?
    {
        if add.type_ == "InternalIP" {
            return Ok(add.address);
        }
    }
    Err(anyhow!("failed to extract node ip"))
}
