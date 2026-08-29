output "artifact_registry_repository" {
  description = "Regional Docker repository prefix used by local image pushes."
  value       = "${var.region}-docker.pkg.dev/${var.project_id}/${google_artifact_registry_repository.images.repository_id}"
}
