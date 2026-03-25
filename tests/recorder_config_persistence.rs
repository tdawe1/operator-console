use std::path::PathBuf;

use operator_console::recorder::{
    load_recorder_config_or_default, save_recorder_config, RecorderConfig,
};

#[test]
fn recorder_config_round_trips_through_disk() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let config_path = temp_dir.path().join("recorder.json");
    let config = RecorderConfig {
        command: PathBuf::from("/tmp/bet-recorder"),
        run_dir: PathBuf::from("/tmp/sabi-live"),
        session: String::from("helium-live"),
        companion_legs_path: Some(PathBuf::from("/tmp/sabi-live/companion-legs.json")),
        profile_path: Some(PathBuf::from("/tmp/owned-profile")),
        disabled_venues: String::from("bet365"),
        autostart: true,
        interval_seconds: 7,
        commission_rate: String::from("0"),
        target_profit: String::from("2"),
        stop_loss: String::from("1"),
        hard_margin_call_profit_floor: String::from("3"),
        warn_only_default: false,
    };

    let note = save_recorder_config(&config_path, &config).expect("save config");
    assert!(note.contains("Saved recorder config"));

    let (loaded, load_note) =
        load_recorder_config_or_default(&config_path).expect("load saved config");
    assert_eq!(loaded, config);
    assert!(load_note.contains("Loaded recorder config"));
}

#[test]
fn recorder_config_defaults_when_file_is_missing() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let config_path = temp_dir.path().join("missing.json");

    let (loaded, note) =
        load_recorder_config_or_default(&config_path).expect("load missing config");

    assert_eq!(loaded, RecorderConfig::default());
    assert_eq!(note, "Using default recorder config.");
    assert!(loaded.run_dir.to_string_lossy().contains("sabi/runs"));
}

#[test]
fn recorder_config_backfills_default_profile_path_for_legacy_json() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let config_path = temp_dir.path().join("legacy-recorder.json");
    std::fs::write(
        &config_path,
        r#"{
  "command": "/tmp/bet-recorder",
  "run_dir": "/tmp/sabi-live",
  "session": "helium-live",
  "companion_legs_path": null,
  "autostart": false,
  "interval_seconds": 5,
  "commission_rate": "0",
  "target_profit": "1",
  "stop_loss": "1",
  "hard_margin_call_profit_floor": "",
  "warn_only_default": true
}
"#,
    )
    .expect("write legacy config");

    let (loaded, note) = load_recorder_config_or_default(&config_path).expect("load legacy config");

    assert_eq!(
        loaded.profile_path,
        Some(
            std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .or_else(|| {
                    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config"))
                })
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join("smarkets-automation")
                .join("profile"),
        )
    );
    assert!(note.contains("Loaded recorder config"));
}

#[test]
fn recorder_config_preserves_explicit_null_profile_path() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let config_path = temp_dir.path().join("explicit-null-recorder.json");
    std::fs::write(
        &config_path,
        r#"{
  "command": "/tmp/bet-recorder",
  "run_dir": "/tmp/sabi-live",
  "session": "helium-live",
  "companion_legs_path": null,
  "profile_path": null,
  "autostart": false,
  "interval_seconds": 5,
  "commission_rate": "0",
  "target_profit": "1",
  "stop_loss": "1",
  "hard_margin_call_profit_floor": "",
  "warn_only_default": true
}
"#,
    )
    .expect("write explicit null config");

    let (loaded, _) =
        load_recorder_config_or_default(&config_path).expect("load explicit null config");

    assert_eq!(loaded.profile_path, None);
}

#[cfg(unix)]
#[test]
fn recorder_config_is_saved_with_private_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let config_path = temp_dir.path().join("recorder.json");

    save_recorder_config(&config_path, &RecorderConfig::default()).expect("save config");

    let mode = std::fs::metadata(&config_path)
        .expect("metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
}
