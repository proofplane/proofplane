resource "google_pubsub_subscription" "worker" {
  project                    = var.project_id
  name                       = local.worker_subscription
  topic                      = local.application_topic_id
  ack_deadline_seconds       = 600
  message_retention_duration = "604800s"
  retain_acked_messages      = false
  deletion_policy            = "PREVENT"
  labels                     = merge(local.labels, { component = "worker-delivery" })

  expiration_policy {
    ttl = ""
  }

  retry_policy {
    minimum_backoff = "10s"
    maximum_backoff = "600s"
  }

  dead_letter_policy {
    dead_letter_topic     = local.dead_letter_topic_id
    max_delivery_attempts = 5
  }

  push_config {
    push_endpoint = "${google_cloud_run_v2_service.worker.uri}/pubsub/messages"

    attributes = {
      x-goog-version = "v1"
    }

    oidc_token {
      service_account_email = local.service_account_emails["pubsub_push"]
      audience              = google_cloud_run_v2_service.worker.uri
    }
  }

  depends_on = [
    google_cloud_run_v2_service_iam_member.worker_push_invoker,
  ]
}

resource "google_pubsub_subscription" "dead_letter_inspection" {
  project                    = var.project_id
  name                       = local.dead_letter_subscription
  topic                      = local.dead_letter_topic_id
  ack_deadline_seconds       = 60
  message_retention_duration = "2678400s"
  retain_acked_messages      = false
  deletion_policy            = "PREVENT"
  labels                     = merge(local.labels, { component = "dead-letter-inspection" })

  expiration_policy {
    ttl = ""
  }
}

resource "google_pubsub_subscription_iam_member" "dead_letter_acknowledger" {
  project      = var.project_id
  subscription = google_pubsub_subscription.worker.name
  role         = "roles/pubsub.subscriber"
  member       = "serviceAccount:${local.pubsub_service_identity_email}"
}
