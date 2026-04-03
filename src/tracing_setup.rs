use tracing::Subscriber;
use tracing_subscriber::fmt;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

const DEFAULT_FILTER: &str = "info";

pub fn make_tracing_subscriber<W>(writer: W) -> impl Subscriber + Send + Sync
where
    W: for<'writer> MakeWriter<'writer> + Send + Sync + 'static,
{
    tracing_subscriber::registry()
        .with(default_env_filter())
        .with(
            fmt::layer()
                .with_writer(writer)
                .with_target(true)
                .with_ansi(false)
                .without_time(),
        )
}

pub fn init_tracing() -> Result<(), tracing::subscriber::SetGlobalDefaultError> {
    tracing::subscriber::set_global_default(make_tracing_subscriber(std::io::stderr))
}

fn default_env_filter() -> EnvFilter {
    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER))
}
