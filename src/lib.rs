use cmd_lib::run_fun;
use lazy_static::lazy_static;
use tokio::time::Duration;
use thiserror::Error;

pub mod batch;
pub mod cli;
pub mod debounce;
pub mod executor;
pub mod iptables;
pub mod logging;
pub mod operator;
pub mod res;
pub mod state;
pub mod watcher;

lazy_static! {
    static ref ARG_MAX: String = {
        let default = 8192_u32;
        let res =
            run_fun!(getconf ARG_MAX).unwrap_or_else(|_| default.to_string());
        format!("{:.0}", (res.parse::<u32>().unwrap_or(default) as f32 * 0.8))
    };
}

pub use batch::Batch;
pub use cli::{BatchOpts, Executor, Opts, SshHost};
pub use debounce::Debounce;
pub use iptables::IptablesBackend;
pub use k8s_openapi::api::core::v1::{
    Node as CoreNode, Pod as CorePod, Service as CoreService,
};
pub use logging::*;
pub use operator::{Backend, Operator, Rule};
pub use res::{
    ExternalPorts, Interface, Node, Pod, PortSpec, Proto, Resource,
    ResourceLike, Service,
};
pub use state::{apply, Op, Ops, State};
pub use watcher::watch;

pub const ANNOTATION: &str = "epok.getbetter.ro/externalports";
pub const INTERNAL_ANNOTATION: &str = "epok.getbetter.ro/internal";
pub const ALLOW_RANGE_ANNOTATION: &str = "epok.getbetter.ro/allow-range";
pub const NODE_EXCLUDE_ANNOTATION: &str = "epok.getbetter.ro/exclude";
pub const NODE_EXCLUDE_LABEL: &str = "epok_exclude";
pub const OP_DEBOUNCE_TIMEOUT: Duration = Duration::from_millis(100);
pub const OP_CHANNEL_SIZE: usize = 64;
pub const OP_DEBOUNCE_CAPACITY: usize = 128;
pub const RULE_MARKER: &str = "epok_rule_id";
pub const SERVICE_MARKER: &str = "epok_service_id";

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("failed to list kube api resources: {0}")]
    KubeApiListError(#[source] kube::runtime::watcher::Error),
    #[error("failed to reconcile: {0}")]
    OperatorError(#[source] Box<Error>),
    #[error("command execution failed: {0}")]
    ExecutorError(#[source] std::io::Error),
    #[error("could not apply iptables rules: {0}")]
    BackendError(#[source] Box<Error>),
}
