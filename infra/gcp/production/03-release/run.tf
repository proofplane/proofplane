resource "google_cloud_run_v2_job" "migration" {
  provider            = google-beta
  project             = var.project_id
  name                = local.service_names.migrate
  location            = var.region
  deletion_protection = true
  labels              = merge(local.labels, { component = "migration" })
  run_execution_token = local.migration_execution_token

  template {
    task_count  = 1
    parallelism = 1

    template {
      service_account = local.service_account_emails["migration"]
      max_retries     = 0
      timeout         = "900s"

      containers {
        name    = "migrate"
        image   = var.app_image_digest
        command = ["/usr/local/bin/migrate"]

        env {
          name  = "PROOFPLANE_MIGRATION_DATABASE_URL_FILE"
          value = "${local.migration_secret_path}/database-url"
        }

        resources {
          limits = {
            cpu    = "1"
            memory = "512Mi"
          }
        }

        volume_mounts {
          name       = "migration-database"
          mount_path = local.migration_secret_path
        }
      }

      volumes {
        name = "migration-database"

        secret {
          secret = local.migration_database_url_secret_name

          items {
            version = var.migration_database_secret_version
            path    = "database-url"
            mode    = 292
          }
        }
      }
    }
  }
}

resource "google_cloud_run_v2_service" "public" {
  for_each = {
    api = {
      name = local.service_names.api
      port = 3000
    }
    mcp = {
      name = local.service_names.mcp
      port = 3002
    }
  }

  project              = var.project_id
  name                 = each.value.name
  location             = var.region
  ingress              = "INGRESS_TRAFFIC_INTERNAL_LOAD_BALANCER"
  invoker_iam_disabled = true
  default_uri_disabled = true
  deletion_protection  = true
  labels               = merge(local.labels, { component = each.key })

  template {
    service_account                  = local.service_account_emails[each.key]
    execution_environment            = "EXECUTION_ENVIRONMENT_GEN2"
    timeout                          = "300s"
    max_instance_request_concurrency = 20

    scaling {
      min_instance_count = 0
      max_instance_count = 3
    }

    containers {
      name    = each.key
      image   = var.app_image_digest
      command = ["/usr/local/bin/${each.key}"]

      ports {
        name           = "http1"
        container_port = each.value.port
      }

      env {
        name  = "PROOFPLANE_CONFIG"
        value = local.runtime_config_file
      }

      resources {
        limits = {
          cpu    = "1"
          memory = "512Mi"
        }
        cpu_idle          = true
        startup_cpu_boost = true
      }

      startup_probe {
        initial_delay_seconds = 0
        timeout_seconds       = 2
        period_seconds        = 2
        failure_threshold     = 30

        tcp_socket {
          port = each.value.port
        }
      }

      volume_mounts {
        name       = "runtime-config"
        mount_path = local.runtime_config_mount_path
      }
    }

    volumes {
      name = "runtime-config"

      secret {
        secret = local.runtime_config_secret_name

        items {
          version = var.runtime_config_secret_version
          path    = "config.yaml"
          mode    = 292
        }
      }
    }
  }

  depends_on = [
    google_cloud_run_v2_job.migration,
  ]
}

resource "google_cloud_run_v2_service" "worker" {
  provider             = google-beta
  project              = var.project_id
  name                 = local.service_names.worker
  location             = var.region
  ingress              = "INGRESS_TRAFFIC_INTERNAL_ONLY"
  default_uri_disabled = false
  deletion_protection  = true
  labels               = merge(local.labels, { component = "worker" })

  template {
    service_account                  = local.service_account_emails["worker"]
    execution_environment            = "EXECUTION_ENVIRONMENT_GEN2"
    timeout                          = "600s"
    max_instance_request_concurrency = 4

    scaling {
      min_instance_count = 0
      max_instance_count = 1
    }

    containers {
      name       = "worker"
      image      = var.app_image_digest
      command    = ["/usr/local/bin/worker"]
      depends_on = ["clamd"]

      ports {
        name           = "http1"
        container_port = 3001
      }

      env {
        name  = "PROOFPLANE_CONFIG"
        value = local.runtime_config_file
      }

      resources {
        limits = {
          cpu    = "1"
          memory = "1Gi"
        }
        cpu_idle          = true
        startup_cpu_boost = true
      }

      startup_probe {
        initial_delay_seconds = 0
        timeout_seconds       = 2
        period_seconds        = 2
        failure_threshold     = 60

        tcp_socket {
          port = 3001
        }
      }

      volume_mounts {
        name       = "runtime-config"
        mount_path = local.runtime_config_mount_path
      }
    }

    containers {
      name  = "clamd"
      image = var.clamav_image_digest

      env {
        name  = "CLAMAV_DEFINITIONS_BUCKET"
        value = local.clamav_definitions_bucket
      }

      env {
        name  = "CLAMAV_MAX_DEFINITION_AGE_SECONDS"
        value = "86400"
      }

      env {
        name  = "CLAMD_MAX_THREADS"
        value = "4"
      }

      resources {
        limits = {
          cpu    = "2"
          memory = "4Gi"
        }
        cpu_idle          = true
        startup_cpu_boost = true
      }

      startup_probe {
        initial_delay_seconds = 0
        timeout_seconds       = 2
        period_seconds        = 5
        failure_threshold     = 60

        tcp_socket {
          port = 3310
        }
      }
    }

    volumes {
      name = "runtime-config"

      secret {
        secret = local.runtime_config_secret_name

        items {
          version = var.runtime_config_secret_version
          path    = "config.yaml"
          mode    = 292
        }
      }
    }
  }

  depends_on = [
    google_cloud_run_v2_job.migration,
  ]
}

resource "google_cloud_run_v2_service_iam_member" "worker_push_invoker" {
  provider = google-beta
  project  = var.project_id
  location = var.region
  name     = google_cloud_run_v2_service.worker.name
  role     = "roles/run.invoker"
  member   = "serviceAccount:${local.service_account_emails["pubsub_push"]}"
}

resource "google_cloud_run_v2_worker_pool" "dequeuer" {
  provider            = google-beta
  project             = var.project_id
  name                = local.service_names.dequeuer
  location            = var.region
  deletion_protection = true
  labels              = merge(local.labels, { component = "dequeuer" })

  scaling {
    scaling_mode          = "MANUAL"
    manual_instance_count = 1
  }

  template {
    service_account = local.service_account_emails["dequeuer"]

    containers {
      name    = "dequeuer"
      image   = var.app_image_digest
      command = ["/usr/local/bin/dequeuer"]

      env {
        name  = "PROOFPLANE_CONFIG"
        value = local.runtime_config_file
      }

      resources {
        limits = {
          cpu    = "1"
          memory = "512Mi"
        }
      }

      volume_mounts {
        name       = "runtime-config"
        mount_path = local.runtime_config_mount_path
      }
    }

    volumes {
      name = "runtime-config"

      secret {
        secret = local.runtime_config_secret_name

        items {
          version = var.runtime_config_secret_version
          path    = "config.yaml"
          mode    = 292
        }
      }
    }
  }

  depends_on = [
    google_cloud_run_v2_job.migration,
  ]
}

resource "google_cloud_run_v2_job" "clamav_update" {
  provider            = google-beta
  project             = var.project_id
  name                = "proofplane-clamav-update"
  location            = var.region
  deletion_protection = true
  labels              = merge(local.labels, { component = "clamav-update" })

  template {
    task_count  = 1
    parallelism = 1

    template {
      service_account = local.service_account_emails["clamav_updater"]
      max_retries     = 0
      timeout         = "1800s"

      containers {
        name  = "clamav-update"
        image = var.clamav_updater_image_digest

        env {
          name  = "CLAMAV_DEFINITIONS_BUCKET"
          value = local.clamav_definitions_bucket
        }

        resources {
          limits = {
            cpu    = "1"
            memory = "1Gi"
          }
        }
      }
    }
  }
}

resource "google_cloud_run_v2_job_iam_member" "clamav_scheduler_invoker" {
  provider = google-beta
  project  = var.project_id
  location = var.region
  name     = google_cloud_run_v2_job.clamav_update.name
  role     = "roles/run.invoker"
  member   = "serviceAccount:${local.service_account_emails["clamav_scheduler"]}"
}

resource "google_cloud_scheduler_job" "clamav_update" {
  project          = var.project_id
  region           = var.region
  name             = "proofplane-clamav-update"
  description      = "Refresh and validate the central ClamAV definition snapshot."
  schedule         = "0 */4 * * *"
  time_zone        = "Etc/UTC"
  attempt_deadline = "1800s"

  retry_config {
    retry_count          = 1
    min_backoff_duration = "60s"
    max_backoff_duration = "600s"
    max_doublings        = 3
  }

  http_target {
    http_method = "POST"
    uri         = "https://run.googleapis.com/v2/projects/${var.project_id}/locations/${var.region}/jobs/${google_cloud_run_v2_job.clamav_update.name}:run"
    body        = base64encode("{}")

    headers = {
      "Content-Type" = "application/json"
    }

    oauth_token {
      service_account_email = local.service_account_emails["clamav_scheduler"]
      scope                 = "https://www.googleapis.com/auth/cloud-platform"
    }
  }

  depends_on = [google_cloud_run_v2_job_iam_member.clamav_scheduler_invoker]
}
