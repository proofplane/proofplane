resource "google_monitoring_notification_channel" "operator_email" {
  project      = var.project_id
  display_name = "Proofplane production operator"
  type         = "email"
  enabled      = true
  force_delete = false

  labels = {
    email_address = var.alert_email
  }

  user_labels = local.labels

  depends_on = [google_project_service.required]
}

resource "google_billing_budget" "production" {
  billing_account = var.billing_account
  display_name    = "Proofplane production monthly budget"

  budget_filter {
    projects               = ["projects/${data.google_project.current.number}"]
    credit_types_treatment = "INCLUDE_ALL_CREDITS"
  }

  amount {
    specified_amount {
      currency_code = "USD"
      units         = tostring(var.budget_amount)
    }
  }

  threshold_rules {
    threshold_percent = 0.5
    spend_basis       = "CURRENT_SPEND"
  }

  threshold_rules {
    threshold_percent = 0.8
    spend_basis       = "CURRENT_SPEND"
  }

  threshold_rules {
    threshold_percent = 1.0
    spend_basis       = "CURRENT_SPEND"
  }

  all_updates_rule {
    monitoring_notification_channels = [google_monitoring_notification_channel.operator_email.name]
    disable_default_iam_recipients   = false
    enable_project_level_recipients  = true
  }

  depends_on = [google_project_service.required]
}
