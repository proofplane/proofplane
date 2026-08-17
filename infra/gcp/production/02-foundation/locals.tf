locals {
  labels = merge({
    application = "proofplane"
    environment = "production"
    managed-by  = "terraform"
  }, var.labels)

  application_topic = "proof.message_bus"
  dead_letter_topic = "proof.message_bus.dead_letter"
}
