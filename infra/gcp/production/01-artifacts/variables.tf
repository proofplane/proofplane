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

variable "artifact_repository" {
  description = "Regional Artifact Registry Docker repository name."
  type        = string
  default     = "proofplane"
}

variable "labels" {
  description = "Additional labels merged into managed resources that support labels."
  type        = map(string)
  default     = {}
}
