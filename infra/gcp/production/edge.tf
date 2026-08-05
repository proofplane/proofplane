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

resource "google_compute_global_address" "edge" {
  count = local.cloud_run_resource_count

  project      = var.project_id
  name         = "proofplane-edge"
  address_type = "EXTERNAL"
  ip_version   = "IPV4"

  depends_on = [google_project_service.required]
}

resource "google_compute_region_network_endpoint_group" "api" {
  count = local.cloud_run_resource_count

  project               = var.project_id
  name                  = "proofplane-api"
  region                = var.region
  network_endpoint_type = "SERVERLESS"

  cloud_run {
    service = google_cloud_run_v2_service.public["api"].name
  }
}

resource "google_compute_region_network_endpoint_group" "mcp" {
  count = local.cloud_run_resource_count

  project               = var.project_id
  name                  = "proofplane-mcp"
  region                = var.region
  network_endpoint_type = "SERVERLESS"

  cloud_run {
    service = google_cloud_run_v2_service.public["mcp"].name
  }
}

resource "google_compute_backend_service" "api" {
  count = local.cloud_run_resource_count

  project               = var.project_id
  name                  = "proofplane-api"
  protocol              = "HTTP"
  port_name             = "http"
  load_balancing_scheme = "EXTERNAL_MANAGED"
  timeout_sec           = 600

  backend {
    group = google_compute_region_network_endpoint_group.api[0].id
  }
}

resource "google_compute_backend_service" "mcp" {
  count = local.cloud_run_resource_count

  project               = var.project_id
  name                  = "proofplane-mcp"
  protocol              = "HTTP"
  port_name             = "http"
  load_balancing_scheme = "EXTERNAL_MANAGED"
  timeout_sec           = 600

  backend {
    group = google_compute_region_network_endpoint_group.mcp[0].id
  }
}

resource "google_compute_url_map" "https" {
  count = local.cloud_run_resource_count

  project         = var.project_id
  name            = "proofplane-https"
  default_service = google_compute_backend_service.api[0].id

  host_rule {
    hosts        = [local.api_hostname]
    path_matcher = "api"
  }

  host_rule {
    hosts        = [local.mcp_hostname]
    path_matcher = "mcp"
  }

  path_matcher {
    name            = "api"
    default_service = google_compute_backend_service.api[0].id
  }

  path_matcher {
    name            = "mcp"
    default_service = google_compute_backend_service.mcp[0].id
  }
}

resource "google_compute_url_map" "http_redirect" {
  count = local.cloud_run_resource_count

  project = var.project_id
  name    = "proofplane-http-redirect"

  default_url_redirect {
    https_redirect = true
    strip_query    = false
  }
}

resource "google_certificate_manager_dns_authorization" "api" {
  count = local.cloud_run_resource_count

  project     = var.project_id
  name        = "proofplane-api"
  description = "DNS authorization for ${local.api_hostname}"
  domain      = local.api_hostname
  labels      = local.labels

  depends_on = [google_project_service.required]
}

resource "google_certificate_manager_dns_authorization" "mcp" {
  count = local.cloud_run_resource_count

  project     = var.project_id
  name        = "proofplane-mcp"
  description = "DNS authorization for ${local.mcp_hostname}"
  domain      = local.mcp_hostname
  labels      = local.labels

  depends_on = [google_project_service.required]
}

resource "google_dns_record_set" "api_certificate_authorization" {
  count = local.cloud_run_resource_count

  project      = var.project_id
  managed_zone = google_dns_managed_zone.primary.name
  name         = google_certificate_manager_dns_authorization.api[0].dns_resource_record[0].name
  type         = google_certificate_manager_dns_authorization.api[0].dns_resource_record[0].type
  ttl          = 300
  rrdatas      = [google_certificate_manager_dns_authorization.api[0].dns_resource_record[0].data]
}

resource "google_dns_record_set" "mcp_certificate_authorization" {
  count = local.cloud_run_resource_count

  project      = var.project_id
  managed_zone = google_dns_managed_zone.primary.name
  name         = google_certificate_manager_dns_authorization.mcp[0].dns_resource_record[0].name
  type         = google_certificate_manager_dns_authorization.mcp[0].dns_resource_record[0].type
  ttl          = 300
  rrdatas      = [google_certificate_manager_dns_authorization.mcp[0].dns_resource_record[0].data]
}

resource "google_certificate_manager_certificate" "edge" {
  count = local.cloud_run_resource_count

  project     = var.project_id
  name        = "proofplane-edge"
  description = "Managed certificate for Proofplane public services"
  labels      = local.labels

  managed {
    domains = [local.api_hostname, local.mcp_hostname]
    dns_authorizations = [
      google_certificate_manager_dns_authorization.api[0].id,
      google_certificate_manager_dns_authorization.mcp[0].id,
    ]
  }

  depends_on = [
    google_dns_record_set.api_certificate_authorization,
    google_dns_record_set.mcp_certificate_authorization,
  ]
}

resource "google_certificate_manager_certificate_map" "edge" {
  count = local.cloud_run_resource_count

  project     = var.project_id
  name        = "proofplane-edge"
  description = "Certificate map for Proofplane public services"
  labels      = local.labels
}

resource "google_certificate_manager_certificate_map_entry" "api" {
  count = local.cloud_run_resource_count

  project      = var.project_id
  name         = "proofplane-api"
  description  = "Certificate selection for ${local.api_hostname}"
  map          = google_certificate_manager_certificate_map.edge[0].name
  certificates = [google_certificate_manager_certificate.edge[0].id]
  hostname     = local.api_hostname
  labels       = local.labels
}

resource "google_certificate_manager_certificate_map_entry" "mcp" {
  count = local.cloud_run_resource_count

  project      = var.project_id
  name         = "proofplane-mcp"
  description  = "Certificate selection for ${local.mcp_hostname}"
  map          = google_certificate_manager_certificate_map.edge[0].name
  certificates = [google_certificate_manager_certificate.edge[0].id]
  hostname     = local.mcp_hostname
  labels       = local.labels
}

resource "google_compute_target_https_proxy" "edge" {
  count = local.cloud_run_resource_count

  project         = var.project_id
  name            = "proofplane-https"
  url_map         = google_compute_url_map.https[0].id
  certificate_map = "//certificatemanager.googleapis.com/${google_certificate_manager_certificate_map.edge[0].id}"

  depends_on = [
    google_certificate_manager_certificate_map_entry.api,
    google_certificate_manager_certificate_map_entry.mcp,
  ]
}

resource "google_compute_target_http_proxy" "redirect" {
  count = local.cloud_run_resource_count

  project = var.project_id
  name    = "proofplane-http-redirect"
  url_map = google_compute_url_map.http_redirect[0].id
}

resource "google_compute_global_forwarding_rule" "https" {
  count = local.cloud_run_resource_count

  project               = var.project_id
  name                  = "proofplane-https"
  ip_address            = google_compute_global_address.edge[0].id
  port_range            = "443"
  target                = google_compute_target_https_proxy.edge[0].id
  load_balancing_scheme = "EXTERNAL_MANAGED"
  network_tier          = "PREMIUM"
}

resource "google_compute_global_forwarding_rule" "http" {
  count = local.cloud_run_resource_count

  project               = var.project_id
  name                  = "proofplane-http"
  ip_address            = google_compute_global_address.edge[0].id
  port_range            = "80"
  target                = google_compute_target_http_proxy.redirect[0].id
  load_balancing_scheme = "EXTERNAL_MANAGED"
  network_tier          = "PREMIUM"
}

resource "google_dns_record_set" "api" {
  count = local.cloud_run_resource_count

  project      = var.project_id
  managed_zone = google_dns_managed_zone.primary.name
  name         = "${local.api_hostname}."
  type         = "A"
  ttl          = 300
  rrdatas      = [google_compute_global_address.edge[0].address]
}

resource "google_dns_record_set" "mcp" {
  count = local.cloud_run_resource_count

  project      = var.project_id
  managed_zone = google_dns_managed_zone.primary.name
  name         = "${local.mcp_hostname}."
  type         = "A"
  ttl          = 300
  rrdatas      = [google_compute_global_address.edge[0].address]
}

