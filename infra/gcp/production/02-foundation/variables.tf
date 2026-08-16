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

variable "evidence_bucket_name" {
  description = "Globally unique evidence bucket name."
  type        = string
}

variable "clamav_definitions_bucket_name" {
  description = "Globally unique bucket name for validated ClamAV snapshots."
  type        = string
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
