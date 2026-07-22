# moon/api — OpenAPI specs and typed client

Frontend API types live in `@gitmono/types` (`packages/types/generated.ts`). They are **generated** from the OpenAPI JSON files under `api/gen/`. Do not treat those outputs as hand-edited sources of truth.

## Do not edit these by hand

| File | Why |
|------|-----|
| [`gen/gitmono.json`](gen/gitmono.json) | Exported from the **mono** OpenAPI (`utoipa`). Refresh from a running mono server. |
| [`../packages/types/generated.ts`](../packages/types/generated.ts) | Produced by [`script/gen-client`](../script/gen-client) via `swagger-typescript-api`. |

Also avoid hand-editing other checked-in OpenAPI dumps that feed the same pipeline (for example [`gen/orion.json`](gen/orion.json) from **orion-server**). Change the Rust `utoipa` annotations instead, then re-export and regenerate.

## Regenerate workflow

From the `moon/` directory:

```bash
# 1) Refresh mono OpenAPI (mono HTTP must be running; default port 8000)
curl -sS http://localhost:8000/api/openapi.json -o api/gen/gitmono.json

# Optional: refresh orion OpenAPI when orion-server routes/DTOs changed (default port 8004)
curl -sS http://localhost:8004/api-doc/openapi.json -o api/gen/orion.json

# 2) Merge api/gen/*.json → api/gen/merged_swagger.json and write packages/types/generated.ts
./script/gen-client
```

`script/gen-client` runs [`merge-swagger.js`](merge-swagger.js) (merges every `api/gen/*.json` except `merged_swagger.json` / `openapi_schema.json`), then regenerates `packages/types/generated.ts`.

## Related endpoints

| Service | Swagger UI | OpenAPI JSON |
|---------|------------|--------------|
| mono | `http://localhost:8000/swagger-ui` | `http://localhost:8000/api/openapi.json` |
| orion-server | `http://localhost:8004/swagger-ui` | `http://localhost:8004/api-doc/openapi.json` |
