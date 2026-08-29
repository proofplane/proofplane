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

resource "google_pubsub_topic_iam_member" "dead_letter_forwarder" {
  project = var.project_id
  topic   = google_pubsub_topic.dead_letter.name
  role    = "roles/pubsub.publisher"
  member  = "serviceAccount:${google_project_service_identity.pubsub.email}"
}
