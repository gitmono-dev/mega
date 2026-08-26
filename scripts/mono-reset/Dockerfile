# mono-reset: wipe PG git/CL data (keep login), clear RustFS git/lfs, re-init mono.
# Build from repo root:
#   docker build -f scripts/mono-reset/Dockerfile -t mega/mono-reset .
FROM python:3.12-slim-bookworm

# Debian slim defaults to http://deb.debian.org. Through corporate proxies:
# - plain HTTP hangs
# - HTTPS works but MITM breaks apt's CA verify
# Switch to HTTPS and relax peer verify only for this apt fetch.
RUN sed -i 's|http://deb.debian.org|https://deb.debian.org|g' /etc/apt/sources.list.d/debian.sources \
    && apt-get -o Acquire::https::Verify-Peer=false -o Acquire::https::Verify-Host=false update -qq \
    && apt-get -o Acquire::https::Verify-Peer=false -o Acquire::https::Verify-Host=false install -y --no-install-recommends \
        ca-certificates \
        curl \
        git \
        postgresql-client \
    && curl -fsSL https://dl.min.io/client/mc/release/linux-amd64/mc -o /usr/local/bin/mc \
    && chmod +x /usr/local/bin/mc \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

ENV PYTHONUNBUFFERED=1

# reset_job imports wipe_sql and shells out to init_mega.py
COPY scripts/mono-reset/ scripts/mono-reset/
COPY scripts/init_mega/ scripts/init_mega/

ENTRYPOINT ["python3", "-u", "scripts/mono-reset/reset_job.py"]
