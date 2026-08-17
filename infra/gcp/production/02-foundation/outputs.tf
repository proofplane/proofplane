output "runtime_config_secret" {
  description = "Secret container whose payload versions are created outside Terraform."
  value       = google_secret_manager_secret.runtime_config.id
}

output "migration_database_url_secret" {
  description = "Migration credential secret container; payload versions are created outside Terraform."
  value       = google_secret_manager_secret.migration_database_url.id
}

output "cloud_dns_name_servers" {
  description = "Replace the registrar delegation with these nameservers after zone validation."
  value       = google_dns_managed_zone.primary.name_servers
}

# 03-release reads everything below through terraform_remote_state. Removing one
# breaks that root's plan, so treat them as a published contract rather than
# operator conveniences.

output "service_account_emails" {
  description = "Workload service account emails, keyed by role."
  value       = { for key, account in google_service_account.workload : key => account.email }
}

output "pubsub_service_identity_email" {
  description = "Pub/Sub service agent that mints push OIDC tokens and operates the dead-letter policy."
  value       = google_project_service_identity.pubsub.email
}

output "application_topic_id" {
  description = "Application topic the worker subscription attaches to."
  value       = google_pubsub_topic.application.id
}

output "dead_letter_topic_id" {
  description = "Dead-letter topic used by the worker subscription policy."
  value       = google_pubsub_topic.dead_letter.id
}

output "runtime_config_secret_name" {
  description = "Short secret name Cloud Run mounts as the runtime configuration volume."
  value       = google_secret_manager_secret.runtime_config.secret_id
}

output "migration_database_url_secret_name" {
  description = "Short secret name the migration job mounts."
  value       = google_secret_manager_secret.migration_database_url.secret_id
}

output "clamav_definitions_bucket" {
  description = "Bucket holding validated ClamAV snapshots and the last-good pointer."
  value       = google_storage_bucket.clamav_definitions.name
}

output "dns_zone_name" {
  description = "Managed zone that release records are written into."
  value       = google_dns_managed_zone.primary.name
}

output "domain" {
  description = "Apex this zone is authoritative for. 03-release builds its hostnames from it."
  value       = var.domain
}

output "notification_channel" {
  description = "Notification channel every release alert policy sends to."
  value       = google_monitoring_notification_channel.operator_email.name
}
