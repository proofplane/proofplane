variable "project_id" {
  description = "Pre-created production GCP project ID."
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

# terraform_remote_state cannot read the partial backend config, so this root
# names the foundation state a second time. Both values must match what
# 02-foundation was initialized with, or this root reads a prefix that holds no
# state. The apex is not repeated here: it arrives as a foundation output, so a
# record can never name a different apex than the zone that holds it.
variable "state_bucket" {
  description = "State bucket holding the 02-foundation state this root reads."
  type        = string
}

variable "foundation_state_prefix" {
  description = "Prefix 02-foundation was initialized with. Change it only together with that root's TF_STATE_PREFIX."
  type        = string
  default     = "proofplane/production/foundation"
}

variable "app_image_digest" {
  description = "Full Artifact Registry Proofplane image reference pinned with @sha256."
  type        = string

  validation {
    condition     = can(regex("^[^[:space:]]+@sha256:[0-9a-f]{64}$", var.app_image_digest))
    error_message = "app_image_digest must be an immutable @sha256 reference."
  }
}

variable "clamav_image_digest" {
  description = "Full mirrored ClamAV worker image reference pinned with @sha256."
  type        = string

  validation {
    condition     = can(regex("^[^[:space:]]+@sha256:[0-9a-f]{64}$", var.clamav_image_digest))
    error_message = "clamav_image_digest must be an immutable @sha256 reference."
  }
}

variable "clamav_updater_image_digest" {
  description = "Full ClamAV definition-updater image reference pinned with @sha256."
  type        = string

  validation {
    condition     = can(regex("^[^[:space:]]+@sha256:[0-9a-f]{64}$", var.clamav_updater_image_digest))
    error_message = "clamav_updater_image_digest must be an immutable @sha256 reference."
  }
}

variable "runtime_config_secret_version" {
  description = "Numeric Secret Manager version containing the complete production YAML. Never use latest."
  type        = string

  validation {
    condition     = can(regex("^[1-9][0-9]*$", var.runtime_config_secret_version))
    error_message = "runtime_config_secret_version must be a numeric pinned version."
  }
}

variable "migration_database_secret_version" {
  description = "Numeric version containing the direct verified-TLS migration database URL."
  type        = string

  validation {
    condition     = can(regex("^[1-9][0-9]*$", var.migration_database_secret_version))
    error_message = "migration_database_secret_version must be a numeric pinned version."
  }
}

variable "labels" {
  description = "Additional labels merged into managed resources that support labels."
  type        = map(string)
  default     = {}
}
