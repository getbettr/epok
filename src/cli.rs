use clap::{Args, Parser};

use crate::*;

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
    pub host: String,
    #[clap(short = 'p', value_parser, env = "EPOK_SSH_PORT", default_value = "22")]
    pub port: u16,
    #[clap(short = 'k', value_parser, env = "EPOK_SSH_KEY")]
    pub key_path: String,
}
