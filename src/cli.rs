use clap::{Args, Parser};

use crate::*;

#[derive(Parser, Debug)]
#[clap(about = "External port operator for Kubernetes", author = AUTHOR)]
pub struct Opts {
    /// Comma-separated list of interfaces to forward packets from
    #[clap(long, short = 'i', value_parser, env = "EPOK_INTERFACES")]
    pub interfaces: String,

    #[clap(flatten)]
    pub batch_opts: BatchOpts,

    #[clap(subcommand)]
    pub executor: Executor,
}

#[derive(Parser, Debug)]
pub struct BatchOpts {
    #[clap(long, env = "EPOK_BATCH_COMMANDS", default_value = "true")]
    pub batch_commands: bool,

    #[clap(long, env = "EPOK_BATCH_SIZE", default_value = &ARG_MAX)]
    pub batch_size: usize,
}

#[derive(Parser, Debug)]
#[clap(long_about = "En taro Adun")]
pub enum Executor<Ssh: Args = SshHost> {
    /// Run operator on bare metal host
    Local,
    /// Run operator inside cluster, SSH-ing back to the metal
    Ssh(Ssh),
}

#[derive(Parser, Debug)]
pub struct SshHost {
    #[clap(short = 'H', value_parser, env = "EPOK_SSH_HOST")]
    pub host: String,
    #[clap(short = 'p', value_parser, env = "EPOK_SSH_PORT", default_value = "22")]
    pub port: u16,
    #[clap(short = 'k', value_parser, env = "EPOK_SSH_KEY")]
    pub key_path: String,
}
