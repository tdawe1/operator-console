use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

use color_eyre::eyre::{Result, WrapErr};
use reqwest::blocking::Client;
use reqwest::Client as AsyncClient;
#[cfg(test)]
use tokio::runtime::Handle;
use tokio::runtime::{Builder, Runtime};
use tokio::sync::mpsc::{self as tokio_mpsc, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use crate::app::{
    start_market_intel_worker, start_oddsmatcher_worker, MarketIntelJob, MarketIntelResult,
    MatchbookSyncJob, MatchbookSyncResult, OddsMatcherJob, OddsMatcherResult, OwlsSyncJob,
    OwlsSyncResult, ProviderJob, ProviderResult,
};
use crate::provider::ExchangeProvider;

#[derive(Clone)]
pub(crate) struct AppRuntimeHost {
    runtime: Arc<Runtime>,
}

impl AppRuntimeHost {
    pub(crate) fn new() -> Result<Self> {
        let runtime = Builder::new_multi_thread()
            .enable_all()
            .worker_threads(2)
            .thread_name("operator-console-runtime")
            .build()
            .wrap_err("failed to build app runtime")?;
        Ok(Self {
            runtime: Arc::new(runtime),
        })
    }

    #[cfg(test)]
    pub(crate) fn handle(&self) -> Handle {
        self.runtime.handle().clone()
    }

    pub(crate) fn spawn<F>(&self, future: F) -> JoinHandle<F::Output>
    where
        F: std::future::Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.runtime.spawn(future)
    }
}

pub(crate) struct AppRuntimeChannels {
    pub(crate) provider_tx: UnboundedSender<ProviderJob>,
    pub(crate) provider_rx: UnboundedReceiver<ProviderResult>,
    pub(crate) oddsmatcher_tx: Sender<OddsMatcherJob>,
    pub(crate) oddsmatcher_rx: Receiver<OddsMatcherResult>,
    pub(crate) market_intel_tx: Sender<MarketIntelJob>,
    pub(crate) market_intel_rx: Receiver<MarketIntelResult>,
    pub(crate) owls_sync_tx: UnboundedSender<OwlsSyncJob>,
    pub(crate) owls_sync_rx: UnboundedReceiver<OwlsSyncResult>,
    pub(crate) matchbook_sync_tx: UnboundedSender<MatchbookSyncJob>,
    pub(crate) matchbook_sync_rx: UnboundedReceiver<MatchbookSyncResult>,
}

impl AppRuntimeChannels {
    pub(crate) fn start_all(
        host: &AppRuntimeHost,
        provider: Box<dyn ExchangeProvider + Send>,
        oddsmatcher_client: Client,
        owls_client: AsyncClient,
    ) -> Self {
        debug!("starting current worker runtime channels");
        let (provider_tx, provider_rx) = Self::start_provider(host, provider);
        let (oddsmatcher_tx, oddsmatcher_rx) = Self::start_oddsmatcher(host, oddsmatcher_client);
        let (market_intel_tx, market_intel_rx) = Self::start_market_intel(host);
        let (owls_sync_tx, owls_sync_rx) = Self::start_owls(host, owls_client);
        let (matchbook_sync_tx, matchbook_sync_rx) = Self::start_matchbook(host);

        Self {
            provider_tx,
            provider_rx,
            oddsmatcher_tx,
            oddsmatcher_rx,
            market_intel_tx,
            market_intel_rx,
            owls_sync_tx,
            owls_sync_rx,
            matchbook_sync_tx,
            matchbook_sync_rx,
        }
    }

    pub(crate) fn start_provider(
        host: &AppRuntimeHost,
        provider: Box<dyn ExchangeProvider + Send>,
    ) -> (
        UnboundedSender<ProviderJob>,
        UnboundedReceiver<ProviderResult>,
    ) {
        let (job_tx, mut job_rx) = tokio_mpsc::unbounded_channel::<ProviderJob>();
        let (result_tx, result_rx) = tokio_mpsc::unbounded_channel::<ProviderResult>();

        host.spawn(async move {
            let mut provider = provider;
            while let Some(job) = job_rx.recv().await {
                let request = job.request.clone();
                debug!(request = ?request, "provider runtime job started");
                let result = provider
                    .handle(request.clone())
                    .map_err(|error| error.to_string());
                match &result {
                    Ok(_) => debug!(request = ?request, "provider runtime job completed"),
                    Err(error) => {
                        warn!(request = ?request, error = %error, "provider runtime job failed")
                    }
                }
                if result_tx
                    .send(ProviderResult {
                        request,
                        result,
                        failure_context: job.failure_context,
                        event_message: job.event_message,
                    })
                    .is_err()
                {
                    break;
                }
            }
        });

        (job_tx, result_rx)
    }

    pub(crate) fn start_oddsmatcher(
        _host: &AppRuntimeHost,
        client: Client,
    ) -> (Sender<OddsMatcherJob>, Receiver<OddsMatcherResult>) {
        start_oddsmatcher_worker(client)
    }

    pub(crate) fn start_market_intel(
        _host: &AppRuntimeHost,
    ) -> (Sender<MarketIntelJob>, Receiver<MarketIntelResult>) {
        start_market_intel_worker()
    }

    pub(crate) fn start_owls(
        host: &AppRuntimeHost,
        client: AsyncClient,
    ) -> (
        UnboundedSender<OwlsSyncJob>,
        UnboundedReceiver<OwlsSyncResult>,
    ) {
        let (job_tx, mut job_rx) = tokio_mpsc::unbounded_channel::<OwlsSyncJob>();
        let (result_tx, result_rx) = tokio_mpsc::unbounded_channel::<OwlsSyncResult>();

        host.spawn(async move {
            while let Some(job) = job_rx.recv().await {
                debug!(reason = job.reason.label(), focused = ?job.focused, "owls runtime job started");
                let outcome = crate::owls::sync_dashboard_async(
                    &client,
                    &job.dashboard,
                    job.reason,
                    job.focused,
                )
                .await;
                info!(reason = job.reason.label(), checked = outcome.checked_count, changed = outcome.changed_count, "owls runtime job completed");
                if result_tx
                    .send(OwlsSyncResult {
                        outcome,
                        reason: job.reason,
                    })
                    .is_err()
                {
                    break;
                }
            }
        });

        (job_tx, result_rx)
    }

    pub(crate) fn start_matchbook(
        host: &AppRuntimeHost,
    ) -> (
        UnboundedSender<MatchbookSyncJob>,
        UnboundedReceiver<MatchbookSyncResult>,
    ) {
        let (job_tx, mut job_rx) = tokio_mpsc::unbounded_channel::<MatchbookSyncJob>();
        let (result_tx, result_rx) = tokio_mpsc::unbounded_channel::<MatchbookSyncResult>();

        host.spawn(async move {
            while let Some(job) = job_rx.recv().await {
                debug!(reason = job.reason.label(), "matchbook runtime job started");
                let state =
                    crate::matchbook_backend::load_account_state(matches!(
                        job.reason,
                        crate::app::MatchbookSyncReason::Manual
                    ))
                    .await
                    .map_err(|error| error.to_string());
                match &state {
                    Ok(_) => debug!(reason = job.reason.label(), "matchbook runtime job completed"),
                    Err(error) => warn!(reason = job.reason.label(), error = %error, "matchbook runtime job failed"),
                }
                if result_tx
                    .send(MatchbookSyncResult {
                        state,
                        reason: job.reason,
                    })
                    .is_err()
                {
                    break;
                }
            }
        });

        (job_tx, result_rx)
    }
}

#[cfg(test)]
mod tests {
    use reqwest::blocking::Client;
    use tokio::sync::mpsc::error::TryRecvError;
    use tokio::sync::oneshot;
    use tokio::time::{sleep, Duration as TokioDuration};

    use super::{AppRuntimeChannels, AppRuntimeHost};
    use crate::app::ProviderJob;
    use crate::domain::ExchangePanelSnapshot;
    use crate::provider::{ExchangeProvider, ProviderRequest};

    struct RuntimeTestProvider;

    impl ExchangeProvider for RuntimeTestProvider {
        fn handle(
            &mut self,
            request: ProviderRequest,
        ) -> color_eyre::Result<ExchangePanelSnapshot> {
            let mut snapshot = ExchangePanelSnapshot::empty();
            snapshot.status_line = format!("runtime::{request:?}");
            Ok(snapshot)
        }
    }

    #[test]
    fn runtime_host_runs_spawned_futures() {
        let runtime = AppRuntimeHost::new().expect("runtime host");
        let (tx, rx) = oneshot::channel();

        runtime.spawn(async move {
            sleep(TokioDuration::from_millis(10)).await;
            tx.send(42_u8).expect("send result");
        });

        let value = runtime
            .handle()
            .block_on(async { rx.await.expect("receive result") });
        assert_eq!(value, 42);
    }

    #[test]
    fn runtime_channels_dispatch_provider_requests_through_current_worker_impl() {
        let runtime = AppRuntimeHost::new().expect("runtime host");
        let (provider_tx, mut provider_rx) =
            AppRuntimeChannels::start_provider(&runtime, Box::new(RuntimeTestProvider));

        provider_tx
            .send(ProviderJob {
                request: ProviderRequest::LoadDashboard,
                failure_context: String::from("test"),
                event_message: None,
            })
            .expect("send provider job");

        let result = runtime.handle().block_on(async {
            tokio::time::timeout(TokioDuration::from_millis(200), async {
                loop {
                    match provider_rx.try_recv() {
                        Ok(result) => return result,
                        Err(TryRecvError::Empty) => sleep(TokioDuration::from_millis(5)).await,
                        Err(error) => panic!("receive provider result: {error}"),
                    }
                }
            })
            .await
            .expect("provider result timeout")
        });

        assert_eq!(result.request, ProviderRequest::LoadDashboard);
        assert_eq!(
            result.result.expect("provider result").status_line,
            "runtime::LoadDashboard"
        );
    }

    #[test]
    fn runtime_channels_start_all_worker_sets() {
        let host = AppRuntimeHost::new().expect("runtime host");
        let runtime = AppRuntimeChannels::start_all(
            &host,
            Box::new(RuntimeTestProvider),
            Client::new(),
            reqwest::Client::new(),
        );

        runtime
            .provider_tx
            .send(ProviderJob {
                request: ProviderRequest::LoadDashboard,
                failure_context: String::from("test"),
                event_message: None,
            })
            .expect("send provider job");

        let mut provider_rx = runtime.provider_rx;
        let result = host.handle().block_on(async {
            tokio::time::timeout(TokioDuration::from_millis(200), async {
                loop {
                    match provider_rx.try_recv() {
                        Ok(result) => return result,
                        Err(TryRecvError::Empty) => sleep(TokioDuration::from_millis(5)).await,
                        Err(error) => panic!("receive provider result: {error}"),
                    }
                }
            })
            .await
            .expect("provider result timeout")
        });

        assert_eq!(result.request, ProviderRequest::LoadDashboard);
    }
}
