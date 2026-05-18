use std::{
    fs,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn api_binary_starts_with_generated_config() {
    let config_path = write_temp_config(valid_config());

    let output = Command::new(env!("CARGO_BIN_EXE_api"))
        .env("PROOFPLANE_CONFIG", &config_path)
        .output()
        .expect("api binary runs");

    assert!(
        output.status.success(),
        "api stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("proofplane api scaffold ready"),
        "api stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );

    let _ = fs::remove_file(config_path);
}

#[test]
fn api_binary_fails_without_config_environment_variable() {
    let output = Command::new(env!("CARGO_BIN_EXE_api"))
        .env_remove("PROOFPLANE_CONFIG")
        .output()
        .expect("api binary runs");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("PROOFPLANE_CONFIG"),
        "api stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn valid_config() -> &'static str {
    r#"
environment: integration
server:
  api_bind: "127.0.0.1:0"
  worker_bind: "127.0.0.1:0"
  mcp_bind: "127.0.0.1:0"
postgres:
  host: "127.0.0.1"
  port: 5432
  database: "proofplane"
  username: "proofplane"
  password: "proofplane"
pubsub:
  project_id: "proofplane-integration"
  emulator_host: "127.0.0.1:8085"
  topics:
    outbox: "proofplane-outbox"
    dead_letter: "proofplane-dead-letter"
  subscriptions:
    worker: "proofplane-worker"
object_storage:
  backend: "filesystem"
  root: ".local/storage"
observability:
  log_format: "json"
  default_filter: "info"
auth:
  api_key_header: "x-proofplane-api-key"
  credential_hash_pepper: "pepper"
worker:
  concurrency: 1
  retry_attempts: 0
  shutdown_grace_seconds: 1
health:
  live_path: "/livez"
  ready_path: "/readyz"
  dependency_timeout_ms: 1
"#
}

fn write_temp_config(contents: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time is after unix epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("proofplane-integration-config-{nanos}.yaml"));

    fs::write(&path, contents).expect("temp config is written");

    path
}
