pub mod app;
pub mod bet_recorder_provider;
pub mod domain;
pub mod panels;
pub mod provider;
pub mod recorder;
pub mod stub_provider;
pub mod transport;
pub mod ui;
pub mod worker_client;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
