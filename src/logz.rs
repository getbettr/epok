use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Registry};

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

pub fn print_startup_string(
    pkg_description: &str,
    pkg_version: &str,
    git_version: Option<&str>,
    target: &str,
    built_time: &str,
    rustc_version: &str,
) {
    let git_information = match git_version {
        None => "".to_string(),
        Some(git) => format!(" (Git information: {})", git),
    };
    info!("Starting {}", pkg_description);
    info!(
        "This is version {}{}, built for {} by {} at {}",
        pkg_version, git_information, target, rustc_version, built_time
    )
}
