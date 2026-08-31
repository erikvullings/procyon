# Generated code — do not edit

Everything under `src/api/generated/` is produced by Orval from
[`frontend/openapi/openapi.json`](../../../openapi/openapi.json) via
`frontend/orval.config.ts` (task 0010).

- Regenerate with `pnpm api:generate` (or `pnpm api:check`, which also
  re-exports the OpenAPI document and fails if the checked-in output is
  stale).
- Never hand-edit files in this directory — changes are overwritten on the
  next generation run.
- Feature code must not import from this directory directly; go through
  `frontend/src/api/client/http-file-manager-client.ts` (task 0012) instead.
