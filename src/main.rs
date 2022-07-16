mod backend;
mod logz;
mod operator;
mod res;

use clap::{Args, Parser};
use futures::StreamExt;
use k8s_openapi::api::core::v1::Service as CoreService;
use kube::{
    api::ListParams,
    runtime::{watcher, watcher::Event},
    Api, Client,
};
use tracing::{debug, error, info};

use std::fmt::Debug;

use crate::backend::IptablesBackend;

pub const APP_NAME: &str = "epok";
pub const AUTHOR: &str = "Rareș Cosma - rares@getbetter.ro";
pub const ANNOTATION: &str = "getbetter.ro/externalport";

mod built_info {
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

#[derive(Parser, Debug)]
#[clap(about = built_info::PKG_DESCRIPTION, author = AUTHOR)]
pub struct Opts {
    /// Comma-separated list of interfaces to forward packets from
    #[clap(long, short = 'i', value_parser, env = "EPOK_INTERFACES")]
    pub interfaces: String,

    #[clap(subcommand)]
    pub executor: Executor,
}

#[derive(clap::Parser, Debug)]
#[clap(long_about = "En taro Adun")]
pub enum Executor<Ssh: Args = SshHost> {
    /// Run operator on bare metal host
    Local,
    /// Run operator inside cluster, SSH-ing back to the metal
    Ssh(Ssh),
}

#[derive(clap::Parser, Debug)]
pub struct SshHost {
    #[clap(short = 'H', value_parser, env = "EPOK_SSH_HOST")]
    host: String,
    #[clap(short = 'p', value_parser, env = "EPOK_SSH_PORT", default_value = "22")]
    port: u16,
    #[clap(short = 'k', value_parser, env = "EPOK_SSH_KEY")]
    key_path: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    logz::initialize_logging("EPOK_LOG_LEVEL");
    logz::print_startup_string();

    let opts = Opts::parse();
    debug!("parsed options: {:?}", opts);
    let kubeclient = Client::try_default().await?;

    let first_node = res::Node::first(kubeclient.clone()).await?;

    info!(
        "forwarding from interfaces '{}' to ip '{}' of node '{}'",
        &opts.interfaces, &first_node.addr, &first_node.name
    );

    let watcher = watcher(Api::<CoreService>::all(kubeclient), ListParams::default());

    let mut operator = operator::Operator::new(
        IptablesBackend::new(&first_node.addr, opts.executor),
        opts.interfaces
            .split(',')
            .map(String::from)
            .collect::<Vec<_>>(),
    );

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
        .for_each(|_| futures::future::ready(()))
        .await;

    Ok(())
}
