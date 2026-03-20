use std::fs;
use std::path::PathBuf;

use operator_console::domain::{
    ExchangePanelSnapshot, RuntimeSummary, WorkerStatus, WorkerSummary,
};
use operator_console::provider::ExchangeProvider;
use operator_console::transport::WorkerConfig;
use operator_console::worker_client::{
    BetRecorderWorkerClient, WorkerClient, WorkerClientExchangeProvider, WorkerRequest,
    WorkerResponse,
};
use serde_json::Value;
use tempfile::tempdir;

struct CountingClient {
    reconnect_count: usize,
}

impl WorkerClient for CountingClient {
    fn send(&mut self, _request: WorkerRequest) -> color_eyre::Result<WorkerResponse> {
        Ok(WorkerResponse {
            snapshot: ExchangePanelSnapshot {
                worker: WorkerSummary {
                    name: String::from("stub-worker"),
                    status: WorkerStatus::Ready,
                    detail: String::from("ok"),
                },
                runtime: Some(RuntimeSummary {
                    updated_at: String::from("2026-03-20T12:00:00Z"),
                    source: String::from("bet-recorder"),
                    refresh_kind: String::from("cached"),
                    worker_reconnect_count: 0,
                    decision_count: 0,
                    watcher_iteration: Some(1),
                    stale: false,
                }),
                ..ExchangePanelSnapshot::default()
            },
            request_error: None,
        })
    }

    fn session_reconnect_count(&self) -> usize {
        self.reconnect_count
    }
}

#[test]
fn worker_exchange_provider_stamps_runtime_with_client_reconnect_count() {
    let mut provider = WorkerClientExchangeProvider::new(
        CountingClient { reconnect_count: 3 },
        WorkerConfig {
            positions_payload_path: None,
            run_dir: None,
            account_payload_path: None,
            open_bets_payload_path: None,
            companion_legs_path: None,
            agent_browser_session: None,
            commission_rate: 0.0,
            target_profit: 1.0,
            stop_loss: 1.0,
            hard_margin_call_profit_floor: None,
            warn_only_default: true,
        },
    );

    let snapshot = provider
        .handle(operator_console::provider::ProviderRequest::LoadDashboard)
        .expect("provider response");

    assert_eq!(
        snapshot
            .runtime
            .as_ref()
            .expect("runtime should be present")
            .worker_reconnect_count,
        3
    );
}

#[test]
fn worker_client_reuses_one_process_for_multiple_requests() {
    let temp_dir = tempdir().expect("temp dir");
    let src_dir = temp_dir.path().join("src").join("bet_recorder");
    fs::create_dir_all(&src_dir).expect("package dir");
    fs::write(src_dir.join("__init__.py"), "").expect("init");
    fs::write(
        src_dir.join("__main__.py"),
        "from bet_recorder.cli import main\n\nif __name__ == \"__main__\":\n    main()\n",
    )
    .expect("main");

    let spawn_count_path = temp_dir.path().join("spawn-count.txt");
    fs::write(&spawn_count_path, "0").expect("spawn count seed");
    let spawn_count_literal = format!("{:?}", spawn_count_path.to_string_lossy().to_string());

    fs::write(
        src_dir.join("cli.py"),
        format!(
            r#"
from __future__ import annotations
import json
import pathlib
import sys

SPAWN_COUNT_PATH = pathlib.Path({spawn_count_literal})

def main() -> None:
    if len(sys.argv) < 2 or sys.argv[1] != "exchange-worker-session":
        raise SystemExit("unexpected command")

    current = int(SPAWN_COUNT_PATH.read_text())
    SPAWN_COUNT_PATH.write_text(str(current + 1))

    request_number = 0
    for line in sys.stdin:
        request = json.loads(line)
        request_number += 1
        if request == {{
            "LoadDashboard": {{
                "config": {{
                    "positions_payload_path": "/tmp/ignored.json",
                    "run_dir": None,
                    "account_payload_path": None,
                    "open_bets_payload_path": None,
                    "companion_legs_path": None,
                    "agent_browser_session": None,
                    "commission_rate": 0.0,
                    "target_profit": 1.0,
                    "stop_loss": 1.0,
                    "hard_margin_call_profit_floor": None,
                    "warn_only_default": True,
                }}
            }}
        }}:
            status_line = "response 1"
        elif request == "RefreshCached":
            status_line = "response 2"
        else:
            raise SystemExit(f"unexpected request: {{request}}")

        sys.stdout.write(json.dumps({{
            "snapshot": {{
                "worker": {{
                    "name": "stub-worker",
                    "status": "ready",
                    "detail": status_line
                }},
                "venues": [],
                "selected_venue": None,
                "events": [],
                "markets": [],
                "preflight": None,
                "status_line": status_line,
                "watch": None
            }}
        }}) + "\n")
        sys.stdout.flush()

if __name__ == "__main__":
    main()
"#
        ),
    )
    .expect("cli");

    let mut client = BetRecorderWorkerClient::new(
        PathBuf::from("/usr/bin/python"),
        temp_dir.path().to_path_buf(),
    );

    let first = client
        .send(WorkerRequest::LoadDashboard {
            config: WorkerConfig {
                positions_payload_path: Some(PathBuf::from("/tmp/ignored.json")),
                run_dir: None,
                account_payload_path: None,
                open_bets_payload_path: None,
                companion_legs_path: None,
                agent_browser_session: None,
                commission_rate: 0.0,
                target_profit: 1.0,
                stop_loss: 1.0,
                hard_margin_call_profit_floor: None,
                warn_only_default: true,
            },
        })
        .expect("first worker response");
    let second = client
        .send(WorkerRequest::RefreshCached)
        .expect("second worker response");

    assert_eq!(first.snapshot.status_line, "response 1");
    assert_eq!(second.snapshot.status_line, "response 2");
    assert_eq!(
        fs::read_to_string(spawn_count_path).expect("spawn count"),
        "1"
    );
}

#[test]
fn worker_client_reboots_session_and_replays_bootstrap_after_worker_exit() {
    let temp_dir = tempdir().expect("temp dir");
    let src_dir = temp_dir.path().join("src").join("bet_recorder");
    fs::create_dir_all(&src_dir).expect("package dir");
    fs::write(src_dir.join("__init__.py"), "").expect("init");
    fs::write(
        src_dir.join("__main__.py"),
        "from bet_recorder.cli import main\n\nif __name__ == \"__main__\":\n    main()\n",
    )
    .expect("main");

    let spawn_count_path = temp_dir.path().join("spawn-count.txt");
    fs::write(&spawn_count_path, "0").expect("spawn count seed");
    let spawn_count_literal = format!("{:?}", spawn_count_path.to_string_lossy().to_string());

    let request_log_path = temp_dir.path().join("request-log.txt");
    let request_log_literal = format!("{:?}", request_log_path.to_string_lossy().to_string());

    fs::write(
        src_dir.join("cli.py"),
        format!(
            r#"
from __future__ import annotations
import json
import pathlib
import sys

SPAWN_COUNT_PATH = pathlib.Path({spawn_count_literal})
REQUEST_LOG_PATH = pathlib.Path({request_log_literal})

LOAD_DASHBOARD = {{
    "LoadDashboard": {{
            "config": {{
                "positions_payload_path": "/tmp/ignored.json",
                "run_dir": None,
                "account_payload_path": None,
                "open_bets_payload_path": None,
                "companion_legs_path": None,
                "agent_browser_session": None,
                "commission_rate": 0.0,
                "target_profit": 1.0,
                "stop_loss": 1.0,
                "hard_margin_call_profit_floor": None,
                "warn_only_default": True,
        }}
    }}
}}

def record_request(spawn_number: int, request: object) -> None:
    with REQUEST_LOG_PATH.open("a", encoding="utf-8") as handle:
        handle.write(f"{{spawn_number}}:{{json.dumps(request, sort_keys=True)}}\n")

def main() -> None:
    if len(sys.argv) < 2 or sys.argv[1] != "exchange-worker-session":
        raise SystemExit("unexpected command")

    spawn_number = int(SPAWN_COUNT_PATH.read_text()) + 1
    SPAWN_COUNT_PATH.write_text(str(spawn_number))

    if spawn_number == 1:
        request = json.loads(sys.stdin.readline())
        record_request(spawn_number, request)
        if request != LOAD_DASHBOARD:
            raise SystemExit(f"unexpected first request: {{request}}")
        sys.stdout.write(json.dumps({{
            "snapshot": {{
                "worker": {{"name": "stub-worker", "status": "ready", "detail": "boot-1"}},
                "venues": [],
                "selected_venue": None,
                "events": [],
                "markets": [],
                "preflight": None,
                "status_line": "boot-1",
                "watch": None
            }}
        }}) + "\n")
        sys.stdout.flush()
        return

    first = json.loads(sys.stdin.readline())
    record_request(spawn_number, first)
    if first != LOAD_DASHBOARD:
        raise SystemExit(f"unexpected reboot request: {{first}}")
    sys.stdout.write(json.dumps({{
        "snapshot": {{
            "worker": {{"name": "stub-worker", "status": "ready", "detail": "boot-2"}},
            "venues": [],
            "selected_venue": None,
            "events": [],
            "markets": [],
            "preflight": None,
            "status_line": "boot-2",
            "watch": None
        }}
    }}) + "\n")
    sys.stdout.flush()

    second = json.loads(sys.stdin.readline())
    record_request(spawn_number, second)
    if second != "RefreshCached":
        raise SystemExit(f"unexpected replayed request: {{second}}")
    sys.stdout.write(json.dumps({{
        "snapshot": {{
            "worker": {{"name": "stub-worker", "status": "ready", "detail": "response 2"}},
            "venues": [],
            "selected_venue": None,
            "events": [],
            "markets": [],
            "preflight": None,
            "status_line": "response 2",
            "watch": None
        }}
    }}) + "\n")
    sys.stdout.flush()

if __name__ == "__main__":
    main()
"#
        ),
    )
    .expect("cli");

    let bootstrap_config = WorkerConfig {
        positions_payload_path: Some(PathBuf::from("/tmp/ignored.json")),
        run_dir: None,
        account_payload_path: None,
        open_bets_payload_path: None,
        companion_legs_path: None,
        agent_browser_session: None,
        commission_rate: 0.0,
        target_profit: 1.0,
        stop_loss: 1.0,
        hard_margin_call_profit_floor: None,
        warn_only_default: true,
    };

    let mut client = BetRecorderWorkerClient::new(
        PathBuf::from("/usr/bin/python"),
        temp_dir.path().to_path_buf(),
    );

    let first = client
        .send(WorkerRequest::LoadDashboard {
            config: bootstrap_config.clone(),
        })
        .expect("first worker response");
    let second = client
        .send(WorkerRequest::RefreshCached)
        .expect("refreshed after worker restart");

    assert_eq!(first.snapshot.status_line, "boot-1");
    assert_eq!(second.snapshot.status_line, "response 2");
    assert_eq!(
        fs::read_to_string(spawn_count_path).expect("spawn count"),
        "2"
    );
    let logged_requests = fs::read_to_string(request_log_path)
        .expect("request log")
        .lines()
        .map(parse_logged_request)
        .collect::<Vec<_>>();
    let expected_bootstrap = serde_json::json!({
        "LoadDashboard": {
                "config": {
                "positions_payload_path": "/tmp/ignored.json",
                "run_dir": null,
                "account_payload_path": null,
                "open_bets_payload_path": null,
                "companion_legs_path": null,
                "agent_browser_session": null,
                "commission_rate": 0.0,
                "target_profit": 1.0,
                "stop_loss": 1.0,
                "hard_margin_call_profit_floor": null,
                "warn_only_default": true,
            }
        }
    });
    assert_eq!(
        logged_requests,
        vec![
            (1, expected_bootstrap.clone()),
            (2, expected_bootstrap),
            (2, serde_json::json!("RefreshCached")),
        ]
    );
}

#[test]
fn worker_client_reboots_session_and_replays_live_refresh_after_worker_exit() {
    let temp_dir = tempdir().expect("temp dir");
    let src_dir = temp_dir.path().join("src").join("bet_recorder");
    fs::create_dir_all(&src_dir).expect("package dir");
    fs::write(src_dir.join("__init__.py"), "").expect("init");
    fs::write(
        src_dir.join("__main__.py"),
        "from bet_recorder.cli import main\n\nif __name__ == \"__main__\":\n    main()\n",
    )
    .expect("main");

    let spawn_count_path = temp_dir.path().join("spawn-count.txt");
    fs::write(&spawn_count_path, "0").expect("spawn count seed");
    let spawn_count_literal = format!("{:?}", spawn_count_path.to_string_lossy().to_string());

    let request_log_path = temp_dir.path().join("request-log-live.txt");
    let request_log_literal = format!("{:?}", request_log_path.to_string_lossy().to_string());

    fs::write(
        src_dir.join("cli.py"),
        format!(
            r#"
from __future__ import annotations
import json
import pathlib
import sys

SPAWN_COUNT_PATH = pathlib.Path({spawn_count_literal})
REQUEST_LOG_PATH = pathlib.Path({request_log_literal})

LOAD_DASHBOARD = {{
    "LoadDashboard": {{
            "config": {{
                "positions_payload_path": "/tmp/ignored.json",
                "run_dir": None,
                "account_payload_path": None,
                "open_bets_payload_path": None,
                "companion_legs_path": None,
                "agent_browser_session": None,
                "commission_rate": 0.0,
                "target_profit": 1.0,
                "stop_loss": 1.0,
                "hard_margin_call_profit_floor": None,
                "warn_only_default": True,
        }}
    }}
}}

def record_request(spawn_number: int, request: object) -> None:
    with REQUEST_LOG_PATH.open("a", encoding="utf-8") as handle:
        handle.write(f"{{spawn_number}}:{{json.dumps(request, sort_keys=True)}}\n")

def main() -> None:
    if len(sys.argv) < 2 or sys.argv[1] != "exchange-worker-session":
        raise SystemExit("unexpected command")

    spawn_number = int(SPAWN_COUNT_PATH.read_text()) + 1
    SPAWN_COUNT_PATH.write_text(str(spawn_number))

    if spawn_number == 1:
        request = json.loads(sys.stdin.readline())
        record_request(spawn_number, request)
        if request != LOAD_DASHBOARD:
            raise SystemExit(f"unexpected first request: {{request}}")
        sys.stdout.write(json.dumps({{
            "snapshot": {{
                "worker": {{"name": "stub-worker", "status": "ready", "detail": "boot-1"}},
                "venues": [],
                "selected_venue": None,
                "events": [],
                "markets": [],
                "preflight": None,
                "status_line": "boot-1",
                "watch": None
            }}
        }}) + "\n")
        sys.stdout.flush()
        return

    first = json.loads(sys.stdin.readline())
    record_request(spawn_number, first)
    if first != LOAD_DASHBOARD:
        raise SystemExit(f"unexpected reboot request: {{first}}")
    sys.stdout.write(json.dumps({{
        "snapshot": {{
            "worker": {{"name": "stub-worker", "status": "ready", "detail": "boot-2"}},
            "venues": [],
            "selected_venue": None,
            "events": [],
            "markets": [],
            "preflight": None,
            "status_line": "boot-2",
            "watch": None
        }}
    }}) + "\n")
    sys.stdout.flush()

    second = json.loads(sys.stdin.readline())
    record_request(spawn_number, second)
    if second != "RefreshLive":
        raise SystemExit(f"unexpected replayed request: {{second}}")
    sys.stdout.write(json.dumps({{
        "snapshot": {{
            "worker": {{"name": "stub-worker", "status": "ready", "detail": "live-response"}},
            "venues": [],
            "selected_venue": None,
            "events": [],
            "markets": [],
            "preflight": None,
            "status_line": "live-response",
            "watch": None
        }}
    }}) + "\n")
    sys.stdout.flush()

if __name__ == "__main__":
    main()
"#
        ),
    )
    .expect("cli");

    let bootstrap_config = WorkerConfig {
        positions_payload_path: Some(PathBuf::from("/tmp/ignored.json")),
        run_dir: None,
        account_payload_path: None,
        open_bets_payload_path: None,
        companion_legs_path: None,
        agent_browser_session: None,
        commission_rate: 0.0,
        target_profit: 1.0,
        stop_loss: 1.0,
        hard_margin_call_profit_floor: None,
        warn_only_default: true,
    };

    let mut client = BetRecorderWorkerClient::new(
        PathBuf::from("/usr/bin/python"),
        temp_dir.path().to_path_buf(),
    );

    let first = client
        .send(WorkerRequest::LoadDashboard {
            config: bootstrap_config.clone(),
        })
        .expect("first worker response");
    let second = client
        .send(WorkerRequest::RefreshLive)
        .expect("live refresh after worker restart");

    assert_eq!(first.snapshot.status_line, "boot-1");
    assert_eq!(second.snapshot.status_line, "live-response");
    assert_eq!(
        fs::read_to_string(spawn_count_path).expect("spawn count"),
        "2"
    );
    let logged_requests = fs::read_to_string(request_log_path)
        .expect("request log")
        .lines()
        .map(parse_logged_request)
        .collect::<Vec<_>>();
    let expected_bootstrap = serde_json::json!({
        "LoadDashboard": {
                "config": {
                "positions_payload_path": "/tmp/ignored.json",
                "run_dir": null,
                "account_payload_path": null,
                "open_bets_payload_path": null,
                "companion_legs_path": null,
                "agent_browser_session": null,
                "commission_rate": 0.0,
                "target_profit": 1.0,
                "stop_loss": 1.0,
                "hard_margin_call_profit_floor": null,
                "warn_only_default": true,
            }
        }
    });
    assert_eq!(
        logged_requests,
        vec![
            (1, expected_bootstrap.clone()),
            (2, expected_bootstrap),
            (2, serde_json::json!("RefreshLive")),
        ]
    );
}

#[test]
fn worker_client_keeps_session_alive_after_request_error_response() {
    let temp_dir = tempdir().expect("temp dir");
    let src_dir = temp_dir.path().join("src").join("bet_recorder");
    fs::create_dir_all(&src_dir).expect("package dir");
    fs::write(src_dir.join("__init__.py"), "").expect("init");
    fs::write(
        src_dir.join("__main__.py"),
        "from bet_recorder.cli import main\n\nif __name__ == \"__main__\":\n    main()\n",
    )
    .expect("main");

    let spawn_count_path = temp_dir.path().join("spawn-count.txt");
    fs::write(&spawn_count_path, "0").expect("spawn count seed");
    let spawn_count_literal = format!("{:?}", spawn_count_path.to_string_lossy().to_string());

    fs::write(
        src_dir.join("cli.py"),
        format!(
            r#"
from __future__ import annotations
import json
import pathlib
import sys

SPAWN_COUNT_PATH = pathlib.Path({spawn_count_literal})

LOAD_DASHBOARD = {{
    "LoadDashboard": {{
        "config": {{
            "positions_payload_path": "/tmp/ignored.json",
            "run_dir": None,
            "account_payload_path": None,
            "open_bets_payload_path": None,
            "companion_legs_path": None,
            "agent_browser_session": None,
            "commission_rate": 0.0,
            "target_profit": 1.0,
            "stop_loss": 1.0,
            "hard_margin_call_profit_floor": None,
            "warn_only_default": True,
        }}
    }}
}}

def main() -> None:
    if len(sys.argv) < 2 or sys.argv[1] != "exchange-worker-session":
        raise SystemExit("unexpected command")

    current = int(SPAWN_COUNT_PATH.read_text())
    SPAWN_COUNT_PATH.write_text(str(current + 1))

    bootstrapped = False
    for line in sys.stdin:
        request = json.loads(line)
        if request == LOAD_DASHBOARD and not bootstrapped:
            bootstrapped = True
            sys.stdout.write(json.dumps({{
                "snapshot": {{
                    "worker": {{
                        "name": "stub-worker",
                        "status": "error",
                        "detail": "missing snapshot"
                    }},
                    "venues": [],
                    "selected_venue": None,
                    "events": [],
                    "markets": [],
                    "preflight": None,
                    "status_line": "missing snapshot",
                    "watch": None
                }},
                "request_error": "No positions_snapshot event found in run bundle"
            }}) + "\n")
            sys.stdout.flush()
            continue

        if request == "RefreshCached" and bootstrapped:
            sys.stdout.write(json.dumps({{
                "snapshot": {{
                    "worker": {{
                        "name": "stub-worker",
                        "status": "ready",
                        "detail": "response 2"
                    }},
                    "venues": [],
                    "selected_venue": None,
                    "events": [],
                    "markets": [],
                    "preflight": None,
                    "status_line": "response 2",
                    "watch": None
                }}
            }}) + "\n")
            sys.stdout.flush()
            continue

        raise SystemExit(f"unexpected request: {{request}}")

if __name__ == "__main__":
    main()
"#
        ),
    )
    .expect("cli");

    let mut client = BetRecorderWorkerClient::new(
        PathBuf::from("/usr/bin/python"),
        temp_dir.path().to_path_buf(),
    );

    let first_error = client
        .send(WorkerRequest::LoadDashboard {
            config: WorkerConfig {
                positions_payload_path: Some(PathBuf::from("/tmp/ignored.json")),
                run_dir: None,
                account_payload_path: None,
                open_bets_payload_path: None,
                companion_legs_path: None,
                agent_browser_session: None,
                commission_rate: 0.0,
                target_profit: 1.0,
                stop_loss: 1.0,
                hard_margin_call_profit_floor: None,
                warn_only_default: true,
            },
        })
        .expect_err("request error should surface without killing the session");
    assert!(first_error
        .to_string()
        .contains("No positions_snapshot event found in run bundle"));

    let refresh = client
        .send(WorkerRequest::RefreshCached)
        .expect("refresh should reuse the same session");
    assert_eq!(refresh.snapshot.status_line, "response 2");
    assert_eq!(
        fs::read_to_string(spawn_count_path).expect("spawn count"),
        "1"
    );
}

fn parse_logged_request(line: &str) -> (usize, Value) {
    let (spawn_number, payload) = line.split_once(':').expect("spawn separator");
    (
        spawn_number.parse::<usize>().expect("spawn number"),
        serde_json::from_str(payload).expect("payload json"),
    )
}
