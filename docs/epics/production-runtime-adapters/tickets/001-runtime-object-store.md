# 001 - Runtime Object Store

**Status:** Todo · **Depends on:** none · **Spec:** [spec.md](../spec.md#object-store-runtime)

**Summary** - Introduce the concrete runtime object-store enum and migrate API
and worker composition away from filesystem-only dependency types.

**Acceptance criteria**

- [ ] Given filesystem configuration, when API and worker start, then upload,
  scan, finalization, and download-grant redemption behavior is unchanged.
- [ ] Given an unknown or invalid storage configuration, when startup occurs,
  then it fails before serving traffic with a clear error.
- [ ] Given service and handler code, when this ships, then it contains no
  filesystem-only assumption outside adapter construction and tests.

**Tasks**

- [ ] Add `RuntimeObjectStore` delegation for the full contract.
- [ ] Update service, handler, app, and binary dependency types.
- [ ] Preserve static dispatch and existing filesystem tests.
- [ ] Add runtime-selection tests.
