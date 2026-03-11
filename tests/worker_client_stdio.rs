use std::fs;
use std::path::PathBuf;

use operator_console::transport::WorkerConfig;
use operator_console::worker_client::{BetRecorderWorkerClient, WorkerClient, WorkerRequest};
use tempfile::tempdir;

#[test]
fn worker_client_uses_exchange_worker_stdio_transport() {
    let temp_dir = tempdir().expect("temp dir");
    let src_dir = temp_dir.path().join("src").join("bet_recorder");
    fs::create_dir_all(&src_dir).expect("package dir");
    fs::write(src_dir.join("__init__.py"), "").expect("init");
    fs::write(
        src_dir.join("__main__.py"),
        "from bet_recorder.cli import main\n\nif __name__ == \"__main__\":\n    main()\n",
    )
    .expect("main");
    fs::write(
        src_dir.join("cli.py"),
        r#"
from __future__ import annotations
import json
import sys

def main() -> None:
    if len(sys.argv) < 2 or sys.argv[1] != "exchange-worker-session":
        raise SystemExit("unexpected command")
    request = json.loads(sys.stdin.readline())
    expected = {
        "LoadDashboard": {
            "config": {
                "positions_payload_path": "/tmp/ignored.json",
                "run_dir": None,
                "account_payload_path": None,
                "open_bets_payload_path": None,
                "agent_browser_session": None,
                "commission_rate": 0.0,
                "target_profit": 1.0,
                "stop_loss": 1.0,
            }
        }
    }
    if request != expected:
        raise SystemExit("unexpected request")
    sys.stdout.write(json.dumps({
        "snapshot": {
            "worker": {
                "name": "stub-worker",
                "status": "ready",
                "detail": "stub transport service"
            },
            "venues": [],
            "selected_venue": None,
            "events": [],
            "markets": [],
            "preflight": None,
            "status_line": "stub transport service",
            "watch": None
        }
    }) + "\n")
    sys.stdout.flush()

if __name__ == "__main__":
    main()
"#,
    )
    .expect("cli");

    let mut client = BetRecorderWorkerClient::new(
        PathBuf::from("/usr/bin/python"),
        temp_dir.path().to_path_buf(),
    );

    let response = client
        .send(WorkerRequest::LoadDashboard {
            config: WorkerConfig {
                positions_payload_path: Some(PathBuf::from("/tmp/ignored.json")),
                run_dir: None,
                account_payload_path: None,
                open_bets_payload_path: None,
                agent_browser_session: None,
                commission_rate: 0.0,
                target_profit: 1.0,
                stop_loss: 1.0,
            },
        })
        .expect("worker response");

    assert_eq!(response.snapshot.status_line, "stub transport service");
    assert_eq!(response.snapshot.worker.name, "stub-worker");
}
