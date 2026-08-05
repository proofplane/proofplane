locals {
  # This root runs before every other one, so it owns the two services that
  # enabling anything else depends on, as well as the two the repository needs.
  # The remaining services belong to 02-foundation, which applies later.
  required_services = toset([
    "artifactregistry.googleapis.com",
    "cloudresourcemanager.googleapis.com",
    "containerscanning.googleapis.com",
    "serviceusage.googleapis.com",
  ])
}

resource "google_project_service" "required" {
  for_each = local.required_services

  project            = var.project_id
  service            = each.value
  disable_on_destroy = false
}
