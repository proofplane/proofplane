output "edge_ipv4_address" {
  description = "Global load-balancer address."
  value       = google_compute_global_address.edge.address
}

output "public_endpoints" {
  description = "Public TLS endpoints."
  value = {
    api = "https://${local.api_hostname}"
    mcp = "https://${local.mcp_hostname}"
  }
}

output "worker_push_endpoint" {
  description = "IAM-protected endpoint configured on the Pub/Sub push subscription."
  value       = "${google_cloud_run_v2_service.worker.uri}/pubsub/messages"
}

output "deployed_application_digest" {
  description = "Immutable Proofplane release selected by this state."
  value       = var.app_image_digest
}
