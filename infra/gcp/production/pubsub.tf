resource "google_pubsub_topic" "application" {
  project         = var.project_id
  name            = local.application_topic
  labels          = merge(local.labels, { component = "message-bus" })
  deletion_policy = "PREVENT"

  depends_on = [google_project_service.required]
}

resource "google_pubsub_topic" "dead_letter" {
  project         = var.project_id
  name            = local.dead_letter_topic
  labels          = merge(local.labels, { component = "dead-letter" })
  deletion_policy = "PREVENT"

  depends_on = [google_project_service.required]
}

resource "google_pubsub_topic_iam_member" "dequeuer_publisher" {
  project = var.project_id
  topic   = google_pubsub_topic.application.name
  role    = "roles/pubsub.publisher"
  member  = "serviceAccount:${google_service_account.workload["dequeuer"].email}"
}

resource "google_pubsub_subscription" "worker" {
  count = local.cloud_run_resource_count

  project                    = var.project_id
  name                       = local.worker_subscription
  topic                      = google_pubsub_topic.application.id
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
    dead_letter_topic     = google_pubsub_topic.dead_letter.id
    max_delivery_attempts = 5
  }

  push_config {
    push_endpoint = "${google_cloud_run_v2_service.worker[0].uri}/pubsub/messages"

    attributes = {
      x-goog-version = "v1"
    }

    oidc_token {
      service_account_email = google_service_account.workload["pubsub_push"].email
      audience              = google_cloud_run_v2_service.worker[0].uri
    }
  }

  depends_on = [
    google_cloud_run_v2_service_iam_member.worker_push_invoker,
    google_service_account_iam_member.pubsub_push_token_creator,
  ]
}

resource "google_pubsub_subscription" "dead_letter_inspection" {
  count = local.cloud_run_resource_count

  project                    = var.project_id
  name                       = local.dead_letter_subscription
  topic                      = google_pubsub_topic.dead_letter.id
  ack_deadline_seconds       = 60
  message_retention_duration = "2678400s"
  retain_acked_messages      = false
  deletion_policy            = "PREVENT"
  labels                     = merge(local.labels, { component = "dead-letter-inspection" })

  expiration_policy {
    ttl = ""
  }
}

resource "google_pubsub_topic_iam_member" "dead_letter_forwarder" {
  project = var.project_id
  topic   = google_pubsub_topic.dead_letter.name
  role    = "roles/pubsub.publisher"
  member  = "serviceAccount:${google_project_service_identity.pubsub.email}"
}

resource "google_pubsub_subscription_iam_member" "dead_letter_acknowledger" {
  count = local.cloud_run_resource_count

  project      = var.project_id
  subscription = google_pubsub_subscription.worker[0].name
  role         = "roles/pubsub.subscriber"
  member       = "serviceAccount:${google_project_service_identity.pubsub.email}"
}
