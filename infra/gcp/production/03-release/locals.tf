locals {
  api_hostname = "api.${local.domain}"
  mcp_hostname = "mcp.${local.domain}"

  labels = merge({
    application = "proofplane"
    environment = "production"
    managed-by  = "terraform"
  }, var.labels)

  service_names = {
    api      = "proofplane-api"
    mcp      = "proofplane-mcp"
    worker   = "proofplane-worker"
    dequeuer = "proofplane-dequeuer"
    migrate  = "proofplane-migrate"
  }

  runtime_config_mount_path = "/var/run/secrets/proofplane"
  runtime_config_file       = "${local.runtime_config_mount_path}/config.yaml"
  migration_secret_path     = "/var/run/secrets/proofplane-migrate"

  migration_database_root_certificate = (
    var.migration_database_root_certificate_file == ""
    ? ""
    : file(var.migration_database_root_certificate_file)
  )

  worker_subscription      = "proofplane-worker"
  dead_letter_subscription = "proofplane-worker-dead-letter-inspection"

  # The execution name ends with this token, so a digest that already migrated
  # names an execution that already exists and starts no new one.
  migration_execution_token = substr(replace(split("@sha256:", var.app_image_digest)[1], "_", "-"), 0, 12)
}
