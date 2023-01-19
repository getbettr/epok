pub use tracing::{debug, info, warn};
use tracing_subscriber::{
    layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Registry,
};

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
