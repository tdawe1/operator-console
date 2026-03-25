use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};

use color_eyre::eyre::{eyre, Context, Result};

use crate::domain::ExchangePanelSnapshot;
use crate::provider::{ExchangeProvider, ProviderRequest};
pub use crate::transport::{
    WorkerConfig, WorkerRequestEnvelope as WorkerRequest, WorkerResponseEnvelope as WorkerResponse,
};

enum WorkerSessionError {
    Request(String),
    Session(color_eyre::Report),
}

impl WorkerSessionError {
    fn into_report(self) -> color_eyre::Report {
        match self {
            Self::Request(detail) => eyre!(detail),
            Self::Session(report) => report,
        }
    }
}

pub trait WorkerClient {
    fn send(&mut self, request: WorkerRequest) -> Result<WorkerResponse>;

    fn session_reconnect_count(&self) -> usize {
        0
    }
}

#[derive(Debug, Clone)]
enum BetRecorderCommand {
    Direct {
        executable: PathBuf,
    },
    PythonModule {
        python_executable: PathBuf,
        bet_recorder_root: PathBuf,
    },
}

#[derive(Debug, Clone)]
pub struct WorkerClientExchangeProvider<C> {
    client: C,
    config: WorkerConfig,
}

impl<C> WorkerClientExchangeProvider<C> {
    pub fn new(client: C, config: WorkerConfig) -> Self {
        Self { client, config }
    }
}

impl<C: WorkerClient + Send> ExchangeProvider for WorkerClientExchangeProvider<C> {
    fn handle(&mut self, request: ProviderRequest) -> Result<ExchangePanelSnapshot> {
        let worker_request = match request {
            ProviderRequest::LoadDashboard => WorkerRequest::LoadDashboard {
                config: self.config.clone(),
            },
            ProviderRequest::RefreshCached => WorkerRequest::RefreshCached,
            ProviderRequest::RefreshLive => WorkerRequest::RefreshLive,
            ProviderRequest::SelectVenue(venue) => WorkerRequest::SelectVenue { venue },
            ProviderRequest::CashOutTrackedBet { bet_id } => {
                WorkerRequest::CashOutTrackedBet { bet_id }
            }
            ProviderRequest::ExecuteTradingAction { intent } => {
                WorkerRequest::ExecuteTradingAction { intent }
            }
            ProviderRequest::LoadHorseMatcher { query } => {
                WorkerRequest::LoadHorseMatcher { query }
            }
        };

        let mut snapshot = self.client.send(worker_request)?.snapshot;
        if let Some(runtime) = snapshot.runtime.as_mut() {
            runtime.worker_reconnect_count = self.client.session_reconnect_count();
        }
        Ok(snapshot)
    }
}

pub struct BetRecorderWorkerClient {
    command: BetRecorderCommand,
    bootstrap_config: Option<WorkerConfig>,
    reconnect_count: usize,
    session: Option<WorkerSession>,
}

impl BetRecorderWorkerClient {
    pub fn new(python_executable: PathBuf, bet_recorder_root: PathBuf) -> Self {
        Self {
            command: BetRecorderCommand::PythonModule {
                python_executable,
                bet_recorder_root,
            },
            bootstrap_config: None,
            reconnect_count: 0,
            session: None,
        }
    }

    pub fn new_command(executable: PathBuf) -> Self {
        Self {
            command: BetRecorderCommand::Direct { executable },
            bootstrap_config: None,
            reconnect_count: 0,
            session: None,
        }
    }

    fn command(&self) -> Command {
        match &self.command {
            BetRecorderCommand::Direct { executable } => {
                let mut command = Command::new(executable);
                command
                    .arg("exchange-worker-session")
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
                command
            }
            BetRecorderCommand::PythonModule {
                python_executable,
                bet_recorder_root,
            } => {
                let mut command = Command::new(python_executable);
                command
                    .current_dir(bet_recorder_root)
                    .env("PYTHONPATH", bet_recorder_root.join("src"))
                    .arg("-m")
                    .arg("bet_recorder")
                    .arg("exchange-worker-session")
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
                command
            }
        }
    }

    fn session(&mut self) -> Result<&mut WorkerSession> {
        if self.session.is_none() {
            let child = self
                .command()
                .spawn()
                .wrap_err("failed to spawn bet-recorder worker subprocess")?;
            self.session = Some(WorkerSession::new(child)?);
        }

        self.session
            .as_mut()
            .ok_or_else(|| eyre!("worker session was not initialized"))
    }
}

impl WorkerClient for BetRecorderWorkerClient {
    fn send(&mut self, request: WorkerRequest) -> Result<WorkerResponse> {
        if let WorkerRequest::LoadDashboard { config } = &request {
            self.bootstrap_config = Some(config.clone());
        }

        match self.send_once(&request) {
            Ok(response) => Ok(response),
            Err(WorkerSessionError::Request(detail)) => Err(eyre!(detail)),
            Err(WorkerSessionError::Session(error)) => {
                self.session = None;
                if self.can_recover(&request) {
                    self.recover_and_retry(&request).wrap_err_with(|| {
                        format!("worker request failed before recovery: {}", error)
                    })
                } else {
                    Err(error)
                }
            }
        }
    }

    fn session_reconnect_count(&self) -> usize {
        self.reconnect_count
    }
}

impl BetRecorderWorkerClient {
    fn send_once(
        &mut self,
        request: &WorkerRequest,
    ) -> std::result::Result<WorkerResponse, WorkerSessionError> {
        let request_payload = serde_json::to_vec(request)
            .wrap_err("failed to serialize worker request")
            .map_err(WorkerSessionError::Session)?;

        let session = self.session().map_err(WorkerSessionError::Session)?;
        session
            .stdin
            .write_all(&request_payload)
            .wrap_err("failed to write worker request to stdin")
            .map_err(WorkerSessionError::Session)?;
        session
            .stdin
            .write_all(b"\n")
            .wrap_err("failed to frame worker request")
            .map_err(WorkerSessionError::Session)?;
        session
            .stdin
            .flush()
            .wrap_err("failed to flush worker request")
            .map_err(WorkerSessionError::Session)?;

        let mut response_line = String::new();
        let byte_count = session
            .stdout
            .read_line(&mut response_line)
            .wrap_err("failed to read worker response")
            .map_err(WorkerSessionError::Session)?;

        if byte_count == 0 {
            let stderr = session.read_stderr();
            self.session = None;
            return Err(WorkerSessionError::Session(eyre!(
                "bet-recorder worker session ended before responding: {}",
                stderr.trim()
            )));
        }

        let response = serde_json::from_str::<WorkerResponse>(response_line.trim_end())
            .wrap_err("failed to decode worker response")
            .map_err(WorkerSessionError::Session)?;
        if let Some(detail) = response.request_error.clone() {
            return Err(WorkerSessionError::Request(detail));
        }
        Ok(response)
    }

    fn can_recover(&self, request: &WorkerRequest) -> bool {
        matches!(request, WorkerRequest::LoadDashboard { .. }) || self.bootstrap_config.is_some()
    }

    fn recover_and_retry(&mut self, request: &WorkerRequest) -> Result<WorkerResponse> {
        if matches!(request, WorkerRequest::LoadDashboard { .. }) {
            return self
                .send_once(request)
                .map_err(WorkerSessionError::into_report);
        }

        let bootstrap_config = self
            .bootstrap_config
            .clone()
            .ok_or_else(|| eyre!("worker recovery requires bootstrap config"))?;
        let bootstrap_request = WorkerRequest::LoadDashboard {
            config: bootstrap_config,
        };

        let _bootstrap_response = self
            .send_once(&bootstrap_request)
            .map_err(WorkerSessionError::into_report)
            .wrap_err("failed to replay worker bootstrap after reconnect")?;
        self.reconnect_count += 1;
        self.send_once(request)
            .map_err(WorkerSessionError::into_report)
    }
}

struct WorkerSession {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    stderr: ChildStderr,
}

impl WorkerSession {
    fn new(mut child: Child) -> Result<Self> {
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| eyre!("bet-recorder worker stdin was not piped"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| eyre!("bet-recorder worker stdout was not piped"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| eyre!("bet-recorder worker stderr was not piped"))?;

        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            stderr,
        })
    }

    fn read_stderr(&mut self) -> String {
        let mut stderr = String::new();
        let _ = self.stderr.read_to_string(&mut stderr);
        let _ = self.child.wait();
        stderr
    }
}

impl Drop for WorkerSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
