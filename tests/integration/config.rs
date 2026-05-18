use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

static TEMP_CONFIG_COUNTER: AtomicU64 = AtomicU64::new(0);

#[test]
fn api_binary_starts_with_generated_config() {
    let output = run_binary("api", env!("CARGO_BIN_EXE_api"), valid_config("json"));

    assert_json_startup_log(&output.stderr, "api", "proofplane api scaffold ready");
}

#[test]
fn worker_binary_starts_with_generated_config() {
    let output = run_binary("worker", env!("CARGO_BIN_EXE_worker"), valid_config("json"));

    assert_json_startup_log(&output.stderr, "worker", "proofplane worker scaffold ready");
}

#[test]
fn mcp_binary_starts_with_generated_config() {
    let output = run_binary("mcp", env!("CARGO_BIN_EXE_mcp"), valid_config("json"));

    assert_json_startup_log(&output.stderr, "mcp", "proofplane mcp scaffold ready");
}

#[test]
fn seed_binary_starts_with_generated_config() {
    let output = run_binary("seed", env!("CARGO_BIN_EXE_seed"), valid_config("json"));

    assert_json_startup_log(
        &output.stderr,
        "seed",
        "proofplane migration scaffold ready",
    );
    assert_json_startup_log(&output.stderr, "seed", "proofplane seed scaffold ready");
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

#[test]
fn rust_log_off_suppresses_startup_info_logs() {
    let config_path = write_temp_config(valid_config("json"));

    let output = Command::new(env!("CARGO_BIN_EXE_api"))
        .env("PROOFPLANE_CONFIG", &config_path)
        .env("RUST_LOG", "off")
        .output()
        .expect("api binary runs");

    assert!(
        output.status.success(),
        "api stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).trim().is_empty(),
        "api stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "api stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );

    let _ = fs::remove_file(config_path);
}

fn run_binary(name: &str, binary: &str, config: String) -> std::process::Output {
    let config_path = write_temp_config(config);

    let output = Command::new(binary)
        .env("PROOFPLANE_CONFIG", &config_path)
        .env_remove("RUST_LOG")
        .output()
        .unwrap_or_else(|error| panic!("{name} binary runs: {error}"));

    assert!(
        output.status.success(),
        "{name} stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "{name} stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );

    let _ = fs::remove_file(config_path);

    output
}

fn assert_json_startup_log(stderr: &[u8], binary: &str, message: &str) {
    let stderr = String::from_utf8_lossy(stderr);
    let logs = stderr
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("log line is json"))
        .collect::<Vec<_>>();

    assert!(
        logs.iter().any(|log| {
            log["level"] == "INFO"
                && log["fields"]["binary"] == binary
                && log["fields"]["message"] == message
                && log["fields"]["version"].as_str().is_some()
        }),
        "missing startup log for {binary}: {stderr}"
    );
    assert!(!stderr.contains("proofplane-local-password"));
    assert!(!stderr.contains("pepper"));
}

fn valid_config(log_format: &str) -> String {
    format!(
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
  log_format: "{log_format}"
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
    )
}

fn write_temp_config(contents: impl AsRef<str>) -> std::path::PathBuf {
    let suffix = TEMP_CONFIG_COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "proofplane-integration-config-{}-{suffix}.yaml",
        std::process::id()
    ));

    fs::write(&path, contents.as_ref()).expect("temp config is written");

    path
}
