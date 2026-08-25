# crates-sync: python3 + git for the K8s crates import Job.
# Build from repo root:
#   docker build -f scripts/crates-sync/Dockerfile -t mega/crates-sync scripts/crates-sync
# Or from this directory:
#   docker build -t mega/crates-sync .
FROM python:3.12-slim-bookworm

# Debian slim defaults to http://deb.debian.org. Through corporate proxies:
# - plain HTTP hangs
# - HTTPS works but MITM breaks apt's CA verify
# Switch to HTTPS and relax peer verify only for this apt fetch.
RUN sed -i 's|http://deb.debian.org|https://deb.debian.org|g' /etc/apt/sources.list.d/debian.sources \
    && apt-get -o Acquire::https::Verify-Peer=false -o Acquire::https::Verify-Host=false update -qq \
    && apt-get -o Acquire::https::Verify-Peer=false -o Acquire::https::Verify-Host=false install -y --no-install-recommends \
        ca-certificates \
        git \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Build context must be the scripts/crates-sync directory (this folder).
COPY . scripts/crates-sync/

ENTRYPOINT ["python3", "scripts/crates-sync/run_job.py"]
