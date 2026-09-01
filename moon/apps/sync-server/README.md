# sync-server

Local development: copy `.env.example` to `.env.local` and set `API_URL` to the
same Campsite API the web app uses. Web `NEXT_PUBLIC_SYNC_URL` must point at this
process (`ws://localhost:9000` locally, or `wss://sync.<base_domain>` for
k3s-dev / k3s-rust / k3s-rk8s — see `apps/web/.env.example`).

## Troubleshooting

To test the Docker build locally, run the following command in your terminal (from the repo root):

```
docker build -f apps/sync-server/Dockerfile . --build-arg SENTRY_AUTH_TOKEN=<xxx>
```

> [!Note]
> Be sure to replace `<xxx>` with the auth token on the Fly instance. You can get this easily like so:

```
cd apps/sync-server
fly ssh console
echo $SENTRY_AUTH_TOKEN
```
