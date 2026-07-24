# syntax = docker/dockerfile:1

FROM node:20-slim AS base

# configure pnpm
ENV PNPM_HOME="/pnpm"
ENV PATH="$PNPM_HOME/bin:$PNPM_HOME:$PATH"
RUN corepack enable && corepack prepare pnpm@9.7.1 --activate


# Throw-away build stage to reduce size of final image
FROM base AS builder
WORKDIR /app
RUN pnpm install turbo@2.9.16 --global
COPY . .
RUN turbo prune @gitmono/web --docker
WORKDIR /app


FROM base AS installer
ARG TARGETARCH
ARG TIPTAP_PRIVATE_REGISTRY_KEY
RUN apt-get update -qq && \
  apt-get install -y build-essential pkg-config python-is-python3 ca-certificates
WORKDIR /app

COPY --from=builder /app/out/json/ .
COPY --from=builder /app/out/pnpm-lock.yaml ./pnpm-lock.yaml
COPY --from=builder /app/patches ./patches
COPY --from=builder /app/pnpm-workspace.yaml ./


RUN pnpm config set "@tiptap-pro:registry" https://registry.tiptap.dev/
RUN pnpm config set "//registry.tiptap.dev/:_authToken" "${TIPTAP_PRIVATE_REGISTRY_KEY}"
RUN --mount=type=cache,id=mega-ui-pnpm-store-${TARGETARCH},target=/pnpm/store,sharing=locked \
    pnpm config set store-dir /pnpm/store && \
    pnpm install --frozen-lockfile
RUN npx patch-package

COPY --from=builder /app/out/full/ .

# Build a single, environment-agnostic image. The placeholder URLs in
# .env.runtime get baked into the bundle here and are replaced with real
# per-environment values at container start by docker-entrypoint.sh.
RUN rm -f /app/apps/web/.env.local \
  && cp /app/apps/web/.env.runtime /app/apps/web/.env.production

RUN --mount=type=cache,id=mega-ui-next-${TARGETARCH},target=/app/apps/web/.next/cache,sharing=locked \
    --mount=type=cache,id=mega-ui-turbo-${TARGETARCH},target=/app/.turbo,sharing=locked \
    npx turbo run build --filter=@gitmono/web



FROM base AS runner

# Set production environment
ENV NODE_ENV="production"

WORKDIR /app

# Copy built application
COPY --from=installer --chown=nextjs:nodejs /app/apps/web/.next/standalone ./
COPY --from=installer --chown=nextjs:nodejs /app/apps/web/.next/static ./apps/web/.next/static
COPY --from=installer --chown=nextjs:nodejs /app/apps/web/public ./apps/web/public

# Runtime env injection: rewrites placeholder URLs in the compiled assets with
# real values from runtime environment variables, then starts the server.
COPY apps/web/docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh
RUN chmod +x /usr/local/bin/docker-entrypoint.sh

# Start the server by default, this can be overwritten at runtime
EXPOSE 3000
ENV PORT=3000

ENTRYPOINT [ "/usr/local/bin/docker-entrypoint.sh" ]
