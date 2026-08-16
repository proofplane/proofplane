terraform {
  backend "gcs" {
    prefix = "03-release"
  }
}
