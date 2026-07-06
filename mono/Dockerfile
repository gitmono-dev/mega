# ────── Stage 0: Chef ──────
# Downloads and installs cargo-chef (prebuilt binary, no Rust compilation needed).
FROM rust:1.96.0 AS chef

ARG CARGO_CHEF_VERSION=0.1.77

WORKDIR /opt/mega

# Install cargo-chef from the release installer script.
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && curl --proto '=https' --tlsv1.2 -LsSf \
         "https://github.com/LukeMathWalker/cargo-chef/releases/download/v${CARGO_CHEF_VERSION}/cargo-chef-installer.sh" \
         | sh

# ────── Stage 1: Planner ──────
# Copies all Cargo.toml files; cargo-chef emits recipe.json describing the full dependency graph.
# This stage is cached unless a Cargo.toml changes.
FROM chef AS planner

WORKDIR /opt/mega
ARG TARGETARCH

COPY Cargo.toml ./
COPY Cargo.lock ./
COPY api-model/Cargo.toml api-model/
COPY ceres/Cargo.toml ceres/
COPY common/Cargo.toml common/
COPY io-orbit/Cargo.toml io-orbit/
COPY jupiter/Cargo.toml jupiter/
COPY jupiter/callisto/Cargo.toml jupiter/callisto/
COPY jupiter-migrate/Cargo.toml jupiter-migrate/
COPY mono/Cargo.toml mono/
COPY orion/Cargo.toml orion/
COPY orion/audit/Cargo.toml orion/audit/
COPY orion/td_util/Cargo.toml orion/td_util/
COPY orion/buck/Cargo.toml orion/buck/
COPY orion-scheduler/Cargo.toml orion-scheduler/
COPY orion-server/Cargo.toml orion-server/
COPY clients/orion-client/Cargo.toml clients/orion-client/
COPY clients/orion-scheduler-client/Cargo.toml clients/orion-scheduler-client/
COPY saturn/Cargo.toml saturn/
COPY vault/Cargo.toml vault/

# Named cache mounts let the builder stage inherit this cache via `from=planner`.
RUN --mount=type=cache,target=/usr/local/cargo/registry,id=mega-cargo-registry-${TARGETARCH},sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,id=mega-cargo-git-${TARGETARCH},sharing=locked \
    cargo chef prepare --bin mono --recipe-path recipe.json

# ────── Stage 2: Builder ──────
# Installs system libs, builds all dependencies (cached), then builds the mono binary.
FROM chef AS builder

WORKDIR /opt/mega
ARG TARGETARCH

# Install system dependencies required for building
RUN apt-get update && apt-get install -y --no-install-recommends \
    libssl-dev \
    ca-certificates \
    clang \
    mold \
    protobuf-compiler \
    libprotobuf-dev \
    && rm -rf /var/lib/apt/lists/*

# Faster linking for release builds.
ENV RUSTFLAGS="-C link-arg=-fuse-ld=mold"

# Inherit registry/git caches from the planner stage.
COPY --from=planner /opt/mega/recipe.json recipe.json

RUN --mount=type=cache,target=/usr/local/cargo/registry,id=mega-cargo-registry-${TARGETARCH},sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,id=mega-cargo-git-${TARGETARCH},sharing=locked \
    cargo chef cook --profile demo --recipe-path recipe.json

COPY . .

# Build the binary into the SAME target dir cargo-chef cooked into
# (/opt/mega/target). Reusing that dir means the cooked dependency artifacts
# from the cook layer are picked up, so only the mono crate itself is compiled
# here instead of the whole dependency graph.
RUN --mount=type=cache,target=/usr/local/cargo/registry,id=mega-cargo-registry-${TARGETARCH},sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,id=mega-cargo-git-${TARGETARCH},sharing=locked \
    cargo build --locked --profile demo -p mono

# ────── Stage 3: Runtime ──────
FROM debian:trixie-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    less \
    libssl3t64 \
    && rm -rf /var/lib/apt/lists/*

# Copy the compiled binary and startup script
COPY --from=builder /opt/mega/target/demo/mono /usr/local/bin/mono
COPY --from=builder /opt/mega/mono/start-mono.sh /usr/local/bin/start-mono.sh

# Ensure binaries are executable
RUN chmod +x /usr/local/bin/mono /usr/local/bin/start-mono.sh

# Optional volume for runtime data
VOLUME /opt/mega

# Default startup command
CMD ["bash", "-c", "/usr/local/bin/start-mono.sh"]
