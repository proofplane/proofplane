# Everything this root attaches to already exists. 02-foundation created the
# identities, secrets, buckets, topics, zone, and notification channel, and it
# publishes each one this root needs as an output. Reading its state keeps the
# values exact. A data-source lookup would re-derive names that Terraform
# already knows.
data "terraform_remote_state" "foundation" {
  backend = "gcs"

  config = {
    bucket = var.state_bucket
    # Must match the prefix in 02-foundation/backend.tf. It is a literal in
    # both places for the same reason: a phase's prefix is a fixed property of
    # the layout, so nothing at plan time should be able to change one of them.
    prefix = "02-foundation"
  }
}

locals {
  foundation = data.terraform_remote_state.foundation.outputs

  service_account_emails             = local.foundation.service_account_emails
  pubsub_service_identity_email      = local.foundation.pubsub_service_identity_email
  application_topic_id               = local.foundation.application_topic_id
  dead_letter_topic_id               = local.foundation.dead_letter_topic_id
  runtime_config_secret_name         = local.foundation.runtime_config_secret_name
  migration_database_url_secret_name = local.foundation.migration_database_url_secret_name
  clamav_definitions_bucket          = local.foundation.clamav_definitions_bucket
  dns_zone_name                      = local.foundation.dns_zone_name
  notification_channel               = local.foundation.notification_channel

  # The apex comes from 02-foundation as well, so the records this root writes
  # can never name a different apex than the zone that holds them.
  domain = local.foundation.domain
}
