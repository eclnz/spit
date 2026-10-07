# Concise operation hovers

- [x] Inspect issue #105 and operation hover generation.
- [x] Omit the generic body at concrete calls; retain declaration bodies and primitive details.
- [x] Wrap signatures longer than 100 bytes with one port per line; cover the reported shape.
- [x] Verify required checks and benchmark both workloads (implementation commit pending).
  Pipeline hovers: 32.1 → 31.5 ms; dataset DAG: 172.7 → 169.5 ms. Both regression checks pass.
  Messie reports existing example/fixture findings and generated benchmark folders locally; the clean dev baseline has the same example/fixture findings.
- [ ] Update extension rendering tests after documentation is finalized, as requested.
- [ ] Delete this plan in a dedicated final commit before merge.
