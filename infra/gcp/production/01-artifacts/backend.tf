terraform {
  backend "gcs" {
    prefix = "01-artifacts"
  }
}
