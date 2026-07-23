#!/bin/bash

# ==============================================================================
#  Orion Runner 清理脚本（systemd ExecStartPre）
#
#  说明：
#  - 在服务启动前执行，清理旧进程和 FUSE 挂载
#  - 清理 Antares 孤儿 upper/cl（无活跃挂载时）
#  - 轮转过大的 orion.log
#  - 即使失败也不会阻止服务启动（始终 exit 0）
# ==============================================================================

set +e  # 允许命令失败，不中断脚本

SCORPIO_TOML="${SCORPIO_CONFIG:-/home/orion/orion-runner/scorpio.toml}"
ORION_LOG="${ORION_LOG_PATH:-/home/orion/orion-runner/log/orion.log}"
ORION_LOG_MAX_BYTES="${ORION_LOG_MAX_BYTES:-209715200}" # 200 MiB
DISK_WARN_PCT="${ORION_DISK_WARN_PCT:-90}"
DISK_CRIT_PCT="${ORION_DISK_CRIT_PCT:-95}"

if [ ! -f "./config.toml" ]; then
    echo "==> [清理] 未发现 ./config.toml，正在创建..."
    printf 'works = []\n' > "./config.toml"
fi

# 从 scorpio.toml 读取路径
read_scorpio_path() {
    local key="$1"
    local file="$2"
    if [ -f "$file" ]; then
        awk -F'"' -v k="$key" '$1 ~ "^"k"[[:space:]]*=" {print $2; exit}' "$file"
    fi
}

disk_used_pct() {
    df -P / 2>/dev/null | awk 'NR==2 { gsub(/%/,"",$5); print $5 }'
}

echo "==> [清理] 停止旧进程..."
if command -v buck2 &>/dev/null; then
    echo "  - 正在执行 'buck2 killall'..."
    buck2 killall 2>&1 || echo "  - buck2 killall 完成"
fi

# Only kill the orion binary, not this cleanup script
# Use full path match to avoid killing ourselves
if pgrep -f "/orion-runner/orion" >/dev/null 2>&1; then
    echo "  - 正在终止旧的 orion 进程..."
    pkill -9 -f "/orion-runner/orion" 2>&1 || echo "  - 进程清理完成"
else
    echo "  - 没有找到运行中的 orion 进程"
fi

echo "==> [清理] 卸载 FUSE 挂载点..."
MOUNT_DIR="$(read_scorpio_path "workspace" "${SCORPIO_TOML}")"
MOUNT_DIR="${MOUNT_DIR:-/workspace/mount}"
UPPER_ROOT="$(read_scorpio_path "antares_upper_root" "${SCORPIO_TOML}")"
UPPER_ROOT="${UPPER_ROOT:-/data/scorpio/antares/upper}"
CL_ROOT="$(read_scorpio_path "antares_cl_root" "${SCORPIO_TOML}")"
CL_ROOT="${CL_ROOT:-/data/scorpio/antares/cl}"
MNT_ROOT="$(read_scorpio_path "antares_mount_root" "${SCORPIO_TOML}")"
MNT_ROOT="${MNT_ROOT:-/data/scorpio/antares/mnt}"

echo "  - 正在卸载 ${MOUNT_DIR}..."
fusermount -uz "${MOUNT_DIR}" 2>/dev/null || true
umount -lf "${MOUNT_DIR}" 2>/dev/null || true

# 清理并重建挂载目录
if ! mountpoint -q "${MOUNT_DIR}" 2>/dev/null; then
    rm -rf "${MOUNT_DIR}" 2>/dev/null || true
    mkdir -p "${MOUNT_DIR}" 2>/dev/null || true
    echo "  - 挂载点已清理"
else
    echo "  - ${MOUNT_DIR} 仍是挂载点，跳过删除"
fi

echo "==> [清理] Antares mount_root 残留..."
if [ -d "${MNT_ROOT}" ]; then
    for mp in "${MNT_ROOT}"/*; do
        [ -e "$mp" ] || continue
        if mountpoint -q "$mp" 2>/dev/null; then
            echo "  - fusermount -uz $mp"
            fusermount -uz "$mp" 2>/dev/null || true
            umount -lf "$mp" 2>/dev/null || true
        fi
        if ! mountpoint -q "$mp" 2>/dev/null; then
            rm -rf "$mp" 2>/dev/null || true
        fi
    done
fi

prune_overlay_root() {
    local root="$1"
    local kind="$2"
    local removed=0
    if [ ! -d "$root" ]; then
        return 0
    fi
    # Only prune when no Antares mounts remain active.
    local active=0
    if [ -d "${MNT_ROOT}" ]; then
        for mp in "${MNT_ROOT}"/*; do
            [ -e "$mp" ] || continue
            if mountpoint -q "$mp" 2>/dev/null; then
                active=1
                break
            fi
        done
    fi
    if [ "$active" -eq 1 ]; then
        echo "  - 跳过 ${kind} 清理：仍有活跃 Antares 挂载"
        return 0
    fi
    for d in "${root}"/*; do
        [ -d "$d" ] || continue
        rm -rf "$d" 2>/dev/null && removed=$((removed + 1))
    done
    echo "  - 已删除 ${removed} 个孤儿 ${kind} 目录 (${root})"
}

echo "==> [清理] Antares 孤儿 overlay..."
prune_overlay_root "${UPPER_ROOT}" "upper"
prune_overlay_root "${CL_ROOT}" "cl"

echo "==> [清理] orion.log 轮转..."
if [ -f "${ORION_LOG}" ]; then
    size=$(wc -c < "${ORION_LOG}" 2>/dev/null || echo 0)
    if [ "${size}" -gt "${ORION_LOG_MAX_BYTES}" ]; then
        echo "  - ${ORION_LOG} 为 ${size} bytes (> ${ORION_LOG_MAX_BYTES}), rotating"
        mv -f "${ORION_LOG}" "${ORION_LOG}.1" 2>/dev/null || true
        : > "${ORION_LOG}" 2>/dev/null || true
        # Keep ownership usable for the runner user when present.
        chown orion:orion "${ORION_LOG}" 2>/dev/null || true
    else
        echo "  - ${ORION_LOG} size=${size} bytes (ok)"
    fi
else
    echo "  - 无 orion.log"
fi

USED_PCT="$(disk_used_pct)"
USED_PCT="${USED_PCT:-0}"
echo "==> [清理] 磁盘用量: ${USED_PCT}%"

if [ "${USED_PCT}" -ge "${DISK_WARN_PCT}" ] 2>/dev/null; then
    echo "  - 用量 ≥ ${DISK_WARN_PCT}%，再次 prune orphan overlays"
    prune_overlay_root "${UPPER_ROOT}" "upper"
    prune_overlay_root "${CL_ROOT}" "cl"
fi

USED_PCT="$(disk_used_pct)"
USED_PCT="${USED_PCT:-0}"
if [ "${USED_PCT}" -ge "${DISK_CRIT_PCT}" ] 2>/dev/null; then
    echo "ERROR: disk usage still ${USED_PCT}% (≥ ${DISK_CRIT_PCT}%). Orion will refuse new builds until space is freed."
fi

echo "==> [清理] 完成"
exit 0  # 总是返回成功
