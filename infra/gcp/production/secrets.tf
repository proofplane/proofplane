resource "google_secret_manager_secret" "runtime_config" {
  project   = var.project_id
  secret_id = "proofplane-production-config"
  labels    = local.labels

  replication {
    auto {}
  }

  depends_on = [google_project_service.required]
}

resource "google_secret_manager_secret" "migration_database_url" {
  project   = var.project_id
  secret_id = "proofplane-production-migration-database-url"
  labels    = local.labels

  replication {
    auto {}
  }

  depends_on = [google_project_service.required]
}

resource "google_secret_manager_secret_iam_member" "runtime_config" {
  for_each = toset(["api", "mcp", "worker", "dequeuer"])

  project   = var.project_id
  secret_id = google_secret_manager_secret.runtime_config.secret_id
  role      = "roles/secretmanager.secretAccessor"
  member    = "serviceAccount:${google_service_account.workload[each.value].email}"
}

resource "google_secret_manager_secret_iam_member" "migration_database_url" {
  project   = var.project_id
  secret_id = google_secret_manager_secret.migration_database_url.secret_id
  role      = "roles/secretmanager.secretAccessor"
  member    = "serviceAccount:${google_service_account.workload["migration"].email}"
}

