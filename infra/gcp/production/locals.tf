locals {
  api_hostname = "api.${var.domain}"
  mcp_hostname = "mcp.${var.domain}"

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

  application_topic         = "proof.message_bus"
  dead_letter_topic         = "proof.message_bus.dead_letter"
  worker_subscription       = "proofplane-worker"
  dead_letter_subscription  = "proofplane-worker-dead-letter-inspection"
  migration_execution_token = var.app_image_digest == null ? null : substr(replace(split("@sha256:", var.app_image_digest)[1], "_", "-"), 0, 12)
  cloud_run_resource_count  = var.release_enabled ? 1 : 0
}

