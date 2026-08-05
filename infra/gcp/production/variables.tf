variable "project_id" {
  description = "Pre-created production GCP project ID."
  type        = string
}

variable "billing_account" {
  description = "Billing account ID used only to create the budget."
  type        = string
}

variable "region" {
  description = "Production region."
  type        = string
  default     = "us-central1"

  validation {
    condition     = var.region == "us-central1"
    error_message = "The accepted production architecture uses us-central1."
  }
}

variable "domain" {
  description = "Authoritative production DNS apex."
  type        = string
  default     = "proofplane.app"
}

variable "dns_zone_name" {
  description = "Cloud DNS managed-zone resource name. Import the manually created zone under this name."
  type        = string
  default     = "proofplane-app"
}

variable "artifact_repository" {
  description = "Regional Artifact Registry Docker repository name."
  type        = string
  default     = "proofplane"
}

variable "evidence_bucket_name" {
  description = "Globally unique evidence bucket name."
  type        = string
}

variable "clamav_definitions_bucket_name" {
  description = "Globally unique bucket name for validated ClamAV snapshots."
  type        = string
}

variable "release_enabled" {
  description = "Create release-dependent Cloud Run, delivery, edge, scheduler, and alert resources. Enable only after all release gates and secret versions are ready."
  type        = bool
  default     = false
}

variable "app_image_digest" {
  description = "Full Artifact Registry Proofplane image reference pinned with @sha256."
  type        = string
  default     = null
  nullable    = true

  validation {
    condition = !var.release_enabled || (
      var.app_image_digest != null && can(regex("^[^[:space:]]+@sha256:[0-9a-f]{64}$", var.app_image_digest))
    )
    error_message = "When release_enabled is true, app_image_digest must be an immutable @sha256 reference."
  }
}

variable "clamav_image_digest" {
  description = "Full mirrored ClamAV worker image reference pinned with @sha256."
  type        = string
  default     = null
  nullable    = true

  validation {
    condition = !var.release_enabled || (
      var.clamav_image_digest != null && can(regex("^[^[:space:]]+@sha256:[0-9a-f]{64}$", var.clamav_image_digest))
    )
    error_message = "When release_enabled is true, clamav_image_digest must be an immutable @sha256 reference."
  }
}

variable "clamav_updater_image_digest" {
  description = "Full ClamAV definition-updater image reference pinned with @sha256."
  type        = string
  default     = null
  nullable    = true

  validation {
    condition = !var.release_enabled || (
      var.clamav_updater_image_digest != null && can(regex("^[^[:space:]]+@sha256:[0-9a-f]{64}$", var.clamav_updater_image_digest))
    )
    error_message = "When release_enabled is true, clamav_updater_image_digest must be an immutable @sha256 reference."
  }
}

variable "runtime_config_secret_version" {
  description = "Numeric Secret Manager version containing the complete production YAML. Never use latest."
  type        = string
  default     = null
  nullable    = true

  validation {
    condition     = !var.release_enabled || (var.runtime_config_secret_version != null && can(regex("^[1-9][0-9]*$", var.runtime_config_secret_version)))
    error_message = "When release_enabled is true, runtime_config_secret_version must be a numeric pinned version."
  }
}

variable "migration_database_secret_version" {
  description = "Numeric version containing the direct verified-TLS migration database URL."
  type        = string
  default     = null
  nullable    = true

  validation {
    condition     = !var.release_enabled || (var.migration_database_secret_version != null && can(regex("^[1-9][0-9]*$", var.migration_database_secret_version)))
    error_message = "When release_enabled is true, migration_database_secret_version must be a numeric pinned version."
  }
}

variable "alert_email" {
  description = "Operator email for Cloud Monitoring notifications."
  type        = string
}

variable "budget_amount" {
  description = "Monthly GCP budget in USD; notifications do not stop resources."
  type        = number
  default     = 100

  validation {
    condition     = var.budget_amount == 100
    error_message = "The accepted initial budget is USD 100."
  }
}

variable "labels" {
  description = "Additional labels merged into managed resources that support labels."
  type        = map(string)
  default     = {}
}

