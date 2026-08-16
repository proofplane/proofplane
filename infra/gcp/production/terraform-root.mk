# Shared rules for every phase root under this directory. Each root includes
# this file after setting PLAN, so that assignment wins over the `?=` default
# below. Every path here is relative to the including root, because make
# evaluates them in that root's directory.
.PHONY: help init plan replan clean

TF ?= terraform
TF_STATE_BUCKET ?=

# Which environment's inputs to plan with. The roots ship `production`, and a
# second environment would add its own file rather than edit that one.
ENV ?= production
TFVARS = tfvars/$(ENV).tfvars

ifeq ($(strip $(PLAN)),)
$(error PLAN must be set by the including root Makefile)
endif

TF_FILES := $(wildcard *.tf)
INIT_STAMP := .terraform/terraform.tfstate

# The GCS backend is a partial config, but only the bucket is missing: each
# root's backend.tf declares its own prefix, so no phase can be initialized
# against another phase's state by passing the wrong argument here. Pass the
# bucket only when one is supplied; otherwise Terraform reuses the config
# cached by the previous init.
BACKEND_CONFIG = $(if $(TF_STATE_BUCKET),-backend-config=bucket=$(TF_STATE_BUCKET))

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
		'  ENV                    Input set under tfvars/ (default $(ENV))' \
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

$(PLAN): $(TF_FILES) $(TFVARS) $(INIT_STAMP)
	$(TF) plan -input=false -var-file $(TFVARS) -out $(PLAN)

# Make only sees local files, so a saved plan can be stale against real cloud
# state. Re-plan before every apply.
replan:
	@rm -f $(PLAN)
	@$(MAKE) $(PLAN)

$(TFVARS):
	@printf '%s\n' \
		'$(TFVARS) is missing.' \
		'Every root ships tfvars/production.tfvars; check ENV=$(ENV).' >&2
	@exit 1

clean:
	rm -f *.tfplan
