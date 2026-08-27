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
        tini \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# So kubectl logs shows print() immediately (stdout is a pipe, not a TTY).
ENV PYTHONUNBUFFERED=1

# Build context must be the scripts/crates-sync directory (this folder).
COPY . scripts/crates-sync/

# tini as PID 1: every git HTTP op leaves an orphaned `git remote-http` shim
# behind; a Python PID 1 never reaps adopted orphans, so they accumulate as
# zombies until the pod hits its pids cgroup limit and all forks fail (EAGAIN),
# which surfaces as mass "push rejected" errors. tini reaps them (-g: group-wide).
ENTRYPOINT ["tini", "-g", "--", "python3", "-u", "scripts/crates-sync/run_job.py"]
