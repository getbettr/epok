mod iptables;

use clap::Parser;
use futures::StreamExt;
use sha256::digest;
use std::fmt::Debug;
use std::str::FromStr;
use tracing::{debug, error, info};

use crate::iptables::{IptablesBackend, RealBackend};
use stackable_operator::{
    cli::Command,
    cli::ProductOperatorRun,
    k8s_openapi::api::core::v1::Service,
    kube::{
        api::Api,
        api::ListParams,
        runtime::{watcher, watcher::Event},
        Client,
    },
};

pub const APP_NAME: &str = "k8s-op";
pub const AUTHOR: &str = "Rareș Cosma - rares@getbetter.ro";
pub const ANNOTATION: &str = "getbetter.ro/externalport";

#[derive(Debug)]
struct ExternalPort {
    src: u32,
    nodeport: u32,
}

#[derive(Debug)]
pub struct SimpleService {
    name: String,
    namespace: String,
}

impl SimpleService {
    pub fn id(&self) -> String {
        let hash = digest(format!("{}::{}", self.namespace, self.name));
        format!("pf-{}-{}-{}", self.namespace, self.name, hash)
    }
}

#[derive(Debug)]
pub struct ServiceExternalPort {
    service: SimpleService,
    external_port: ExternalPort,
}

impl ServiceExternalPort {
    pub fn id(&self) -> String {
        let hash = digest(format!(
            "{}:{}",
            self.external_port.src, self.external_port.nodeport
        ));
        format!("{}-{}", self.service.id(), hash)
    }
}

impl FromStr for ExternalPort {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts = s.split(':').collect::<Vec<_>>();
        match parts.len() {
            2 => Ok(Self {
                src: parts[0].parse()?,
                nodeport: parts[1].parse()?,
            }),
            _ => Err(anyhow::Error::msg("nope")),
        }
    }
}

mod built_info {
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

#[derive(Parser)]
#[clap(about = built_info::PKG_DESCRIPTION, author = AUTHOR)]
struct Opts {
    #[clap(subcommand)]
    cmd: Command,
}

pub struct Ctx {
    pub client: stackable_operator::client::Client,
}

pub fn delete<B: IptablesBackend + Debug>(s: &Service, backend: &mut B) -> anyhow::Result<()> {
    let m = s.metadata.clone();
    let svc = SimpleService {
        name: m.name.unwrap(),
        namespace: m.namespace.unwrap(),
    };
    info!("service deleted: {:?}", &svc);
    backend.delete(&svc)?;
    Ok(())
}

pub fn insert<B: IptablesBackend + Debug>(s: &Service, backend: &mut B) -> anyhow::Result<()> {
    let m = s.metadata.clone();
    let svc = SimpleService {
        name: m.name.unwrap(),
        namespace: m.namespace.unwrap(),
    };

    if let Some(anno) = m.annotations {
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opts = Opts::parse();
    match opts.cmd {
        Command::Crd => eprintln!("not implemented!"),
        Command::Run(ProductOperatorRun { tracing_target, .. }) => {
            let kubeclient = Client::try_default().await?;

            stackable_operator::logging::initialize_logging(
                "K8S_OPERATOR_LOG",
                APP_NAME,
                tracing_target,
            );
            stackable_operator::utils::print_startup_string(
                built_info::PKG_DESCRIPTION,
                built_info::PKG_VERSION,
                built_info::GIT_VERSION,
                built_info::TARGET,
                built_info::BUILT_TIME_UTC,
                built_info::RUSTC_VERSION,
            );

            let watcher = watcher(Api::<Service>::all(kubeclient), ListParams::default());
            let mut backend = RealBackend::new("wlo1", "10.40.0.26");

            watcher
                .map(|event| {
                    if let Err(e) = match event.unwrap() {
                        Event::Applied(obj) => insert(&obj, &mut backend),
                        Event::Restarted(obj) => {
                            obj.iter().try_for_each(|o| insert(o, &mut backend))
                        }
                        Event::Deleted(obj) => delete(&obj, &mut backend),
                    } {
                        error!("error while processing event: {:?}", e)
                    }
                })
                .collect::<Vec<()>>()
                .await;
        }
    }

    Ok(())
}
