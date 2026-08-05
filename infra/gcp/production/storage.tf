resource "google_storage_bucket" "evidence" {
  name                        = var.evidence_bucket_name
  project                     = var.project_id
  location                    = var.region
  storage_class               = "STANDARD"
  uniform_bucket_level_access = true
  public_access_prevention    = "enforced"
  force_destroy               = false
  labels                      = merge(local.labels, { component = "evidence" })

  soft_delete_policy {
    retention_duration_seconds = 2592000
  }

  lifecycle {
    prevent_destroy = true
  }

  depends_on = [google_project_service.required]
}

resource "google_storage_bucket" "clamav_definitions" {
  name                        = var.clamav_definitions_bucket_name
  project                     = var.project_id
  location                    = var.region
  storage_class               = "STANDARD"
  uniform_bucket_level_access = true
  public_access_prevention    = "enforced"
  force_destroy               = false
  labels                      = merge(local.labels, { component = "clamav-definitions" })

  soft_delete_policy {
    retention_duration_seconds = 604800
  }

  lifecycle_rule {
    condition {
      age = 7
    }
    action {
      type = "Delete"
    }
  }

  lifecycle {
    prevent_destroy = true
  }

  depends_on = [google_project_service.required]
}

resource "google_storage_bucket_iam_member" "evidence_runtime" {
  for_each = toset(["api", "mcp", "worker"])

  bucket = google_storage_bucket.evidence.name
  role   = "roles/storage.objectUser"
  member = "serviceAccount:${google_service_account.workload[each.value].email}"
}

resource "google_storage_bucket_iam_member" "clamav_worker_reader" {
  bucket = google_storage_bucket.clamav_definitions.name
  role   = "roles/storage.objectViewer"
  member = "serviceAccount:${google_service_account.workload["worker"].email}"
}

resource "google_storage_bucket_iam_member" "clamav_updater_writer" {
  bucket = google_storage_bucket.clamav_definitions.name
  role   = "roles/storage.objectAdmin"
  member = "serviceAccount:${google_service_account.workload["clamav_updater"].email}"
}

