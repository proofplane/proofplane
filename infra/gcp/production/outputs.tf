output "artifact_registry_repository" {
  description = "Regional Docker repository prefix used by local image pushes."
  value       = "${var.region}-docker.pkg.dev/${var.project_id}/${google_artifact_registry_repository.images.repository_id}"
}

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

output "edge_ipv4_address" {
  description = "Global load-balancer address when the release is enabled."
  value       = var.release_enabled ? google_compute_global_address.edge[0].address : null
}

output "public_endpoints" {
  description = "Public TLS endpoints when the release is enabled."
  value = var.release_enabled ? {
    api = "https://${local.api_hostname}"
    mcp = "https://${local.mcp_hostname}"
  } : null
}

output "worker_push_endpoint" {
  description = "IAM-protected endpoint configured on the Pub/Sub push subscription."
  value       = var.release_enabled ? "${google_cloud_run_v2_service.worker[0].uri}/pubsub/messages" : null
}

output "deployed_application_digest" {
  description = "Immutable Proofplane release selected by this state."
  value       = var.release_enabled ? var.app_image_digest : null
}

