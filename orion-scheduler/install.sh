#!/usr/bin/env bash
# Install orion-scheduler from an extracted release bundle.
#
# Layout expected in the same directory as this script:
#   bin/orion-scheduler
#   etc/target_config.json.template
#   etc/needrestart-orion-scheduler.conf
#   systemd/orion-scheduler.service
#
# Env vars (all optional):
#   PREFIX            install prefix (default /opt/orion-scheduler)
#   ETC_DIR           config dir (default /etc/orion-scheduler)
#   STATE_DIR         qlean state dir (default /var/lib/orion-scheduler)
#   LOG_DIR           default /var/log/orion-scheduler
#   CACHE_DIR         default /var/cache/orion-scheduler
#   SERVICE_USER      default orion
#   SERVICE_GROUP     default orion
#   SKIP_ENABLE       if "1", install files only, do not enable/start unit
#
# Run as root (or via sudo).

set -euo pipefail

PREFIX="${PREFIX:-/opt/orion-scheduler}"
ETC_DIR="${ETC_DIR:-/etc/orion-scheduler}"
STATE_DIR="${STATE_DIR:-/var/lib/orion-scheduler}"
LOG_DIR="${LOG_DIR:-/var/log/orion-scheduler}"
CACHE_DIR="${CACHE_DIR:-/var/cache/orion-scheduler}"
SERVICE_USER="${SERVICE_USER:-orion}"
SERVICE_GROUP="${SERVICE_GROUP:-orion}"
SKIP_ENABLE="${SKIP_ENABLE:-0}"

SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)

if [[ $EUID -ne 0 ]]; then
  echo "[install] must run as root (use sudo)" >&2
  exit 1
fi

for f in "$SCRIPT_DIR/bin/orion-scheduler" \
         "$SCRIPT_DIR/etc/target_config.json.template" \
         "$SCRIPT_DIR/etc/needrestart-orion-scheduler.conf" \
         "$SCRIPT_DIR/systemd/orion-scheduler.service"; do
  if [[ ! -f "$f" ]]; then
    echo "[install] missing bundle file: $f" >&2
    exit 1
  fi
done

echo "[install] ensuring group $SERVICE_GROUP, user $SERVICE_USER (member of kvm)"
if ! getent group "$SERVICE_GROUP" >/dev/null; then
  groupadd --system "$SERVICE_GROUP"
fi
if ! id -u "$SERVICE_USER" >/dev/null 2>&1; then
  # qlean needs $HOME for ~/.local/share/qlean; we point HOME at $STATE_DIR.
  useradd --system \
          --gid "$SERVICE_GROUP" \
          --home-dir "$STATE_DIR" \
          --shell /usr/sbin/nologin \
          "$SERVICE_USER"
fi
if getent group kvm >/dev/null; then
  usermod -aG kvm "$SERVICE_USER"
else
  echo "[install] WARN: 'kvm' group not present on this host; install qemu-kvm first" >&2
fi

echo "[install] creating directories"
install -d -o "$SERVICE_USER" -g "$SERVICE_GROUP" -m 0755 \
  "$PREFIX/bin" "$STATE_DIR" "$LOG_DIR" "$CACHE_DIR"
install -d -o "$SERVICE_USER" -g "$SERVICE_GROUP" -m 0750 "$ETC_DIR"

# qlean's QleanDirs hardcodes ~/.local/share/qlean; symlink that into
# $STATE_DIR so VM images/runs persist outside the user's home.
install -d -o "$SERVICE_USER" -g "$SERVICE_GROUP" -m 0755 \
  "$STATE_DIR/.local" "$STATE_DIR/.local/share"
if [[ ! -e "$STATE_DIR/.local/share/qlean" ]]; then
  ln -s "$STATE_DIR/qlean" "$STATE_DIR/.local/share/qlean"
fi
install -d -o "$SERVICE_USER" -g "$SERVICE_GROUP" -m 0755 \
  "$STATE_DIR/qlean" "$STATE_DIR/qlean/images" "$STATE_DIR/qlean/runs"

echo "[install] installing binary to $PREFIX/bin/orion-scheduler"
install -o root -g root -m 0755 \
  "$SCRIPT_DIR/bin/orion-scheduler" "$PREFIX/bin/orion-scheduler"

echo "[install] installing config to $ETC_DIR (only if missing)"
if [[ ! -f "$ETC_DIR/target_config.json" ]]; then
  install -o "$SERVICE_USER" -g "$SERVICE_GROUP" -m 0640 \
    "$SCRIPT_DIR/etc/target_config.json.template" \
    "$ETC_DIR/target_config.json"
  echo "[install]   wrote template — edit $ETC_DIR/target_config.json before starting"
else
  echo "[install]   $ETC_DIR/target_config.json already exists, keeping"
fi

echo "[install] installing systemd unit"
install -o root -g root -m 0644 \
  "$SCRIPT_DIR/systemd/orion-scheduler.service" \
  /etc/systemd/system/orion-scheduler.service
systemctl daemon-reload

# Prevent needrestart (unattended-upgrade) from restarting this unit and
# thereby SIGTERM-killing every tracked QEMU VM. Non-fatal if needrestart
# is not installed on the host.
if [[ -d /etc/needrestart/conf.d ]]; then
  echo "[install] installing needrestart override"
  install -o root -g root -m 0644 \
    "$SCRIPT_DIR/etc/needrestart-orion-scheduler.conf" \
    /etc/needrestart/conf.d/orion-scheduler.conf
else
  echo "[install] needrestart conf.d absent; skipping override (install needrestart to protect VMs from apt upgrades)"
fi

if [[ "$SKIP_ENABLE" == "1" ]]; then
  echo "[install] SKIP_ENABLE=1, leaving unit disabled"
  echo "[install] Done."
  exit 0
fi

if [[ -f "$ETC_DIR/target_config.json" ]] \
   && grep -q '"/path/to/' "$ETC_DIR/target_config.json"; then
  echo "[install] config still contains template placeholders;"
  echo "[install]   enabling unit but NOT starting it. Edit $ETC_DIR/target_config.json"
  echo "[install]   then run: sudo systemctl start orion-scheduler"
  systemctl enable orion-scheduler.service
else
  systemctl enable --now orion-scheduler.service
  systemctl --no-pager --lines=0 status orion-scheduler.service || true
fi

echo "[install] Done."
