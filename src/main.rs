mod backend;
mod logz;

use clap::{Args, Parser};
use futures::StreamExt;
use k8s_openapi::api::core::v1::{Node, Service as CoreService};
use kube::{
    api::ListParams,
    runtime::{watcher, watcher::Event},
    Api, Client,
};
use sha256::digest;
use tracing::{debug, error, info};

use anyhow::{anyhow, Context};
use std::{fmt::Debug, str::FromStr};

use crate::backend::{Backend, IptablesBackend};

pub const APP_NAME: &str = "epok";
pub const AUTHOR: &str = "Rareș Cosma - rares@getbetter.ro";
pub const ANNOTATION: &str = "getbetter.ro/externalport";

mod built_info {
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

#[derive(Parser, Debug)]
#[clap(about = built_info::PKG_DESCRIPTION, author = AUTHOR)]
pub struct Opts {
    /// Interface to forward packets from
    #[clap(long, short = 'i', value_parser, env = "EPOK_INTERFACE")]
    pub interface: String,

    #[clap(subcommand)]
    pub executor: Executor,
}

#[derive(clap::Parser, Debug)]
#[clap(long_about = "En taro Adun")]
pub enum Executor<Ssh: Args = backend::SshHost> {
    /// Run operator on bare metal host
    Local,
    /// Run operator inside cluster, SSH-ing back to the metal
    Ssh(Ssh),
}

#[derive(Debug)]
struct ExternalPort {
    host_port: u16,
    node_port: u16,
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
    iface: Option<String>,
    name: String,
    namespace: String,
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

    pub fn with_iface(self, iface: &str) -> Self {
        Self {
            iface: Some(iface.to_owned()),
            ..self
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
    service: Service,
    external_port: ExternalPort,
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

struct Operator<B>
where
    B: Backend,
{
    backend: B,
    interface: String,
}

impl<B> Operator<B>
where
    B: Backend,
{
    pub fn apply(&mut self, s: &CoreService) -> anyhow::Result<()> {
        let svc = Service::from(s).with_iface(&self.interface);

        if let Some(anno) = s.metadata.clone().annotations {
            if anno.contains_key(ANNOTATION) {
                info!("service changed: {:?}", &svc);
                if let Ok(ep) = ExternalPort::from_str(&anno[ANNOTATION]) {
                    let sep = ServiceExternalPort {
                        external_port: ep,
                        service: svc,
                    };
                    self.backend.upsert(&sep)?;
                } else {
                    error!(
                        "invalid annotation format '{}' for {:?}",
                        &anno[ANNOTATION], &svc
                    );
                }
            } else {
                // extraneous delete, but better safe than sorry
                debug!("missing annotation for: {:?} -> delete", &svc);
                self.backend.delete(&svc)?;
            }
        } else {
            debug!("ignoring {:?} {reason}", &svc, reason = "no annotation");
        }
        Ok(())
    }

    pub fn delete(&mut self, s: &CoreService) -> anyhow::Result<()> {
        let svc = Service::from(s).with_iface(&self.interface);
        info!("service deleted: {:?}", &svc);
        self.backend.delete(&svc)?;
        Ok(())
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    logz::initialize_logging("EPOK_LOG_LEVEL");
    logz::print_startup_string(
        built_info::PKG_DESCRIPTION,
        built_info::PKG_VERSION,
        built_info::GIT_VERSION,
        built_info::TARGET,
        built_info::BUILT_TIME_UTC,
        built_info::RUSTC_VERSION,
    );

    let opts = Opts::parse();
    debug!("parsed options: {:?}", opts);
    let kubeclient = Client::try_default().await?;

    let nodes: Api<Node> = Api::all(kubeclient.clone());
    let n = nodes.list(&ListParams::default()).await?.items;

    // TODO - load balancing
    if n.is_empty() {
        let err = "could not find any active nodes; bailing...";
        error!(err);
        return Err(anyhow!(err));
    }
    assert!(!n.is_empty());
    let first_address = node_ip(&n[0])?;
    info!(
        "forwarding from interface '{}' to ip '{}' of node '{}'",
        &opts.interface,
        &first_address,
        n[0].metadata.clone().name.unwrap(),
    );

    let watcher = watcher(Api::<CoreService>::all(kubeclient), ListParams::default());

    let mut operator = Operator {
        backend: IptablesBackend::new(&opts.interface, &first_address, opts.executor),
        interface: opts.interface,
    };

    watcher
        .map(|event| {
            if let Err(e) = match event.unwrap() {
                Event::Applied(obj) => operator.apply(&obj),
                Event::Restarted(obj) => obj.iter().try_for_each(|o| operator.apply(o)),
                Event::Deleted(obj) => operator.delete(&obj),
            } {
                error!("error while processing event: {:?}", e)
            }
        })
        .collect::<Vec<_>>()
        .await;

    Ok(())
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
