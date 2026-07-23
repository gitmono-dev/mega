# moon/api — OpenAPI specs and typed client

Frontend API types live in `@gitmono/types` (`packages/types/generated.ts`). They are **generated** from the OpenAPI JSON files under `api/gen/`. Do not treat those outputs as hand-edited sources of truth.

## Do not edit these by hand

| File                                                               | Why                                                                                              |
| ------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| [`gen/gitmono.json`](gen/gitmono.json)                             | Exported from the **mono** OpenAPI (`utoipa`). Refresh from a running mono server.               |
| [`gen/1schema_swagger.json`](gen/1schema_swagger.json)             | Exported from **Campsite** Apigen (`rake apigen:…`). Owns types like `SyncUser` / `CurrentUser`. |
| [`../packages/types/generated.ts`](../packages/types/generated.ts) | Produced by [`script/gen-client`](../script/gen-client) via `swagger-typescript-api`.            |

Also avoid hand-editing other checked-in OpenAPI dumps that feed the same pipeline (for example [`gen/orion.json`](gen/orion.json) from **orion-server**). Change the Rust `utoipa` annotations (mono/orion) or Campsite serializers + Apigen export, then re-export and regenerate.

**Note:** Refreshing only `gitmono.json` from mono will **not** pick up Campsite fields such as `SyncUser.github_login`. Those live in `1schema_swagger.json`.

## Regenerate workflow

From the `moon/` directory (mono HTTP must be running on port 8000 by default):

```bash
# Fetches http://localhost:8000/api/openapi.json → pretty-prints → replaces api/gen/gitmono.json,
# then merges api/gen/*.json → api/gen/merged_swagger.json and regenerates packages/types/generated.ts
./script/gen-client

# Optional: override mono OpenAPI URL
# GITMONO_OPENAPI_URL=http://127.0.0.1:8000/api/openapi.json ./script/gen-client

# Optional: refresh orion OpenAPI when orion-server routes/DTOs changed (default port 8004)
curl -sS http://localhost:8004/api-doc/openapi.json -o api/gen/orion.json
./script/gen-client
```

`script/gen-client` refreshes `gitmono.json` from the live mono OpenAPI, runs [`merge-swagger.js`](merge-swagger.js) (merges every `api/gen/*.json` except `merged_swagger.json` / `openapi_schema.json`), then regenerates `packages/types/generated.ts`.

## Related endpoints

| Service      | Swagger UI                         | OpenAPI JSON                                 |
| ------------ | ---------------------------------- | -------------------------------------------- |
| mono         | `http://localhost:8000/swagger-ui` | `http://localhost:8000/api/openapi.json`     |
| orion-server | `http://localhost:8004/swagger-ui` | `http://localhost:8004/api-doc/openapi.json` |
