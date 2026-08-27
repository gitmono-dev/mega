# mono-reset

Destructive K8s Job: wipe monorepo/git data in Postgres + RustFS `git/`/`lfs/`, keep Campsite login, restart mono so `init_monorepo` runs, then sync `toolchains/buckal-bundles` via `init_mega.py`.

**Default off.** Only enable when you intend to erase all git content.

## What it keeps

- Entire Campsite MySQL database (users / orgs / sessions)
- Postgres: `campsite_member_identity`, `user_approval_status`, `access_token`, `ssh_keys`, `gpg_key`, `cla_sign_status`, `vault`, `path_check_configs`, `seaql_migrations`

## What it deletes

- All other public Postgres tables (TRUNCATE … CASCADE), including bots (recreated by `bootstrap-init`)
- RustFS bucket prefixes `git/` and `lfs/`
- Does **not** touch freighter hostPath (crates-sync cache)

After wipe, `third-party` is an empty root again (`.gitkeep` from `init_monorepo`). Re-run crates-sync if you need crates back.

## Container image

Dockerfile: [`Dockerfile`](Dockerfile) (python3 + git + postgresql-client + mc).

- CI: `.github/workflows/mono-reset-deploy.yml` → `registry.xuanwu.openatom.cn/mega/mono-reset:<sha>`
- Local (from repo root):

```bash
docker build -f scripts/mono-reset/Dockerfile -t mega/mono-reset:local .
```

## Terraform (onprem)

In `envs/onprem/k3s-rust` (or sibling env). Put **only** `enable_mono_reset = true`
in tfvars; type the confirm string at apply time (plan fails if wrong/missing):

```hcl
enable_mono_reset = true
mono_reset_image  = "registry.xuanwu.openatom.cn/mega/mono-reset:<sha>"  # optional
```

```bash
printf 'Confirm wipe (WIPE_GIT_DATA:mega-rust): '
read -r c
terraform apply -var="mono_reset_confirm=$c"
kubectl -n mega-rust logs -f job/mono-reset
```

After success, set `enable_mono_reset = false` and apply without the confirm
`-var` so a later unrelated apply does not recreate the Job.

While the Job scales `mono-engine` (and `orion-server`) to 0, git API is down; **mega-ui** and **campsite-api** stay up so login UI remains reachable.

## Manual / debug flags

```text
--dry-run       Print actions only
--skip-s3       Skip RustFS wipe
--skip-buckal   Skip init_mega buckal sync
--skip-scale    Do not scale deployments (unsafe if mono is still writing)
--db-url URL    Override MEGA_DATABASE__DB_URL
--base-url URL  Mono HTTP base (default MONO_BASE_URL or http://mono-engine:8000)
```

## Requirements

- Job ServiceAccount can get/patch `deployments` / `deployments/scale` for `mono-engine` and `orion-server`
- Env: `MEGA_DATABASE__DB_URL`, S3 `MEGA_OBJECT_STORAGE__S3__*`, `MEGA_INIT_BOOTSTRAP_SECRET`
- Mono must have `MEGA_MONOREPO__ADMIN` (or image config) for post-init admins
