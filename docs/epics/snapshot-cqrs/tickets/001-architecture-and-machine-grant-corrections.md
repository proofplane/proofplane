# 001 - Architecture and Machine-Grant Corrections

**Status:** Done · **Triage:** ready-for-agent · **Depends on:** none · **Spec:** [spec.md](../spec.md#machine-upload-grant-correction)

**Summary** - Establish the domain and architecture contract and correct both
machine-upload-grant aggregates so persisted state and locking match it.

**Acceptance criteria**

- [x] Given a valid pending or completed grant, when it is rehydrated, then its complete snapshot is restored.
- [x] Given completion outside the issuance-to-expiry interval, when rehydration or persistence is attempted, then it is rejected.
- [x] Given an aggregate read inside a transaction, when it succeeds, then its row lock lives through the transaction; an autocommit read does not claim that lock.
- [x] Given existing valid grant rows, when the migration runs, then they remain readable.

**Tasks**

- [x] Record CQRS vocabulary, aggregate boundaries, and the architecture decision.
- [x] Add aggregate tests for completion and rehydration boundaries.
- [x] Correct both rehydration paths and schema constraints.
- [x] Separate locking aggregate loads from non-locking verification reads.
- [x] Add repository integration tests and run focused checks.
