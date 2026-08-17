resource "google_monitoring_alert_policy" "service_5xx" {
  project      = var.project_id
  display_name = "Proofplane Cloud Run 5xx responses"
  combiner     = "OR"
  enabled      = true

  conditions {
    display_name = "Any service returns 5xx responses"

    condition_threshold {
      filter          = "resource.type = \"cloud_run_revision\" AND metric.type = \"run.googleapis.com/request_count\" AND metric.labels.response_code_class = \"5xx\""
      comparison      = "COMPARISON_GT"
      threshold_value = 0
      duration        = "0s"

      aggregations {
        alignment_period   = "300s"
        per_series_aligner = "ALIGN_RATE"
      }
    }
  }

  alert_strategy {
    auto_close = "1800s"
  }

  notification_channels = [local.notification_channel]
  user_labels           = local.labels
}

resource "google_monitoring_alert_policy" "unhealthy_startup_probe" {
  project      = var.project_id
  display_name = "Proofplane Cloud Run startup probe failures"
  combiner     = "OR"
  enabled      = true

  conditions {
    display_name = "A startup probe reports unhealthy"

    condition_threshold {
      filter          = "resource.type = \"cloud_run_revision\" AND metric.type = \"run.googleapis.com/container/completed_probe_attempt_count\" AND metric.labels.probe_type = \"STARTUP\" AND metric.labels.is_healthy = false"
      comparison      = "COMPARISON_GT"
      threshold_value = 0
      duration        = "0s"

      aggregations {
        alignment_period   = "300s"
        per_series_aligner = "ALIGN_DELTA"
      }
    }
  }

  alert_strategy {
    auto_close = "1800s"
  }

  notification_channels = [local.notification_channel]
  user_labels           = local.labels
}

resource "google_monitoring_alert_policy" "migration_failure" {
  project      = var.project_id
  display_name = "Proofplane migration job failed"
  combiner     = "OR"
  enabled      = true

  conditions {
    display_name = "Migration execution result is failed"

    condition_threshold {
      filter          = "resource.type = \"cloud_run_job\" AND resource.labels.job_name = \"${local.service_names.migrate}\" AND metric.type = \"run.googleapis.com/job/completed_execution_count\" AND metric.labels.result = \"failed\""
      comparison      = "COMPARISON_GT"
      threshold_value = 0
      duration        = "0s"

      aggregations {
        alignment_period   = "300s"
        per_series_aligner = "ALIGN_DELTA"
      }
    }
  }

  notification_channels = [local.notification_channel]
  user_labels           = local.labels
}

resource "google_monitoring_alert_policy" "clamav_update_failures" {
  project      = var.project_id
  display_name = "Proofplane ClamAV updater failed twice"
  combiner     = "OR"
  enabled      = true

  conditions {
    display_name = "Two failed updater executions in eight hours"

    condition_threshold {
      filter          = "resource.type = \"cloud_run_job\" AND resource.labels.job_name = \"proofplane-clamav-update\" AND metric.type = \"run.googleapis.com/job/completed_execution_count\" AND metric.labels.result = \"failed\""
      comparison      = "COMPARISON_GT"
      threshold_value = 1
      duration        = "0s"

      aggregations {
        alignment_period   = "28800s"
        per_series_aligner = "ALIGN_SUM"
      }
    }
  }

  notification_channels = [local.notification_channel]
  user_labels           = local.labels
}

resource "google_monitoring_alert_policy" "pubsub_backlog" {
  project      = var.project_id
  display_name = "Proofplane worker Pub/Sub backlog age"
  combiner     = "OR"
  enabled      = true

  conditions {
    display_name = "Oldest worker message exceeds 10 minutes"

    condition_threshold {
      filter          = "resource.type = \"pubsub_subscription\" AND resource.labels.subscription_id = \"${local.worker_subscription}\" AND metric.type = \"pubsub.googleapis.com/subscription/oldest_unacked_message_age\""
      comparison      = "COMPARISON_GT"
      threshold_value = 600
      duration        = "300s"

      aggregations {
        alignment_period   = "60s"
        per_series_aligner = "ALIGN_MAX"
      }
    }
  }

  notification_channels = [local.notification_channel]
  user_labels           = local.labels
}

resource "google_monitoring_alert_policy" "dead_letter" {
  project      = var.project_id
  display_name = "Proofplane dead-letter message observed"
  combiner     = "OR"
  enabled      = true

  conditions {
    display_name = "Pub/Sub forwarded any dead-letter message"

    condition_threshold {
      filter          = "resource.type = \"pubsub_subscription\" AND resource.labels.subscription_id = \"${local.worker_subscription}\" AND metric.type = \"pubsub.googleapis.com/subscription/dead_letter_message_count\""
      comparison      = "COMPARISON_GT"
      threshold_value = 0
      duration        = "0s"

      aggregations {
        alignment_period   = "300s"
        per_series_aligner = "ALIGN_DELTA"
      }
    }
  }

  notification_channels = [local.notification_channel]
  user_labels           = local.labels
}

resource "google_monitoring_alert_policy" "dequeuer_instance_count" {
  project      = var.project_id
  display_name = "Proofplane dequeuer worker pool has no instance"
  combiner     = "OR"
  enabled      = true

  conditions {
    display_name = "Dequeuer instance count below one"

    condition_threshold {
      filter          = "resource.type = \"cloud_run_worker_pool\" AND resource.labels.worker_pool_name = \"${local.service_names.dequeuer}\" AND metric.type = \"run.googleapis.com/container/instance_count\""
      comparison      = "COMPARISON_LT"
      threshold_value = 1
      duration        = "300s"

      aggregations {
        alignment_period   = "60s"
        per_series_aligner = "ALIGN_SUM"
      }

      evaluation_missing_data = "EVALUATION_MISSING_DATA_ACTIVE"
    }
  }

  notification_channels = [local.notification_channel]
  user_labels           = local.labels
}

resource "google_monitoring_alert_policy" "service_cpu" {
  project      = var.project_id
  display_name = "Proofplane Cloud Run CPU above 80 percent"
  combiner     = "OR"
  enabled      = true

  conditions {
    display_name = "Service CPU p99 exceeds 80 percent"

    condition_threshold {
      filter          = "resource.type = \"cloud_run_revision\" AND metric.type = \"run.googleapis.com/container/cpu/utilizations\""
      comparison      = "COMPARISON_GT"
      threshold_value = 0.8
      duration        = "300s"

      aggregations {
        alignment_period   = "60s"
        per_series_aligner = "ALIGN_PERCENTILE_99"
      }
    }
  }

  conditions {
    display_name = "Dequeuer CPU p99 exceeds 80 percent"

    condition_threshold {
      filter          = "resource.type = \"cloud_run_worker_pool\" AND resource.labels.worker_pool_name = \"${local.service_names.dequeuer}\" AND metric.type = \"run.googleapis.com/container/cpu/utilizations\""
      comparison      = "COMPARISON_GT"
      threshold_value = 0.8
      duration        = "300s"

      aggregations {
        alignment_period   = "60s"
        per_series_aligner = "ALIGN_PERCENTILE_99"
      }
    }
  }

  notification_channels = [local.notification_channel]
  user_labels           = local.labels
}

resource "google_monitoring_alert_policy" "service_memory" {
  project      = var.project_id
  display_name = "Proofplane Cloud Run memory above 80 percent"
  combiner     = "OR"
  enabled      = true

  conditions {
    display_name = "Service memory p99 exceeds 80 percent"

    condition_threshold {
      filter          = "resource.type = \"cloud_run_revision\" AND metric.type = \"run.googleapis.com/container/memory/utilizations\""
      comparison      = "COMPARISON_GT"
      threshold_value = 0.8
      duration        = "300s"

      aggregations {
        alignment_period   = "60s"
        per_series_aligner = "ALIGN_PERCENTILE_99"
      }
    }
  }

  conditions {
    display_name = "Dequeuer memory p99 exceeds 80 percent"

    condition_threshold {
      filter          = "resource.type = \"cloud_run_worker_pool\" AND resource.labels.worker_pool_name = \"${local.service_names.dequeuer}\" AND metric.type = \"run.googleapis.com/container/memory/utilizations\""
      comparison      = "COMPARISON_GT"
      threshold_value = 0.8
      duration        = "300s"

      aggregations {
        alignment_period   = "60s"
        per_series_aligner = "ALIGN_PERCENTILE_99"
      }
    }
  }

  notification_channels = [local.notification_channel]
  user_labels           = local.labels
}

resource "google_logging_metric" "clamav_stale_snapshot" {
  project     = var.project_id
  name        = "proofplane_clamav_stale_snapshot"
  description = "Worker refused startup because the last-good ClamAV snapshot was stale."
  filter      = "resource.type=\"cloud_run_revision\" resource.labels.service_name=\"${local.service_names.worker}\" (jsonPayload.event=\"clamav_definitions_stale\" OR textPayload:\"ClamAV definitions are stale\")"

  metric_descriptor {
    metric_kind = "DELTA"
    value_type  = "INT64"
    unit        = "1"
  }
}

resource "google_monitoring_alert_policy" "clamav_stale_snapshot" {
  project      = var.project_id
  display_name = "Proofplane ClamAV definitions are stale"
  combiner     = "OR"
  enabled      = true

  conditions {
    display_name = "A worker rejected a stale last-good snapshot"

    condition_threshold {
      filter          = "resource.type = \"cloud_run_revision\" AND metric.type = \"logging.googleapis.com/user/${google_logging_metric.clamav_stale_snapshot.name}\""
      comparison      = "COMPARISON_GT"
      threshold_value = 0
      duration        = "0s"

      aggregations {
        alignment_period   = "300s"
        per_series_aligner = "ALIGN_DELTA"
      }
    }
  }

  notification_channels = [local.notification_channel]
  user_labels           = local.labels
}
