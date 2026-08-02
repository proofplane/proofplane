# 001 - Architecture and Machine-Grant Corrections

**Status:** Done · **Triage:** ready-for-agent · **Depends on:** none · **Spec:** [spec.md](../spec.md#machine-upload-grant-correction)

**Summary** - Establish the domain and architecture contract and correct both
machine-upload-grant aggregates so persisted state and locking match it.

**Acceptance criteria**

- [x] Given a valid pending or completed grant, when it is rehydrated, then its complete snapshot is restored.
- [x] Given completion outside the issuance-to-expiry interval, when rehydration or persistence is attempted, then it is rejected.
- [x] Given an aggregate read inside a transaction, when it succeeds, then its row lock lives through the transaction; an autocommit read does not claim that lock.
- [x] Given a fresh baseline database, when valid grants are saved and loaded, then the complete snapshots remain readable.

**Tasks**

- [x] Record CQRS vocabulary, aggregate boundaries, and the architecture decision.
- [x] Add aggregate tests for completion and rehydration boundaries.
- [x] Correct both rehydration paths and baseline schema constraints.
- [x] Separate locking aggregate loads from non-locking verification reads.
- [x] Add repository integration tests and run focused checks.

**Notes** - The pre-deployment migration history was consolidated into V001;
see the 2026-08-02 revision in the spec.
