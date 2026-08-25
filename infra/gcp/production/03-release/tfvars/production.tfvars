project_id   = "proofplane-production"
state_bucket = "proofplane-production-terraform-state"

# Every value below is required. Push the images and create the secret payload
# versions first. This root creates nothing until all of them are pinned.
app_image_digest            = "us-central1-docker.pkg.dev/PROJECT/proofplane/proofplane@sha256:..."
clamav_image_digest         = "us-central1-docker.pkg.dev/PROJECT/proofplane/clamav@sha256:..."
clamav_updater_image_digest = "us-central1-docker.pkg.dev/PROJECT/proofplane/clamav-updater@sha256:..."

runtime_config_secret_version     = "1"
migration_database_secret_version = "1"

# This value is optional, and it is the only optional value in this file. Set it
# only when the database endpoint chains to a private root. See the certificate
# check in docs/runbooks/production-deployment.md.
# migration_database_root_certificate_file = "supabase-root.crt"
