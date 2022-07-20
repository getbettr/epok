use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Registry};

pub use tracing::{debug, info, warn};

use crate::built_info;

pub fn initialize_logging(env: &str) {
    let filter = match EnvFilter::try_from_env(env) {
        Ok(env_filter) => env_filter,
        _ => EnvFilter::try_new(tracing::Level::INFO.to_string())
            .expect("Failed to initialize default tracing level to INFO"),
    };

    let fmt = tracing_subscriber::fmt::layer();
    let registry = Registry::default().with(filter).with(fmt);
    registry.init();
}

pub fn print_startup_string() {
    let git_information = match built_info::GIT_VERSION {
        None => "".to_string(),
        Some(git) => format!(" (git: {})", git),
    };
    info!("Starting {}", built_info::PKG_DESCRIPTION);
    info!(
        "This is version {}{}, built for {} by {} on {}",
        built_info::PKG_VERSION,
        git_information,
        built_info::TARGET,
        built_info::RUSTC_VERSION,
        built_info::BUILT_TIME_UTC
    )
}
