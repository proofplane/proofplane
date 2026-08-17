locals {
  labels = merge({
    application = "proofplane"
    environment = "production"
    managed-by  = "terraform"
  }, var.labels)
}
