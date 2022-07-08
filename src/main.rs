mod iptables;
mod logz;

use clap::{Args, Parser};
use futures::StreamExt;
use k8s_openapi::api::core::v1::Node;
use k8s_openapi::api::core::v1::Service as CoreService;
use kube::api::ListParams;
use kube::runtime::{watcher, watcher::Event};
use kube::{Api, Client};
use sha256::digest;
use tracing::{debug, error, info};

use std::fmt::Debug;
use std::str::FromStr;

use crate::iptables::{IptablesBackend, RealBackend};

pub const APP_NAME: &str = "k8s-op";
pub const AUTHOR: &str = "Rareș Cosma - rares@getbetter.ro";
pub const ANNOTATION: &str = "getbetter.ro/externalport";

mod built_info {
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

#[derive(Parser, Debug)]
#[clap(about = built_info::PKG_DESCRIPTION, author = AUTHOR)]
pub struct Opts {
    /// Interface to forward packets from
    #[clap(long, short = 'i', value_parser, env = "K8S_OP_INTERFACE")]
    pub interface: String,

    #[clap(subcommand)]
    pub executor: Executor,
}

#[derive(clap::Parser, Debug)]
#[clap(long_about = "En taro Adun")]
pub enum Executor<Ssh: Args = iptables::SshHost> {
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
    name: String,
    namespace: String,
}

impl Service {
    pub fn id(&self) -> String {
        let hash = digest(format!("{}::{}", self.namespace, self.name));
        format!("pf-{}-{}-{}", self.namespace, self.name, hash)
    }
}

impl From<&CoreService> for Service {
    fn from(cs: &CoreService) -> Self {
        let metadata = cs.metadata.clone();
        let (name, namespace) = (metadata.name.unwrap(), metadata.namespace.unwrap());
        Service { name, namespace }
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

pub fn apply<B: IptablesBackend>(s: &CoreService, backend: &mut B) -> anyhow::Result<()> {
    let svc = Service::from(s);

    if let Some(anno) = s.metadata.clone().annotations {
        if anno.contains_key(ANNOTATION) {
            info!("service changed: {:?}", &svc);
            if let Ok(ep) = ExternalPort::from_str(&anno[ANNOTATION]) {
                let sep = ServiceExternalPort {
                    external_port: ep,
                    service: svc,
                };
                backend.upsert(&sep)?;
            } else {
                error!(
                    "invalid annotation format '{}' for {:?}",
                    &anno[ANNOTATION], &svc
                );
            }
        } else {
            // extraneous delete, but better safe than sorry
            debug!("missing annotation for: {:?} -> delete", &svc);
            backend.delete(&svc)?;
        }
    } else {
        debug!("ignoring {:?} {reason}", &svc, reason = "no annotation");
    }
    Ok(())
}

pub fn delete<B: IptablesBackend>(s: &CoreService, backend: &mut B) -> anyhow::Result<()> {
    let svc = Service::from(s);
    info!("service deleted: {:?}", &svc);
    backend.delete(&svc)?;
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    logz::initialize_logging("K8S_OP_LOG_LEVEL");
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

    // TODO - error handling + load balancing
    assert!(!n.is_empty());
    let first_address = node_ip(&n[0]);
    info!(
        "forwarding from interface '{}' to ip '{}' of node '{}'",
        &opts.interface,
        &first_address,
        n[0].metadata.clone().name.unwrap(),
    );

    let watcher = watcher(Api::<CoreService>::all(kubeclient), ListParams::default());
    let mut backend = RealBackend::new(&opts.interface, &first_address, opts.executor);

    watcher
        .map(|event| {
            if let Err(e) = match event.unwrap() {
                Event::Applied(obj) => apply(&obj, &mut backend),
                Event::Restarted(obj) => obj.iter().try_for_each(|o| apply(o, &mut backend)),
                Event::Deleted(obj) => delete(&obj, &mut backend),
            } {
                error!("error while processing event: {:?}", e)
            }
        })
        .collect::<Vec<()>>()
        .await;

    Ok(())
}

fn node_ip(n: &Node) -> String {
    for add in n.status.clone().unwrap().addresses.unwrap().iter() {
        if add.type_ == "InternalIP" {
            return add.address.to_owned();
        }
    }
    "".to_owned()
}
