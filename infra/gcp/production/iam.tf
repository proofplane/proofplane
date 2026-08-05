locals {
  service_accounts = {
    api = {
      account_id   = "proofplane-api"
      display_name = "Proofplane production API"
    }
    mcp = {
      account_id   = "proofplane-mcp"
      display_name = "Proofplane production MCP"
    }
    worker = {
      account_id   = "proofplane-worker"
      display_name = "Proofplane production worker"
    }
    dequeuer = {
      account_id   = "proofplane-dequeuer"
      display_name = "Proofplane production dequeuer"
    }
    migration = {
      account_id   = "proofplane-migration"
      display_name = "Proofplane production migration job"
    }
    clamav_updater = {
      account_id   = "proofplane-clamav-updater"
      display_name = "Proofplane ClamAV definition updater"
    }
    pubsub_push = {
      account_id   = "proofplane-pubsub-push"
      display_name = "Proofplane Pub/Sub push identity"
    }
    clamav_scheduler = {
      account_id   = "proofplane-clamav-scheduler"
      display_name = "Proofplane ClamAV scheduler identity"
    }
  }
}

resource "google_service_account" "workload" {
  for_each = local.service_accounts

  project      = var.project_id
  account_id   = each.value.account_id
  display_name = each.value.display_name
}

resource "google_service_account_iam_member" "pubsub_push_token_creator" {
  service_account_id = google_service_account.workload["pubsub_push"].name
  role               = "roles/iam.serviceAccountTokenCreator"
  member             = "serviceAccount:${google_project_service_identity.pubsub.email}"
}

