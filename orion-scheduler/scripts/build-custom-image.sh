#!/bin/bash
# Build a custom Debian image with buck2 and Rust toolchain pre-installed.
#
# Pre-installs Rust toolchain, apt packages, and buck2 directly into the image
# via chroot. Also caches the prelude cpython_archive tarball behind a local
# GitHub HTTPS mirror so toolchains//:cpython_archive can fetch without hitting
# github.com during worker builds (no toolchains repo changes required).
#
# Usage: sudo ./build-custom-image.sh
#
# Note: Must run as root because qemu-nbd / mount / chroot need it.
# Images are published to the invoking user's ~/.local/share/qlean/images
# (e.g. /home/orion/... when run as `sudo -u` or `sudo` from user orion),
# not /root/. Override with OUTPUT_DIR=... if needed.

set -eo pipefail

# ============================================================================
# Configuration
# ============================================================================
# qlean / orion-scheduler run as the login user (orion), not root. When this
# script is invoked via sudo, $HOME is /root — publish to the invoking user's
# qlean images dir unless OUTPUT_DIR is set explicitly.
if [ -n "${OUTPUT_DIR:-}" ]; then
    : # caller override
elif [ -n "${SUDO_USER:-}" ] && [ "$SUDO_USER" != "root" ]; then
    _qlean_home="$(getent passwd "$SUDO_USER" | cut -d: -f6)"
    OUTPUT_DIR="${_qlean_home}/.local/share/qlean/images"
elif [ -n "${SUDO_UID:-}" ] && [ "$SUDO_UID" != "0" ]; then
    _qlean_home="$(getent passwd "$SUDO_UID" | cut -d: -f6)"
    OUTPUT_DIR="${_qlean_home}/.local/share/qlean/images"
else
    OUTPUT_DIR="${HOME}/.local/share/qlean/images"
fi
unset _qlean_home
IMAGE_NAME="debian-13-buck2"
IMAGE_DIR="$OUTPUT_DIR/$IMAGE_NAME"
BASE_DIR="$OUTPUT_DIR/debian-13-generic-amd64"
BASE_IMAGE="$BASE_DIR/debian-13-generic-amd64.qcow2"
CUSTOM_IMAGE="$IMAGE_DIR/$IMAGE_NAME.qcow2"

# Upstream base image (same image qlean itself pins). Auto-downloaded when
# $BASE_IMAGE is missing; verified against the published SHA512SUMS.
#
# Override the mirror via env, e.g. to use the official source:
#   BASE_MIRROR_URL=https://cloud.debian.org/images/cloud/trixie/latest \
#     sudo -E bash scripts/build-custom-image.sh
# Default points at a CN mirror (SJTU) since cloud.debian.org is often
# unreachable from CN build hosts. NOTE: the USTC mirror returns HTTP 403 for
# the large .qcow2/.raw cloud images (it only serves the metadata files), so
# it cannot be used here.
BASE_IMAGE_FILE="debian-13-generic-amd64.qcow2"
BASE_MIRROR_URL="${BASE_MIRROR_URL:-https://mirror.sjtu.edu.cn/debian-cdimage/cloud/trixie/latest}"
BASE_IMAGE_URL="${BASE_MIRROR_URL%/}/${BASE_IMAGE_FILE}"
BASE_CHECKSUM_URL="${BASE_MIRROR_URL%/}/SHA512SUMS"

IMAGE_SIZE="15G"

RUST_VERSION="1.97.1"
RUST_ARCH="x86_64-unknown-linux-gnu"
# CN rustup dist mirror (RUSTUP_DIST_SERVER). Xuanwu mirrors .tar.xz reliably;
# undated /dist/rust-*.tar.* and .tar.gz often 404 — we use the dated xz path
# from channel-rust-${RUST_VERSION}.toml. Override for the official CDN:
#   RUSTUP_DIST_SERVER=https://static.rust-lang.org sudo -E bash scripts/build-custom-image.sh
# Docs: https://xuanwu.openatom.cn/guide/quick-start/install.html
RUSTUP_DIST_SERVER="${RUSTUP_DIST_SERVER:-https://mirror.xuanwu.openatom.cn}"
# Optional offline overrides. If either is unset, both date + xz_hash are
# resolved from ${RUSTUP_DIST_SERVER}/dist/channel-rust-${RUST_VERSION}.toml
# so bumping RUST_VERSION alone is enough (avoids 404 from a stale date dir).
RUST_DIST_DATE="${RUST_DIST_DATE:-}"
RUST_SHA256="${RUST_SHA256:-}"
# Host-side cache for the Rust toolchain tarball. Pre-place the file here to
# skip the download (useful when the network is flaky/blocked):
#   curl -fL -o /tmp/rust-1.97.1-x86_64-unknown-linux-gnu.tar.xz "$RUST_TARBALL_URL"
# Truncated/corrupt caches are detected via SHA256 and re-downloaded.
RUST_TARBALL="/tmp/rust-${RUST_VERSION}-${RUST_ARCH}.tar.xz"
RUST_TARBALL_URL=""  # set by resolve_rust_channel

BUCK2_VERSION="2026-04-15"
BUCK2_ARCH="x86_64-unknown-linux-gnu"
# Label recorded in orion-image-info.json (release tag, not just the date).
BUCK2_LABEL="scorpiofs-execvp-2026-04-15"
# BUCK2_URL="https://github.com/facebook/buck2/releases/download/${BUCK2_VERSION}/buck2-${BUCK2_ARCH}.zst"
BUCK2_URL="https://github.com/Ivanbeethoven/buck2/releases/download/${BUCK2_LABEL}/buck2-${BUCK2_ARCH}.zst"

# Persistent host-side cache (NOT /tmp — systemd-tmpfiles / reboots clear that).
# Lives next to apt caches under /var/cache/orion-image and is never deleted by
# this script after a successful download. Override with BUCK2_CACHE_DIR=...
BUCK2_CACHE_DIR="${BUCK2_CACHE_DIR:-/var/cache/orion-image/buck2}"
BUCK2_TARBALL="${BUCK2_CACHE_DIR}/buck2-${BUCK2_ARCH}.zst"
# Host-side apt caches, bind-mounted into the chroot so repeated image builds
# reuse already-downloaded indexes and .deb packages. They live on the host and
# are unmounted before the image is sealed, so cached data is NOT baked into the
# final image. Override with APT_LISTS_DIR / APT_CACHE_DIR.
APT_LISTS_DIR="${APT_LISTS_DIR:-/var/cache/orion-image/apt-lists}"
APT_CACHE_DIR="${APT_CACHE_DIR:-/var/cache/orion-image/apt-archives}"
# CN apt mirrors written into the image before chroot apt-get. Official
# deb.debian.org is often too slow from CN build hosts (apt appears "stuck").
# Override, e.g.:
#   APT_MIRROR=https://mirrors.aliyun.com/debian \
#   APT_SECURITY_MIRROR=https://mirrors.aliyun.com/debian-security \
#     sudo -E bash scripts/build-custom-image.sh
APT_MIRROR="${APT_MIRROR:-https://mirrors.ustc.edu.cn/debian}"
APT_SECURITY_MIRROR="${APT_SECURITY_MIRROR:-https://mirrors.ustc.edu.cn/debian-security}"

# CPython tarball for prelude remote_python_toolchain / toolchains//:cpython_archive.
# Baked into a local GitHub HTTPS mirror (see chroot) so buck2 http_archive keeps
# the upstream URL but reads the file from disk on workers.
CPYTHON_VERSION="3.13.6"
CPYTHON_BUILD="20250807"
CPYTHON_ARCH="x86_64-unknown-linux-gnu"
CPYTHON_TARBALL="/tmp/cpython-${CPYTHON_VERSION}-${CPYTHON_ARCH}.tar.gz"
CPYTHON_TARBALL_URL="https://github.com/astral-sh/python-build-standalone/releases/download/${CPYTHON_BUILD}/cpython-${CPYTHON_VERSION}+${CPYTHON_BUILD}-${CPYTHON_ARCH}-install_only_stripped.tar.gz"
CPYTHON_MIRROR_REL_PATH="astral-sh/python-build-standalone/releases/download/${CPYTHON_BUILD}/cpython-${CPYTHON_VERSION}+${CPYTHON_BUILD}-${CPYTHON_ARCH}-install_only_stripped.tar.gz"
CPYTHON_SHA256="e3e280d4b1ead63de6ebc9816de71792fc8c71b7a6a999ea82f937047beba037"

ROOT_SSH_KEY="ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIF9LTEGIaaad0XP4qUfBoVRgeOg+G36jIWIiqIWP/k4g"

NBD_DEV="/dev/nbd0"
NBD_PART="${NBD_DEV}p1"

# When run via sudo, root creates files under the invoking user's OUTPUT_DIR.
# qlean / orion-scheduler must be able to read/write debian-13-buck2.json.
fix_qlean_ownership() {
    local owner="${SUDO_USER:-}"
    if [ -z "$owner" ] || [ "$owner" = "root" ]; then
        return 0
    fi
    echo "[build-custom-image] chown $owner: $OUTPUT_DIR ($IMAGE_NAME artifacts)"
    chown -R "$owner:$owner" "$IMAGE_DIR" "$BASE_DIR" 2>/dev/null || true
    chown "$owner:$owner" "$PUBLISHED_IMAGE" "$PUBLISHED_JSON" 2>/dev/null || true
    chmod 644 "$PUBLISHED_JSON" "$IMAGE_DIR/checksums" 2>/dev/null || true
    chmod 644 "$PUBLISHED_IMAGE" "$CUSTOM_IMAGE" 2>/dev/null || true
}

# ============================================================================
# Cleanup trap — always release mounts and NBD even on failure / Ctrl-C
# ============================================================================
MOUNT_DIR=""
BUILD_STAGE="init"
CLEANUP_RUNNING=0

log_stage() {
    BUILD_STAGE="$1"
    echo "[build-custom-image] >>> stage: $BUILD_STAGE"
}

log_cmd() {
    echo "[build-custom-image] \$ $*"
}

# Unmount image tree safely. Never `rm -rf` the mount dir while /proc|/sys|/dev
# are still bind-mounted — that prints endless "Operation not permitted" under
# proc and can wedge the host. Prefer lazy umount if busy after Ctrl-C.
unmount_image_tree() {
    local root="$1"
    local sub tries

    [ -n "$root" ] && [ -d "$root" ] || return 0

    # Drop processes still in the chroot (apt-get, etc.) so umount can succeed.
    if command -v fuser >/dev/null 2>&1; then
        sudo fuser -km "$root" >/dev/null 2>&1 || true
        sleep 0.5
    fi

    for sub in var/lib/apt/lists var/cache/apt/archives proc sys dev; do
        if mountpoint -q "$root/$sub" 2>/dev/null; then
            sudo umount "$root/$sub" 2>/dev/null || sudo umount -l "$root/$sub" 2>/dev/null || true
        fi
    done

    # Any nested mounts left under $root (e.g. unexpected bind)?
    if command -v findmnt >/dev/null 2>&1; then
        findmnt -Rnn -o TARGET --target "$root" 2>/dev/null \
            | awk -v r="$root" '$0 != r { print }' \
            | sort -r \
            | while IFS= read -r tgt; do
                sudo umount "$tgt" 2>/dev/null || sudo umount -l "$tgt" 2>/dev/null || true
              done
    fi

    if mountpoint -q "$root" 2>/dev/null; then
        for tries in 1 2 3 4 5; do
            sudo umount "$root" 2>/dev/null && break
            sudo umount -l "$root" 2>/dev/null && break
            sleep 1
        done
    fi

    if mountpoint -q "$root" 2>/dev/null; then
        echo "[build-custom-image] WARNING: $root still mounted; NOT removing dir (avoid rm -rf on /proc)" >&2
        findmnt -R "$root" 2>/dev/null || true
        return 1
    fi

    rmdir "$root" 2>/dev/null || true
    return 0
}

# Disconnect NBD and drop any leftover qemu-nbd holding this image's write lock.
release_nbd() {
    local img="${1:-}"
    if [ -n "$img" ]; then
        local pids
        pids=$(ps -eo pid=,args= | awk -v img="$img" '
            $0 ~ /qemu-nbd/ && index($0, img) { print $1 }
        ')
        if [ -n "$pids" ]; then
            echo "[build-custom-image] Killing stale qemu-nbd for $img: $pids"
            # shellcheck disable=SC2086
            sudo kill $pids 2>/dev/null || true
            sleep 1
            # shellcheck disable=SC2086
            sudo kill -9 $pids 2>/dev/null || true
        fi
    fi
    sudo qemu-nbd --disconnect "$NBD_DEV" 2>/dev/null || true
}

cleanup() {
    local rc=$?
    # Re-entrant: a second Ctrl-C while umount is in progress must not start
    # another cleanup (or worse, an rm) on a half-unmounted tree.
    if [ "$CLEANUP_RUNNING" -ne 0 ]; then
        return
    fi
    CLEANUP_RUNNING=1
    # Ignore further INT/TERM during cleanup so we finish umounts.
    trap '' INT TERM HUP
    set +e
    if [ $rc -ne 0 ]; then
        echo "[build-custom-image] FAILED with exit code $rc (stage: ${BUILD_STAGE:-unknown})" >&2
    fi
    if [ -n "$MOUNT_DIR" ]; then
        echo "[build-custom-image] cleanup: unmounting $MOUNT_DIR (stage was: ${BUILD_STAGE:-unknown})"
        unmount_image_tree "$MOUNT_DIR" || true
        MOUNT_DIR=""
    fi
    release_nbd "${CUSTOM_IMAGE:-}"
    exit $rc
}
trap cleanup EXIT INT TERM HUP

# ============================================================================
# Download the upstream base image (qcow2) + verify SHA512
# ============================================================================
download_base_image() {
    echo "[build-custom-image] Base image not found at $BASE_IMAGE"
    echo "[build-custom-image] Downloading base image from $BASE_IMAGE_URL..."
    mkdir -p "$BASE_DIR"

    local part="$BASE_IMAGE.part"
    rm -f "$part"
    curl -fSL --retry 3 --retry-delay 2 \
        --connect-timeout 20 --max-time 1800 \
        -o "$part" "$BASE_IMAGE_URL"

    echo "[build-custom-image] Verifying SHA512 checksum..."
    local expected
    expected=$(curl -fsSL --connect-timeout 20 --max-time 60 "$BASE_CHECKSUM_URL" \
        | awk -v f="$BASE_IMAGE_FILE" '$2 == f { print $1 }')
    if [ -z "$expected" ]; then
        echo "[build-custom-image] ERROR: no checksum entry for $BASE_IMAGE_FILE in $BASE_CHECKSUM_URL"
        rm -f "$part"
        exit 1
    fi

    local actual
    actual=$(sha512sum "$part" | awk '{ print $1 }')
    if [ "$expected" != "$actual" ]; then
        echo "[build-custom-image] ERROR: checksum mismatch"
        echo "[build-custom-image]   expected: $expected"
        echo "[build-custom-image]   actual:   $actual"
        rm -f "$part"
        exit 1
    fi

    mv "$part" "$BASE_IMAGE"
    echo "[build-custom-image] Base image downloaded and verified ($(du -sh "$BASE_IMAGE" | cut -f1))"
}

# ============================================================================
# Pre-flight: ensure base image (download if missing)
# ============================================================================
log_stage "preflight"
echo "[build-custom-image] Starting custom image build..."
echo "[build-custom-image] OUTPUT_DIR=$OUTPUT_DIR"
echo "[build-custom-image] CUSTOM_IMAGE=$CUSTOM_IMAGE"

if [ ! -f "$BASE_IMAGE" ]; then
    download_base_image
fi

# Kernel/initrd are resolved later: if not already present alongside the base
# image, they are extracted from the mounted image's /boot in Stage 3.
KERNEL=$(ls "$BASE_DIR"/vmlinuz-* 2>/dev/null | head -n1 || true)
INITRD=$(ls "$BASE_DIR"/initrd.img-* 2>/dev/null | head -n1 || true)

mkdir -p "$IMAGE_DIR"

# Recover from a previous Ctrl-C that left bind mounts behind. Never rm -rf
# those dirs while /proc is still mounted.
if compgen -G '/tmp/custom-image-mnt.*' >/dev/null 2>&1; then
    echo "[build-custom-image] Cleaning stale mount dirs under /tmp/custom-image-mnt.*"
    for stale in /tmp/custom-image-mnt.*; do
        [ -d "$stale" ] || continue
        unmount_image_tree "$stale" || true
    done
fi
release_nbd "$CUSTOM_IMAGE"

# ============================================================================
# Stage 1: Copy + resize qcow2
# ============================================================================
log_stage "1-copy-resize"
echo "[build-custom-image] Copying base image..."
cp "$BASE_IMAGE" "$CUSTOM_IMAGE"

echo "[build-custom-image] Resizing image to $IMAGE_SIZE..."
qemu-img resize "$CUSTOM_IMAGE" "$IMAGE_SIZE"

# ============================================================================
# Stage 2: Download Rust on host (avoids needing DNS inside chroot)
# ============================================================================
# Xuanwu (and most rustup mirrors) require /dist/YYYY-MM-DD/rust-VER-ARCH.tar.xz.
# Resolve that date + xz_hash from the channel manifest so RUST_VERSION bumps
# don't leave a stale RUST_DIST_DATE and 404.
resolve_rust_channel() {
    local channel_url="${RUSTUP_DIST_SERVER%/}/dist/channel-rust-${RUST_VERSION}.toml"
    local channel_file tmp_date tmp_hash

    if [ -n "$RUST_DIST_DATE" ] && [ -n "$RUST_SHA256" ]; then
        RUST_TARBALL_URL="${RUSTUP_DIST_SERVER%/}/dist/${RUST_DIST_DATE}/rust-${RUST_VERSION}-${RUST_ARCH}.tar.xz"
        echo "[build-custom-image] Using overridden Rust dist: ${RUST_TARBALL_URL}"
        return 0
    fi

    echo "[build-custom-image] Resolving Rust ${RUST_VERSION} channel: ${channel_url}"
    channel_file=$(mktemp)
    if ! curl -fsSL --connect-timeout 20 --max-time 60 --retry 3 \
            -o "$channel_file" "$channel_url"; then
        rm -f "$channel_file"
        echo "[build-custom-image] ERROR: failed to fetch Rust channel from $channel_url" >&2
        exit 56
    fi

    # Read from a file (not a pipe): under `set -o pipefail`, feeding the
    # whole channel through a pipe and exiting early triggers SIGPIPE 141.
    tmp_date=$(awk '
        /^date = "/ {
            sub(/^date = "/, "")
            sub(/"$/, "")
            print
            exit
        }
    ' "$channel_file")
    tmp_hash=$(awk -v arch="$RUST_ARCH" '
        $0 == "[pkg.rust.target." arch "]" { p = 1; next }
        /^\[/ { p = 0 }
        p && /^xz_hash = "/ {
            sub(/^xz_hash = "/, "")
            sub(/"$/, "")
            print
            exit
        }
    ' "$channel_file")
    rm -f "$channel_file"

    if [ -z "$tmp_date" ] || [ -z "$tmp_hash" ]; then
        echo "[build-custom-image] ERROR: could not parse date/xz_hash for ${RUST_ARCH} from $channel_url" >&2
        exit 1
    fi

    RUST_DIST_DATE="${RUST_DIST_DATE:-$tmp_date}"
    RUST_SHA256="${RUST_SHA256:-$tmp_hash}"
    RUST_TARBALL_URL="${RUSTUP_DIST_SERVER%/}/dist/${RUST_DIST_DATE}/rust-${RUST_VERSION}-${RUST_ARCH}.tar.xz"
    echo "[build-custom-image] Rust dist date=${RUST_DIST_DATE} sha256=${RUST_SHA256}"
    echo "[build-custom-image] Rust URL: ${RUST_TARBALL_URL}"
}

# Reuse a cached archive only if SHA256 matches. A flaky download can leave a
# truncated archive that still passes `[ -f ]` and then fails later in chroot.
verify_rust_tarball() {
    local actual
    actual=$(sha256sum "$RUST_TARBALL" | awk '{ print $1 }')
    if [ "$RUST_SHA256" = "$actual" ]; then
        return 0
    fi
    echo "[build-custom-image] Rust tarball checksum mismatch (will re-download)" >&2
    echo "[build-custom-image]   expected: $RUST_SHA256" >&2
    echo "[build-custom-image]   actual:   $actual" >&2
    echo "[build-custom-image]   size:     $(du -sh "$RUST_TARBALL" | cut -f1) ($(stat -c%s "$RUST_TARBALL") bytes)" >&2
    return 1
}

download_rust_tarball() {
    echo "[build-custom-image] Downloading Rust ${RUST_VERSION} from ${RUSTUP_DIST_SERVER}..."
    # Resume + retries: toolchain mirrors can still truncate mid-transfer on
    # flaky links (same class of failure as GitHub TLS resets for buck2).
    if ! curl -fL --connect-timeout 30 --retry 5 --retry-delay 3 --retry-all-errors \
            -C - -o "$RUST_TARBALL" "$RUST_TARBALL_URL"; then
        rm -f "$RUST_TARBALL"
        echo "[build-custom-image] ERROR: failed to download Rust from $RUST_TARBALL_URL" >&2
        echo "[build-custom-image] Pre-download it manually then re-run, e.g.:" >&2
        echo "[build-custom-image]   curl -fL -o $RUST_TARBALL \"$RUST_TARBALL_URL\"" >&2
        exit 56
    fi
    echo "[build-custom-image] Rust tarball downloaded: $(du -sh "$RUST_TARBALL" | cut -f1)"
}

resolve_rust_channel

if [ -f "$RUST_TARBALL" ] && verify_rust_tarball; then
    echo "[build-custom-image] Using cached Rust tarball: $(du -sh "$RUST_TARBALL" | cut -f1)"
else
    rm -f "$RUST_TARBALL"
    download_rust_tarball
    if ! verify_rust_tarball; then
        rm -f "$RUST_TARBALL"
        echo "[build-custom-image] ERROR: Rust tarball checksum mismatch after download" >&2
        exit 1
    fi
fi

# ============================================================================
# Stage 2b: Download buck2 on host (avoids needing reliable network in chroot)
# ============================================================================
# Cache under $BUCK2_CACHE_DIR (durable). On success the file is kept forever by
# this script — only replace it when missing/corrupt. Partial downloads go to
# a sibling .partial file so a failed curl never clobbers a good cache.
mkdir -p "$BUCK2_CACHE_DIR"
# One-time migrate from the old /tmp location if present.
for _old_buck2 in \
    "/tmp/buck2-${BUCK2_ARCH}.zst" \
    "/tmp/buck2-x86_64-unknown-linux-musl.zst" \
    "/tmp/buck2-x86_64-unknown-linux-gnu.zst"; do
    if [ ! -f "$BUCK2_TARBALL" ] && [ -s "$_old_buck2" ]; then
        echo "[build-custom-image] Migrating buck2 cache $_old_buck2 -> $BUCK2_TARBALL"
        cp -a "$_old_buck2" "$BUCK2_TARBALL"
        break
    fi
done
unset _old_buck2

verify_buck2_tarball() {
    [ -s "$BUCK2_TARBALL" ] || return 1
    if command -v zstd >/dev/null 2>&1; then
        zstd -t "$BUCK2_TARBALL" >/dev/null 2>&1 || return 1
    fi
    return 0
}

download_buck2_tarball() {
    local part="${BUCK2_TARBALL}.partial"
    echo "[build-custom-image] Downloading buck2 ${BUCK2_VERSION} -> $BUCK2_TARBALL"
    rm -f "$part"
    if ! curl -fL --connect-timeout 30 --retry 5 --retry-delay 3 --retry-all-errors \
            -C - -o "$part" "$BUCK2_URL"; then
        rm -f "$part"
        echo "[build-custom-image] ERROR: failed to download buck2 from $BUCK2_URL" >&2
        echo "[build-custom-image] Pre-download it manually then re-run, e.g.:" >&2
        echo "[build-custom-image]   sudo mkdir -p $BUCK2_CACHE_DIR" >&2
        echo "[build-custom-image]   sudo curl -fL -o $BUCK2_TARBALL \"$BUCK2_URL\"" >&2
        exit 56
    fi
    if ! [ -s "$part" ]; then
        rm -f "$part"
        echo "[build-custom-image] ERROR: buck2 download empty: $part" >&2
        exit 1
    fi
    if command -v zstd >/dev/null 2>&1 && ! zstd -t "$part" >/dev/null 2>&1; then
        rm -f "$part"
        echo "[build-custom-image] ERROR: buck2 archive failed zstd integrity check" >&2
        exit 1
    fi
    mv -f "$part" "$BUCK2_TARBALL"
    chmod 644 "$BUCK2_TARBALL" 2>/dev/null || true
    echo "[build-custom-image] buck2 cached: $(du -sh "$BUCK2_TARBALL" | cut -f1) -> $BUCK2_TARBALL"
}

if verify_buck2_tarball; then
    echo "[build-custom-image] Using cached buck2 archive: $(du -sh "$BUCK2_TARBALL" | cut -f1) ($BUCK2_TARBALL)"
else
    rm -f "$BUCK2_TARBALL"
    download_buck2_tarball
fi

# ============================================================================
# Stage 2c: Download cpython_archive tarball on host (buckal / prelude python bootstrap)
# ============================================================================
if [ ! -f "$CPYTHON_TARBALL" ]; then
    echo "[build-custom-image] Downloading CPython ${CPYTHON_VERSION} (${CPYTHON_BUILD})..."
    if ! curl -fL --connect-timeout 30 --retry 5 --retry-delay 3 --retry-all-errors \
            -C - -o "$CPYTHON_TARBALL" "$CPYTHON_TARBALL_URL"; then
        rm -f "$CPYTHON_TARBALL"
        echo "[build-custom-image] ERROR: failed to download CPython from $CPYTHON_TARBALL_URL" >&2
        echo "[build-custom-image] Pre-download it manually then re-run, e.g.:" >&2
        echo "[build-custom-image]   curl -fL -o $CPYTHON_TARBALL \"$CPYTHON_TARBALL_URL\"" >&2
        exit 56
    fi
    echo "[build-custom-image] CPython tarball downloaded: $(du -sh "$CPYTHON_TARBALL" | cut -f1)"
else
    echo "[build-custom-image] Using cached CPython tarball: $(du -sh "$CPYTHON_TARBALL" | cut -f1)"
fi

echo "[build-custom-image] Verifying CPython SHA256..."
actual_cpython_sha=$(sha256sum "$CPYTHON_TARBALL" | awk '{ print $1 }')
if [ "$CPYTHON_SHA256" != "$actual_cpython_sha" ]; then
    echo "[build-custom-image] ERROR: CPython checksum mismatch" >&2
    echo "[build-custom-image]   expected: $CPYTHON_SHA256" >&2
    echo "[build-custom-image]   actual:   $actual_cpython_sha" >&2
    exit 1
fi

# ============================================================================
# Stage 3: NBD attach + filesystem grow + mount
# ============================================================================
log_stage "3-nbd-mount"
wait_for() {
    # wait_for <path> <max-tries-of-0.25s>
    local path="$1" tries="${2:-40}" i
    for ((i = 0; i < tries; i++)); do
        [ -e "$path" ] && return 0
        sleep 0.25
    done
    return 1
}

if [ ! -e "$NBD_DEV" ]; then
    echo "[build-custom-image] Loading NBD kernel module..."
    sudo modprobe nbd max_part=8
    if ! wait_for "$NBD_DEV" 20; then
        echo "[build-custom-image] ERROR: $NBD_DEV not found after loading module" >&2
        exit 1
    fi
fi

echo "[build-custom-image] Connecting NBD device..."
release_nbd "$CUSTOM_IMAGE"
sudo qemu-nbd -c "$NBD_DEV" "$CUSTOM_IMAGE"

sudo udevadm settle
if ! wait_for "$NBD_PART" 40; then
    echo "[build-custom-image] ERROR: $NBD_PART did not appear" >&2
    exit 1
fi

echo "[build-custom-image] Extending partition to fill $IMAGE_SIZE..."
sudo growpart "$NBD_DEV" 1
sudo udevadm settle
wait_for "$NBD_PART" 20 || true

echo "[build-custom-image] Running e2fsck..."
sudo e2fsck -fy "$NBD_PART"

echo "[build-custom-image] Extending filesystem..."
sudo resize2fs "$NBD_PART"

MOUNT_DIR=$(mktemp -d /tmp/custom-image-mnt.XXXXXX)
echo "[build-custom-image] Mounting image at $MOUNT_DIR..."
sudo mount "$NBD_PART" "$MOUNT_DIR"

# Extract kernel/initrd from the image's /boot if they weren't supplied
# alongside the base image. The generic cloud qcow2 ships them inside /boot,
# so we lift them out here for the direct-kernel-boot path (Stage 6).
if [ -z "$KERNEL" ] || [ ! -f "$KERNEL" ] || [ -z "$INITRD" ] || [ ! -f "$INITRD" ]; then
    echo "[build-custom-image] Extracting kernel/initrd from image /boot..."
    sudo bash -c "cp '$MOUNT_DIR'/boot/vmlinuz-* '$BASE_DIR'/ && cp '$MOUNT_DIR'/boot/initrd.img-* '$BASE_DIR'/"
    KERNEL=$(ls "$BASE_DIR"/vmlinuz-* 2>/dev/null | head -n1 || true)
    INITRD=$(ls "$BASE_DIR"/initrd.img-* 2>/dev/null | head -n1 || true)
    if [ ! -f "$KERNEL" ] || [ ! -f "$INITRD" ]; then
        echo "[build-custom-image] ERROR: failed to extract kernel/initrd from $MOUNT_DIR/boot"
        exit 1
    fi
fi
echo "[build-custom-image] Using kernel: $(basename "$KERNEL")"
echo "[build-custom-image] Using initrd: $(basename "$INITRD")"

# Bind mounts for chroot
sudo mount --bind /proc "$MOUNT_DIR/proc"
sudo mount --bind /sys "$MOUNT_DIR/sys"
sudo mount --bind /dev "$MOUNT_DIR/dev"
sudo cp --remove-destination /etc/resolv.conf "$MOUNT_DIR/etc/resolv.conf"

# Bind-mount host apt caches so `apt-get update` / `apt-get install` reuse
# previously downloaded indexes and .deb files. Because these are bind mounts
# onto the host, cached data stays on the host and is unmounted before the
# image is compacted (Stage 5), so it doesn't bloat the final image.
echo "[build-custom-image] Using apt lists cache: $APT_LISTS_DIR"
echo "[build-custom-image] Using apt archive cache: $APT_CACHE_DIR"
sudo mkdir -p "$APT_LISTS_DIR" "$APT_CACHE_DIR/partial"
sudo mkdir -p "$MOUNT_DIR/var/lib/apt/lists" "$MOUNT_DIR/var/cache/apt/archives/partial"
sudo mount --bind "$APT_LISTS_DIR" "$MOUNT_DIR/var/lib/apt/lists"
sudo mount --bind "$APT_CACHE_DIR" "$MOUNT_DIR/var/cache/apt/archives"

# Point the image at a CN Debian mirror before chroot apt-get. Debian 13 cloud
# images use mirror+file:///etc/apt/mirrors/*.list (not classic sources.list).
if [ -d "$MOUNT_DIR/etc/apt/mirrors" ]; then
    echo "[build-custom-image] Configuring apt mirrors:"
    echo "[build-custom-image]   debian:          $APT_MIRROR"
    echo "[build-custom-image]   debian-security: $APT_SECURITY_MIRROR"
    printf '%s\n' "$APT_MIRROR" | sudo tee "$MOUNT_DIR/etc/apt/mirrors/debian.list" >/dev/null
    printf '%s\n' "$APT_SECURITY_MIRROR" | sudo tee "$MOUNT_DIR/etc/apt/mirrors/debian-security.list" >/dev/null
fi
# Avoid hanging forever on flaky links during apt-get update/install.
sudo tee "$MOUNT_DIR/etc/apt/apt.conf.d/80orion-retries" >/dev/null <<'APT_CONF'
Acquire::Retries "5";
Acquire::http::Timeout "30";
Acquire::https::Timeout "30";
APT_CONF

# Copy Rust tarball into the image's /tmp before entering chroot
sudo cp "$RUST_TARBALL" "$MOUNT_DIR/tmp/rust.tar.xz"
# Copy buck2 archive in too so the chroot reuses it instead of hitting GitHub.
sudo cp "$BUCK2_TARBALL" "$MOUNT_DIR/tmp/buck2.zst"
# Copy cpython_archive tarball for the in-image GitHub mirror (toolchains unchanged).
sudo cp "$CPYTHON_TARBALL" "$MOUNT_DIR/tmp/cpython.tar.gz"

# Resolve github.com while the host still has normal DNS (chroot will point it at 127.0.0.1).
GITHUB_UPSTREAM_IP=$(getent ahostsv4 github.com 2>/dev/null | awk '{ print $1; exit }')
if [ -z "$GITHUB_UPSTREAM_IP" ] || [ "$GITHUB_UPSTREAM_IP" = "127.0.0.1" ]; then
    GITHUB_UPSTREAM_IP="20.205.243.166"
    echo "[build-custom-image] WARNING: could not resolve github.com; using fallback ${GITHUB_UPSTREAM_IP}"
else
    echo "[build-custom-image] github.com upstream IP for mirror proxy: ${GITHUB_UPSTREAM_IP}"
fi

# ============================================================================
# Stage 4: Install everything inside the chroot (no VM boot needed)
# ============================================================================
log_stage "4-chroot-install"
CHROOT_LOG="/tmp/chroot_install.log"
echo "[build-custom-image] Installing toolchain into image (chroot)..."
echo "[build-custom-image] chroot log: $CHROOT_LOG"
# Disable errexit and pipefail for chroot|tee: with pipefail off, set -e would
# still abort on tee(1) failure even when chroot succeeded; capture PIPESTATUS
# explicitly instead.
set +e
set +o pipefail
sudo BUCK2_URL="$BUCK2_URL" BUCK2_VERSION="$BUCK2_VERSION" \
     CPYTHON_MIRROR_REL_PATH="$CPYTHON_MIRROR_REL_PATH" \
     GITHUB_UPSTREAM_IP="$GITHUB_UPSTREAM_IP" \
     ROOT_SSH_KEY="$ROOT_SSH_KEY" \
     chroot "$MOUNT_DIR" /bin/bash <<'CHROOT_EOF' 2>&1 | tee "$CHROOT_LOG"
set -eu
export HOME=/root
export DEBIAN_FRONTEND=noninteractive
export PATH=/root/.cargo/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin

# Block daemons from starting during apt-get install inside the chroot.
cat > /usr/sbin/policy-rc.d <<'POLICY_EOF'
#!/bin/sh
exit 101
POLICY_EOF
chmod +x /usr/sbin/policy-rc.d

echo "=== [chroot] Disk space before install ==="
df -h /

echo "=== [chroot] Extracting Rust toolchain ==="
tar -xJf /tmp/rust.tar.xz -C /tmp/
bash /tmp/rust-*/install.sh --destdir="" --prefix=/root/.cargo --without=rust-docs
rm -rf /tmp/rust.tar.xz /tmp/rust-*

ln -sf /root/.cargo/bin/rustc /usr/local/bin/rustc
ln -sf /root/.cargo/bin/cargo /usr/local/bin/cargo
echo "=== [chroot] Rust version: $(/root/.cargo/bin/rustc --version) ==="

echo "=== [chroot] Installing apt packages ==="
echo "=== [chroot] apt mirrors: ==="
cat /etc/apt/mirrors/debian.list 2>/dev/null || true
cat /etc/apt/mirrors/debian-security.list 2>/dev/null || true
apt-get update
apt-get install -y \
    clang lld pkg-config protobuf-compiler zstd fuse curl git nginx \
    seccomp libseccomp-dev libpython3-dev openssl libssl-dev build-essential ca-certificates

echo "=== [chroot] Verifying installed tools ==="
git --version
clang --version | head -1
ld.lld --version | head -1
protoc --version
zstd --version | head -1

echo "=== [chroot] Installing buck2 (${BUCK2_VERSION}) ==="
# Prefer the archive copied in from the host ($MOUNT_DIR/tmp/buck2.zst). Only
# fall back to downloading inside the chroot if it's missing. GitHub release
# downloads frequently get TLS-reset (curl 56: unexpected eof) on flaky
# networks, so the fallback retries with resume.
if [ -s /tmp/buck2.zst ]; then
    echo "=== [chroot] Using buck2 archive provided by host ==="
else
    rm -f /tmp/buck2.zst
    buck2_downloaded=0
    for attempt in 1 2 3 4 5; do
        if curl -fL --connect-timeout 30 --retry 5 --retry-delay 3 --retry-all-errors \
                -C - -o /tmp/buck2.zst "$BUCK2_URL"; then
            buck2_downloaded=1
            break
        fi
        echo "=== [chroot] buck2 download attempt ${attempt} failed; retrying in 5s ==="
        sleep 5
    done
    if [ "$buck2_downloaded" -ne 1 ]; then
        echo "=== [chroot] ERROR: failed to download buck2 from $BUCK2_URL after retries ===" >&2
        exit 56
    fi
fi
zstd -d /tmp/buck2.zst -o /usr/local/bin/buck2
chmod +x /usr/local/bin/buck2
rm -f /tmp/buck2.zst
/usr/local/bin/buck2 --version

echo "=== [chroot] Installing local GitHub mirror for cpython_archive ==="
if [ ! -s /tmp/cpython.tar.gz ]; then
    echo "=== [chroot] ERROR: missing /tmp/cpython.tar.gz (host should have copied it) ===" >&2
    exit 1
fi
MIRROR_ROOT=/var/cache/orion-github-mirror
CERT_DIR=/etc/orion-github-mirror
mkdir -p "${MIRROR_ROOT}/$(dirname "${CPYTHON_MIRROR_REL_PATH}")" "${CERT_DIR}"
mv /tmp/cpython.tar.gz "${MIRROR_ROOT}/${CPYTHON_MIRROR_REL_PATH}"

# buck2/rustls rejects openssl req -x509 leaf certs (CaUsedAsEndEntity). Use a
# small PKI: CA cert -> system trust store; leaf server cert -> nginx.
cat > "${CERT_DIR}/openssl-ca.cnf" <<'OPENSSL_CA_EOF'
[req]
distinguished_name = dn
x509_extensions = v3_ca
prompt = no

[dn]
CN = Orion GitHub Mirror CA

[v3_ca]
basicConstraints = critical, CA:true, pathlen:0
keyUsage = critical, keyCertSign, cRLSign
subjectKeyIdentifier = hash
authorityKeyIdentifier = keyid:always,issuer
OPENSSL_CA_EOF

cat > "${CERT_DIR}/openssl-leaf.cnf" <<'OPENSSL_LEAF_EOF'
[req]
distinguished_name = dn
req_extensions = v3_req
prompt = no

[dn]
CN = github.com

[v3_req]
basicConstraints = critical, CA:FALSE
keyUsage = critical, digitalSignature, keyEncipherment
extendedKeyUsage = serverAuth
subjectAltName = @alt_names

[alt_names]
DNS.1 = github.com
OPENSSL_LEAF_EOF

openssl genrsa -out "${CERT_DIR}/ca.key" 4096
openssl req -x509 -new -nodes \
    -key "${CERT_DIR}/ca.key" \
    -days 3650 \
    -out "${CERT_DIR}/ca.crt" \
    -config "${CERT_DIR}/openssl-ca.cnf"

openssl genrsa -out "${CERT_DIR}/github.com.key" 2048
openssl req -new \
    -key "${CERT_DIR}/github.com.key" \
    -out "${CERT_DIR}/github.com.csr" \
    -config "${CERT_DIR}/openssl-leaf.cnf"

openssl x509 -req \
    -in "${CERT_DIR}/github.com.csr" \
    -CA "${CERT_DIR}/ca.crt" \
    -CAkey "${CERT_DIR}/ca.key" \
    -CAcreateserial \
    -out "${CERT_DIR}/github.com.crt" \
    -days 3650 \
    -extensions v3_req \
    -extfile "${CERT_DIR}/openssl-leaf.cnf"

# Trust the CA only (not the leaf); rustls validates the server leaf against it.
cp "${CERT_DIR}/ca.crt" /usr/local/share/ca-certificates/orion-github-mirror-ca.crt
update-ca-certificates

# Verify leaf is not flagged as a CA (would trigger CaUsedAsEndEntity in rustls).
cert_text=$(openssl x509 -in "${CERT_DIR}/github.com.crt" -noout -text)
if echo "$cert_text" | grep -q "CA:TRUE"; then
    echo "=== [chroot] ERROR: github.com leaf cert has CA:TRUE extension ===" >&2
    exit 1
fi

# Present leaf + CA chain to TLS clients.
cat "${CERT_DIR}/github.com.crt" "${CERT_DIR}/ca.crt" > "${CERT_DIR}/github.com-chain.crt"

cat > /etc/nginx/sites-available/orion-github-mirror <<NGINX_EOF
server {
    listen 443 ssl;
    server_name github.com;

    ssl_certificate     ${CERT_DIR}/github.com-chain.crt;
    ssl_certificate_key ${CERT_DIR}/github.com.key;

    location = /${CPYTHON_MIRROR_REL_PATH} {
        alias ${MIRROR_ROOT}/${CPYTHON_MIRROR_REL_PATH};
    }

    location / {
        proxy_pass https://${GITHUB_UPSTREAM_IP}\$request_uri;
        proxy_ssl_server_name on;
        proxy_set_header Host github.com;
    }
}
NGINX_EOF

ln -sf /etc/nginx/sites-available/orion-github-mirror /etc/nginx/sites-enabled/orion-github-mirror
rm -f /etc/nginx/sites-enabled/default
if ! grep -q '[[:space:]]github.com$' /etc/hosts; then
    echo "127.0.0.1 github.com" >> /etc/hosts
fi
systemctl enable nginx
echo "=== [chroot] cpython mirror path: https://github.com/${CPYTHON_MIRROR_REL_PATH} ==="

echo "=== [chroot] Installing SSH key for root ==="
mkdir -p /root/.ssh
chmod 700 /root/.ssh
echo "$ROOT_SSH_KEY" > /root/.ssh/authorized_keys
chmod 600 /root/.ssh/authorized_keys

echo "=== [chroot] Cleaning temp files (host apt caches kept) ==="
# NOTE: do NOT run `apt-get clean` or wipe /var/lib/apt/lists here. Both
# /var/lib/apt/lists and /var/cache/apt/archives are bind-mounted to the host
# cache, so cleaning them would wipe the cross-build cache. Cached data is
# unmounted (not copied) before the image is sealed, so it doesn't end up in
# the final image anyway.
rm -f /var/cache/apt/*.bin
# Only clear known scratch files — never `rm -rf /tmp/*` from the host against
# a still-mounted image tree (bind-mounted /proc cannot be deleted that way).
rm -rf /tmp/rust-* /tmp/*.tar.* /tmp/*.zst /tmp/*.gz /var/tmp/* 2>/dev/null || true
rm -f /usr/sbin/policy-rc.d

echo "=== [chroot] Clearing cloud-init state ==="
rm -rf /var/lib/cloud/data/* /var/lib/cloud/instance/* 2>/dev/null || true

# Zero unused blocks so qemu-img convert below can drop them, producing a
# much smaller final qcow2. Hitting ENOSPC here is expected and harmless.
echo "=== [chroot] Zeroing free space (helps qcow2 compact; ENOSPC expected) ==="
dd if=/dev/zero of=/EMPTY bs=4M conv=fdatasync 2>/dev/null || true
rm -f /EMPTY
sync

echo "=== [chroot] Disk space after install ==="
df -h /
echo "=== [chroot] install finished (exit 0) ==="
CHROOT_EOF
_pipe_status=("${PIPESTATUS[@]}")
CHROOT_RC="${_pipe_status[0]:-255}"
TEE_RC="${_pipe_status[1]:-255}"
PIPELINE_RC=$?
set -e
set -o pipefail

echo "[build-custom-image] chroot pipeline done: pipeline_rc=$PIPELINE_RC chroot_rc=$CHROOT_RC tee_rc=$TEE_RC"
if [ -f "$CHROOT_LOG" ]; then
    echo "[build-custom-image] chroot log: $(wc -l < "$CHROOT_LOG") lines, $(stat -c '%s bytes, owner %U:%G' "$CHROOT_LOG" 2>/dev/null || ls -la "$CHROOT_LOG")"
else
    echo "[build-custom-image] WARNING: chroot log missing at $CHROOT_LOG" >&2
fi

if [ "$CHROOT_RC" -ne 0 ]; then
    echo "[build-custom-image] ERROR: chroot install failed (exit $CHROOT_RC)" >&2
    echo "[build-custom-image] Last 20 lines of $CHROOT_LOG:" >&2
    tail -20 "$CHROOT_LOG" >&2 2>/dev/null || true
    exit "$CHROOT_RC"
fi
if [ "$TEE_RC" -ne 0 ]; then
    echo "[build-custom-image] WARNING: tee failed (exit $TEE_RC) but chroot succeeded; continuing" >&2
    echo "[build-custom-image] tee target: $CHROOT_LOG ($(ls -la "$CHROOT_LOG" 2>/dev/null || echo missing))" >&2
fi
echo "[build-custom-image] chroot install OK"

# Write image metadata into the still-mounted guest (and later onto the host
# beside the published qcow2). Scheduler reads the host sidecar at VM create.
write_orion_image_info() {
    local dest="$1"
    local built_at kernel_ver
    built_at=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
    kernel_ver=""
    if [ -n "${KERNEL:-}" ]; then
        kernel_ver=$(basename "$KERNEL" | sed 's/^vmlinuz-//')
    fi
    cat > "$dest" <<INFO_EOF
{
  "image_name": "${IMAGE_NAME}",
  "built_at": "${built_at}",
  "rust": "${RUST_VERSION}",
  "buck2": "${BUCK2_LABEL}",
  "python": "${CPYTHON_VERSION}",
  "kernel": "${kernel_ver}"
}
INFO_EOF
    echo "[build-custom-image] Wrote image info: $dest"
    cat "$dest"
}

echo "[build-custom-image] Writing /etc/orion-image-info.json into guest..."
write_orion_image_info "$MOUNT_DIR/etc/orion-image-info.json"

# ============================================================================
# Stage 5: Unmount, disconnect, then compact the qcow2
# ============================================================================
log_stage "5-unmount-compact"
echo "[build-custom-image] Unmounting image tree..."
if ! unmount_image_tree "$MOUNT_DIR"; then
    echo "[build-custom-image] ERROR: failed to unmount $MOUNT_DIR" >&2
    exit 1
fi
MOUNT_DIR=""

log_cmd qemu-nbd --disconnect "$NBD_DEV"
release_nbd "$CUSTOM_IMAGE"
echo "[build-custom-image] NBD disconnected"

UNCOMPACT_SIZE=$(du -sh "$CUSTOM_IMAGE" | cut -f1)
echo "[build-custom-image] Compacting qcow2 (size before: $UNCOMPACT_SIZE)..."
NEED_BYTES=$(stat -c%s "$CUSTOM_IMAGE")
AVAIL_BYTES=$(df --output=avail -B1 "$(dirname "$CUSTOM_IMAGE")" | tail -1)
echo "[build-custom-image] qemu-img convert needs ~$((NEED_BYTES / 1024 / 1024))MiB, avail $((AVAIL_BYTES / 1024 / 1024))MiB on $(dirname "$CUSTOM_IMAGE")"
if [ "$AVAIL_BYTES" -lt "$NEED_BYTES" ]; then
    echo "[build-custom-image] ERROR: need ~$((NEED_BYTES / 1024 / 1024))MiB free for qemu-img convert, only $((AVAIL_BYTES / 1024 / 1024))MiB available on $(dirname "$CUSTOM_IMAGE")" >&2
    exit 1
fi
log_cmd qemu-img convert -O qcow2 -c "$CUSTOM_IMAGE" "${CUSTOM_IMAGE}.compact"
if ! qemu-img convert -O qcow2 -c "$CUSTOM_IMAGE" "${CUSTOM_IMAGE}.compact"; then
    echo "[build-custom-image] ERROR: qemu-img convert failed (exit $?)" >&2
    exit 1
fi
mv "${CUSTOM_IMAGE}.compact" "$CUSTOM_IMAGE"
echo "[build-custom-image] Final image size: $(du -sh "$CUSTOM_IMAGE" | cut -f1)"

# ============================================================================
# Stage 6: Copy kernel + initrd, write checksums
# ============================================================================
log_stage "6-kernel-checksums"
echo "[build-custom-image] Copying kernel and initrd..."
log_cmd cp "$KERNEL" "$IMAGE_DIR/"
cp "$KERNEL" "$IMAGE_DIR/"
log_cmd cp "$INITRD" "$IMAGE_DIR/"
cp "$INITRD" "$IMAGE_DIR/"

echo "[build-custom-image] Calculating checksums..."
(
    cd "$IMAGE_DIR"
    sha256sum "$IMAGE_NAME.qcow2" > checksums
)
echo "[build-custom-image] checksums: $(cat "$IMAGE_DIR/checksums")"

# ============================================================================
# Stage 7: Publish image to the path qlean expects (parent-dir flat layout)
# ============================================================================
log_stage "7-publish"
# qlean reads $OUTPUT_DIR/$IMAGE_NAME.json which carries `path` pointing at
# $OUTPUT_DIR/$IMAGE_NAME.qcow2 (flat). The subdir build artifacts above are
# kept for record but qlean won't read them directly.
PUBLISHED_IMAGE="$OUTPUT_DIR/$IMAGE_NAME.qcow2"
PUBLISHED_JSON="$OUTPUT_DIR/$IMAGE_NAME.json"

# Refuse to overwrite if a VM has the file locked (qemu holds a write lock).
if [ -f "$PUBLISHED_IMAGE" ]; then
    if qemu-img info "$PUBLISHED_IMAGE" >/dev/null 2>&1; then
        echo "[build-custom-image] published image exists and is readable: $PUBLISHED_IMAGE"
    else
        echo "[build-custom-image] WARNING: $PUBLISHED_IMAGE appears locked (VM running?)."
        echo "[build-custom-image] qemu-img info exit=$?; skipping publish."
        echo "[build-custom-image] Shut down any VMs using it and re-run, or manually:"
        echo "[build-custom-image]   cp $CUSTOM_IMAGE $PUBLISHED_IMAGE"
    fi
fi
if [ ! -f "$PUBLISHED_IMAGE" ] || qemu-img info "$PUBLISHED_IMAGE" >/dev/null 2>&1; then
    if [ -f "$PUBLISHED_IMAGE" ]; then
        echo "[build-custom-image] Publishing to $PUBLISHED_IMAGE (overwrite)..."
    else
        echo "[build-custom-image] Publishing to $PUBLISHED_IMAGE (new file)..."
    fi
    log_cmd cp "$CUSTOM_IMAGE" "$PUBLISHED_IMAGE"
    if ! cp "$CUSTOM_IMAGE" "$PUBLISHED_IMAGE"; then
        echo "[build-custom-image] ERROR: failed to copy image to $PUBLISHED_IMAGE (exit $?; disk full or file locked?)" >&2
        df -h "$(dirname "$PUBLISHED_IMAGE")" >&2
        echo "[build-custom-image] Build artifact remains at: $CUSTOM_IMAGE" >&2
        exit 1
    fi

    NEW_DIGEST=$(sha256sum "$PUBLISHED_IMAGE" | awk '{print $1}')
    echo "[build-custom-image] published digest: sha256:$NEW_DIGEST"

    if [ -f "$PUBLISHED_JSON" ]; then
        echo "[build-custom-image] Updating digest in $PUBLISHED_JSON..."
        TMP_JSON=$(mktemp)
        if ! jq --arg d "$NEW_DIGEST" \
           --arg p "$PUBLISHED_IMAGE" \
           '.path = $p | .digest = ["Sha256", $d]' \
           "$PUBLISHED_JSON" > "$TMP_JSON"; then
            echo "[build-custom-image] ERROR: jq failed updating $PUBLISHED_JSON (exit $?)" >&2
            rm -f "$TMP_JSON"
            exit 1
        fi
        mv "$TMP_JSON" "$PUBLISHED_JSON"
    else
        echo "[build-custom-image] Creating $PUBLISHED_JSON..."
        cat > "$PUBLISHED_JSON" <<JSON_EOF
{
  "name": "$IMAGE_NAME",
  "path": "$PUBLISHED_IMAGE",
  "arch": "Amd64",
  "distro": "Debian",
  "digest": ["Sha256", "$NEW_DIGEST"]
}
JSON_EOF
    fi
    echo "[build-custom-image] JSON updated: $(cat "$PUBLISHED_JSON")"
fi

# Host sidecar next to the subdir build and the flat published qcow2.
write_orion_image_info "$IMAGE_DIR/image-info.json"
write_orion_image_info "$OUTPUT_DIR/${IMAGE_NAME}.image-info.json"

if [ -z "${NEW_DIGEST:-}" ]; then
    NEW_DIGEST=$(sha256sum "$CUSTOM_IMAGE" | awk '{print $1}')
fi

fix_qlean_ownership

log_stage "done"
echo ""
echo "[build-custom-image] ==============================================="
echo "[build-custom-image] Custom image build complete!"
echo "[build-custom-image] ==============================================="
echo ""
echo "Build artifacts (subdir):"
echo "  Image:  $CUSTOM_IMAGE"
echo "  Kernel: $IMAGE_DIR/$(basename "$KERNEL")"
echo "  Initrd: $IMAGE_DIR/$(basename "$INITRD")"
echo ""
echo "Published to qlean (flat):"
echo "  Image:  $PUBLISHED_IMAGE"
echo "  JSON:   $PUBLISHED_JSON"
echo ""
echo "sha256:$NEW_DIGEST"
echo ""
cat "$IMAGE_DIR/checksums"
