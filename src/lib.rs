pub mod app;
pub mod bet_recorder_provider;
pub mod domain;
pub mod provider;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
