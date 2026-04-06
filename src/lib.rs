pub mod agent_browser;
pub mod alerts;
pub mod app;
mod app_state;
pub mod calculator;
pub mod domain;
pub mod exchange_api;
pub mod execution_backend;
pub mod horse_matcher;
pub mod manual_positions;
pub mod market_intel;
pub mod market_normalization;
pub mod native_provider;
pub mod native_trading;
pub mod oddsmatcher;
pub mod owls;
pub mod panels;
pub mod provider;
pub mod recorder;
pub mod resource_state;
pub mod runtime;
pub mod stub_provider;
pub mod theme;
pub mod tracing_setup;
pub mod trading_actions;
pub mod transport;
pub mod ui;
pub mod wm;
pub mod worker_client;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
