use cmd_lib::run_fun;
use lazy_static::lazy_static;
use tokio::time::Duration;

pub mod batch;
pub mod cli;
pub mod debounce;
pub mod executor;
pub mod iptables;
pub mod logging;
pub mod operator;
pub mod res;
pub mod state;

lazy_static! {
    static ref ARG_MAX: String = {
        let default = 8192_u32;
        let res = run_fun!(getconf ARG_MAX).unwrap_or_else(|_| default.to_string());
        format!(
            "{:.0}",
            (res.parse::<u32>().unwrap_or(default) as f32 * 0.8)
        )
    };
}

pub use batch::Batch;
pub use cli::{BatchOpts, Executor, Opts, SshHost};
pub use debounce::Debounce;
pub use iptables::IptablesBackend;
pub use logging::*;
pub use operator::{Backend, Operator, Rule};
pub use res::{ExternalPort, Interface, Node, Resource, ResourceLike, Service};
pub use state::{Op, Ops, State};

pub use k8s_openapi::api::core::v1::{Node as CoreNode, Service as CoreService};

pub const ANNOTATION: &str = "epok.getbetter.ro/externalport";
pub const NODE_EXCLUDE_ANNOTATION: &str = "epok.getbetter.ro/exclude";
pub const NODE_EXCLUDE_LABEL: &str = "epok_exclude";
pub const OP_DEBOUNCE_TIMEOUT: Duration = Duration::from_millis(100);
pub const OP_CHANNEL_SIZE: usize = 64;
pub const OP_DEBOUNCE_CAPACITY: usize = 128;
pub const RULE_MARKER: &str = "epok_rule_id";
pub const SERVICE_MARKER: &str = "epok_service_id";
