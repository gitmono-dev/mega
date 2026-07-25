# syntax = docker/dockerfile:1

FROM node:26-slim AS base

# configure pnpm (node:26 images no longer ship corepack)
ENV PNPM_HOME="/pnpm"
ENV PATH="$PNPM_HOME/bin:$PNPM_HOME:$PATH"
RUN npm install -g pnpm@11.17.0


# ENV SENTRY_PROPERTIES=sentry.properties

# Throw-away build stage to reduce size of final image
FROM base AS builder
# Install packages needed to build node modules
RUN apt-get update -qq && \
  apt-get install -y build-essential pkg-config python-is-python3 ca-certificates
WORKDIR /app
RUN pnpm install turbo@2.9.16 --global
COPY . .
RUN turbo prune @gitmono/sync-server --docker
WORKDIR /app


FROM base AS installer
WORKDIR /app
COPY --from=builder /app/out/json/ .
COPY --from=builder /app/out/pnpm-lock.yaml ./pnpm-lock.yaml

RUN pnpm config set "@tiptap-pro:registry" https://registry.tiptap.dev/
RUN pnpm config set "//registry.tiptap.dev/:_authToken" CjqFil0n7z4GGur+RPric21fBBeSB4R4FoNdRYOE1bQEz6AXLCoANCc+o9rLIg6Q
RUN pnpm install

COPY --from=builder /app/out/full/ .

RUN npx turbo run build --filter=@gitmono/sync-server

# ARG SENTRY_AUTH_TOKEN
# ARG SENTRY_ORG
# ARG SENTRY_PROJECT

RUN apt-get update -y && apt-get install -y ca-certificates

# RUN SENTRY_AUTH_TOKEN=${SENTRY_AUTH_TOKEN} SENTRY_ORG=${SENTRY_ORG} SENTRY_PROJECT=${SENTRY_PROJECT} npx turbo run sentry:sourcemaps --filter=@gitmono/sync-server


FROM base AS runner

# Set production environment
ENV NODE_ENV="production"

WORKDIR /app/apps/sync-server

# Copy built application
COPY --from=installer /app /app

# Start the server by default, this can be overwritten at runtime
EXPOSE 9000
ENV PORT=9000

ENTRYPOINT [ "node", "dist/index.mjs" ]
