# Unscanned uploads land here and never stay. The worker copies a clean document
# into the evidence bucket and deletes the source. Age removes what the worker
# did not: a document a scan condemned, or an upload nobody finished. Soft
# delete is off, so condemned bytes do not survive as deleted generations.
resource "google_storage_bucket" "quarantine" {
  name                        = var.quarantine_bucket_name
  project                     = var.project_id
  location                    = var.region
  storage_class               = "STANDARD"
  uniform_bucket_level_access = true
  public_access_prevention    = "enforced"
  force_destroy               = false
  labels                      = merge(local.labels, { component = "quarantine" })

  soft_delete_policy {
    retention_duration_seconds = 0
  }

  lifecycle_rule {
    condition {
      age = var.quarantine_retention_days
    }
    action {
      type = "Delete"
    }
  }

  depends_on = [google_project_service.required]
}

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

# Every runtime writes and deletes quarantine objects: the API and the MCP server
# stage uploads and clean up abandoned ones, and the worker reads and removes
# them after a scan.
resource "google_storage_bucket_iam_member" "quarantine_runtime" {
  for_each = toset(["api", "mcp", "worker"])

  bucket = google_storage_bucket.quarantine.name
  role   = "roles/storage.objectUser"
  member = "serviceAccount:${google_service_account.workload[each.value].email}"
}

# The API redeems download grants and streams finalized bytes, so it reads
# evidence and never writes it. The MCP server only mints grant URLs that the API
# redeems, so it reaches this bucket not at all.
resource "google_storage_bucket_iam_member" "evidence_api_reader" {
  bucket = google_storage_bucket.evidence.name
  role   = "roles/storage.objectViewer"
  member = "serviceAccount:${google_service_account.workload["api"].email}"
}

# The worker is the only writer. It rewrites a scanned object out of quarantine
# and deletes a destination whose metadata did not match the persisted document.
resource "google_storage_bucket_iam_member" "evidence_worker" {
  bucket = google_storage_bucket.evidence.name
  role   = "roles/storage.objectUser"
  member = "serviceAccount:${google_service_account.workload["worker"].email}"
}

# The worker keeps the binding it already had. Only the api and mcp bindings
# change, so only those are replaced.
moved {
  from = google_storage_bucket_iam_member.evidence_runtime["worker"]
  to   = google_storage_bucket_iam_member.evidence_worker
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

