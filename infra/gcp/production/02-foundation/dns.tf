# The zone is the one edge resource that must exist before a release. Cloud DNS
# delegation is a registrar-level change with its own propagation window, so it
# is deliberately not tied to a release. 03-release adds the records.
resource "google_dns_managed_zone" "primary" {
  project     = var.project_id
  name        = var.dns_zone_name
  dns_name    = "${var.domain}."
  description = "Authoritative production zone for ${var.domain}"
  labels      = local.labels

  lifecycle {
    prevent_destroy = true
  }

  depends_on = [google_project_service.required]
}
