resource "google_compute_global_address" "edge" {
  project      = var.project_id
  name         = "proofplane-edge"
  address_type = "EXTERNAL"
  ip_version   = "IPV4"
}

resource "google_compute_region_network_endpoint_group" "api" {
  project               = var.project_id
  name                  = "proofplane-api"
  region                = var.region
  network_endpoint_type = "SERVERLESS"

  cloud_run {
    service = google_cloud_run_v2_service.public["api"].name
  }
}

resource "google_compute_region_network_endpoint_group" "mcp" {
  project               = var.project_id
  name                  = "proofplane-mcp"
  region                = var.region
  network_endpoint_type = "SERVERLESS"

  cloud_run {
    service = google_cloud_run_v2_service.public["mcp"].name
  }
}

resource "google_compute_backend_service" "api" {
  project               = var.project_id
  name                  = "proofplane-api"
  protocol              = "HTTP"
  port_name             = "http"
  load_balancing_scheme = "EXTERNAL_MANAGED"
  timeout_sec           = 600

  backend {
    group = google_compute_region_network_endpoint_group.api.id
  }
}

resource "google_compute_backend_service" "mcp" {
  project               = var.project_id
  name                  = "proofplane-mcp"
  protocol              = "HTTP"
  port_name             = "http"
  load_balancing_scheme = "EXTERNAL_MANAGED"
  timeout_sec           = 600

  backend {
    group = google_compute_region_network_endpoint_group.mcp.id
  }
}

resource "google_compute_url_map" "https" {
  project         = var.project_id
  name            = "proofplane-https"
  default_service = google_compute_backend_service.api.id

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
    default_service = google_compute_backend_service.api.id
  }

  path_matcher {
    name            = "mcp"
    default_service = google_compute_backend_service.mcp.id
  }
}

resource "google_compute_url_map" "http_redirect" {
  project = var.project_id
  name    = "proofplane-http-redirect"

  default_url_redirect {
    https_redirect = true
    strip_query    = false
  }
}

resource "google_certificate_manager_dns_authorization" "api" {
  project     = var.project_id
  name        = "proofplane-api"
  description = "DNS authorization for ${local.api_hostname}"
  domain      = local.api_hostname
  labels      = local.labels
}

resource "google_certificate_manager_dns_authorization" "mcp" {
  project     = var.project_id
  name        = "proofplane-mcp"
  description = "DNS authorization for ${local.mcp_hostname}"
  domain      = local.mcp_hostname
  labels      = local.labels
}

resource "google_dns_record_set" "api_certificate_authorization" {
  project      = var.project_id
  managed_zone = local.dns_zone_name
  name         = google_certificate_manager_dns_authorization.api.dns_resource_record[0].name
  type         = google_certificate_manager_dns_authorization.api.dns_resource_record[0].type
  ttl          = 300
  rrdatas      = [google_certificate_manager_dns_authorization.api.dns_resource_record[0].data]
}

resource "google_dns_record_set" "mcp_certificate_authorization" {
  project      = var.project_id
  managed_zone = local.dns_zone_name
  name         = google_certificate_manager_dns_authorization.mcp.dns_resource_record[0].name
  type         = google_certificate_manager_dns_authorization.mcp.dns_resource_record[0].type
  ttl          = 300
  rrdatas      = [google_certificate_manager_dns_authorization.mcp.dns_resource_record[0].data]
}

resource "google_certificate_manager_certificate" "edge" {
  project     = var.project_id
  name        = "proofplane-edge"
  description = "Managed certificate for Proofplane public services"
  labels      = local.labels

  managed {
    domains = [local.api_hostname, local.mcp_hostname]
    dns_authorizations = [
      google_certificate_manager_dns_authorization.api.id,
      google_certificate_manager_dns_authorization.mcp.id,
    ]
  }

  depends_on = [
    google_dns_record_set.api_certificate_authorization,
    google_dns_record_set.mcp_certificate_authorization,
  ]
}

resource "google_certificate_manager_certificate_map" "edge" {
  project     = var.project_id
  name        = "proofplane-edge"
  description = "Certificate map for Proofplane public services"
  labels      = local.labels
}

resource "google_certificate_manager_certificate_map_entry" "api" {
  project      = var.project_id
  name         = "proofplane-api"
  description  = "Certificate selection for ${local.api_hostname}"
  map          = google_certificate_manager_certificate_map.edge.name
  certificates = [google_certificate_manager_certificate.edge.id]
  hostname     = local.api_hostname
  labels       = local.labels
}

resource "google_certificate_manager_certificate_map_entry" "mcp" {
  project      = var.project_id
  name         = "proofplane-mcp"
  description  = "Certificate selection for ${local.mcp_hostname}"
  map          = google_certificate_manager_certificate_map.edge.name
  certificates = [google_certificate_manager_certificate.edge.id]
  hostname     = local.mcp_hostname
  labels       = local.labels
}

resource "google_compute_target_https_proxy" "edge" {
  project         = var.project_id
  name            = "proofplane-https"
  url_map         = google_compute_url_map.https.id
  certificate_map = "//certificatemanager.googleapis.com/${google_certificate_manager_certificate_map.edge.id}"

  depends_on = [
    google_certificate_manager_certificate_map_entry.api,
    google_certificate_manager_certificate_map_entry.mcp,
  ]
}

resource "google_compute_target_http_proxy" "redirect" {
  project = var.project_id
  name    = "proofplane-http-redirect"
  url_map = google_compute_url_map.http_redirect.id
}

resource "google_compute_global_forwarding_rule" "https" {
  project               = var.project_id
  name                  = "proofplane-https"
  ip_address            = google_compute_global_address.edge.id
  port_range            = "443"
  target                = google_compute_target_https_proxy.edge.id
  load_balancing_scheme = "EXTERNAL_MANAGED"
  network_tier          = "PREMIUM"
}

resource "google_compute_global_forwarding_rule" "http" {
  project               = var.project_id
  name                  = "proofplane-http"
  ip_address            = google_compute_global_address.edge.id
  port_range            = "80"
  target                = google_compute_target_http_proxy.redirect.id
  load_balancing_scheme = "EXTERNAL_MANAGED"
  network_tier          = "PREMIUM"
}

resource "google_dns_record_set" "api" {
  project      = var.project_id
  managed_zone = local.dns_zone_name
  name         = "${local.api_hostname}."
  type         = "A"
  ttl          = 300
  rrdatas      = [google_compute_global_address.edge.address]
}

resource "google_dns_record_set" "mcp" {
  project      = var.project_id
  managed_zone = local.dns_zone_name
  name         = "${local.mcp_hostname}."
  type         = "A"
  ttl          = 300
  rrdatas      = [google_compute_global_address.edge.address]
}

