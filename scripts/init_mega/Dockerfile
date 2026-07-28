# mega-init: python3 + git + init/sync scripts for K8s Job.
# Terraform runs: python3 scripts/init_mega/init_mega.py --base-url ...
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

# Only the scripts needed by init_mega (buckal-bundles sync).
COPY scripts/init_mega/ scripts/init_mega/

# Default entry matches the Job command prefix.
ENTRYPOINT ["python3", "scripts/init_mega/init_mega.py"]
