use tokio::time::Duration;

pub mod cli;
pub mod logging;
pub mod operator;
pub mod res;
pub mod state;

pub mod built_info {
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

pub use cli::*;
pub use logging::*;
pub use operator::*;
pub use res::{Node, Service};
pub use state::{Interface, Op, Ops, State};

pub use k8s_openapi::api::core::v1::{Node as CoreNode, Service as CoreService};

pub const APP_NAME: &str = "epok";
pub const AUTHOR: &str = "Rareș Cosma - rares@getbetter.ro";
pub const ANNOTATION: &str = "getbetter.ro/externalport";
pub const DEBOUNCE_TIMEOUT: Duration = Duration::from_millis(500);
pub const RULE_MARKER: &str = "epok_rule_id";
pub const SERVICE_MARKER: &str = "epok_service_id";