# Shared rules for every phase root under this directory. Each root includes
# this file after setting TF_STATE_PREFIX and PLAN, so those two assignments win
# over the `?=` defaults below. Every path here is relative to the including
# root, because make evaluates them in that root's directory.
.PHONY: help init plan replan clean

TF ?= terraform
TF_STATE_BUCKET ?=

# No defaults for these two. A wrong-but-plausible prefix would initialize a
# root against another phase's state, so an unset value has to stop the run.
ifeq ($(strip $(TF_STATE_PREFIX)),)
$(error TF_STATE_PREFIX must be set by the including root Makefile)
endif
ifeq ($(strip $(PLAN)),)
$(error PLAN must be set by the including root Makefile)
endif

TF_FILES := $(wildcard *.tf)
INIT_STAMP := .terraform/terraform.tfstate

# The GCS backend is a partial config. Pass the backend settings only when a
# bucket is supplied; otherwise Terraform reuses the config cached by the
# previous init.
BACKEND_CONFIG = $(if $(TF_STATE_BUCKET),-backend-config=bucket=$(TF_STATE_BUCKET) -backend-config=prefix=$(TF_STATE_PREFIX))

help:
	@printf '%s\n' \
		'Targets:' \
		'  make init              Initialize the backend and providers' \
		'  make plan              Write $(PLAN) if any input changed' \
		'  make replan            Discard the saved plan and plan again' \
		'  make clean             Remove saved plans' \
		'' \
		'Variables:' \
		'  TF_STATE_BUCKET        State bucket; required for the first init' \
		'  TF_STATE_PREFIX        State prefix (default $(TF_STATE_PREFIX))' \
		'  PLAN                   Saved plan path (default $(PLAN))'

init: $(INIT_STAMP)

$(INIT_STAMP): backend.tf versions.tf .terraform.lock.hcl
	@if [ -z '$(TF_STATE_BUCKET)' ] && [ ! -f '$(INIT_STAMP)' ]; then \
		printf '%s\n' \
			'TF_STATE_BUCKET is required for the first init.' \
			'Run: make init TF_STATE_BUCKET=your-state-bucket'; \
		exit 1; \
	fi
	$(TF) init -input=false $(BACKEND_CONFIG)
	@touch $(INIT_STAMP)

plan: $(PLAN)

$(PLAN): $(TF_FILES) terraform.tfvars $(INIT_STAMP)
	$(TF) plan -input=false -out $(PLAN)

# Make only sees local files, so a saved plan can be stale against real cloud
# state. Re-plan before every apply.
replan:
	@rm -f $(PLAN)
	@$(MAKE) $(PLAN)

terraform.tfvars:
	@printf '%s\n' \
		'terraform.tfvars is missing.' \
		'Run: cp terraform.tfvars.example terraform.tfvars, then fill it in.' >&2
	@exit 1

clean:
	rm -f *.tfplan
