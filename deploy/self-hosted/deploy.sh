#!/usr/bin/env bash
set -euo pipefail

APP_DIR="${SELF_HOSTED_DEPLOY_DIR:-/opt/nexal}"
WEB_ROOT="${SELF_HOSTED_WEB_ROOT:-$APP_DIR/web-dist}"
CONFIG_DIR="${SELF_HOSTED_CONFIG_DIR:-$APP_DIR/.config/nexal}"
DATA_DIR="${SELF_HOSTED_DATA_DIR:-$APP_DIR/.local/share/nexal}"
SYSTEMD_USER_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
DBUS_SESSION_BUS_ADDRESS="${DBUS_SESSION_BUS_ADDRESS:-unix:path=$XDG_RUNTIME_DIR/bus}"
export XDG_RUNTIME_DIR
export DBUS_SESSION_BUS_ADDRESS
export PATH="$HOME/.bun/bin:$HOME/.cargo/bin:$PATH"

cd "$APP_DIR"

mkdir -p "$CONFIG_DIR" "$WEB_ROOT" "$DATA_DIR/workspace" "$DATA_DIR/skills" "$SYSTEMD_USER_DIR"

if [[ -n "${NEXAL_PROD_ENV_FILE:-}" && -f "$NEXAL_PROD_ENV_FILE" ]]; then
  install -m 600 "$NEXAL_PROD_ENV_FILE" "$CONFIG_DIR/server.env"
elif [[ -n "${NEXAL_PROD_ENV:-}" ]]; then
  printf '%s\n' "$NEXAL_PROD_ENV" > "$CONFIG_DIR/server.env"
  chmod 600 "$CONFIG_DIR/server.env"
fi

if [[ -n "${NEXAL_GATEWAY_CONFIG_FILE:-}" && -f "$NEXAL_GATEWAY_CONFIG_FILE" ]]; then
  install -m 600 "$NEXAL_GATEWAY_CONFIG_FILE" "$CONFIG_DIR/gateway.toml"
elif [[ -n "${NEXAL_GATEWAY_CONFIG:-}" ]]; then
  printf '%s\n' "$NEXAL_GATEWAY_CONFIG" > "$CONFIG_DIR/gateway.toml"
  chmod 600 "$CONFIG_DIR/gateway.toml"
fi

sed \
  -e "s#__APP_DIR__#$APP_DIR#g" \
  -e "s#__CONFIG_DIR__#$CONFIG_DIR#g" \
  deploy/self-hosted/nexal-gateway.service > "$SYSTEMD_USER_DIR/nexal-gateway.service"
sed \
  -e "s#__APP_DIR__#$APP_DIR#g" \
  -e "s#__CONFIG_DIR__#$CONFIG_DIR#g" \
  deploy/self-hosted/nexal-server.service > "$SYSTEMD_USER_DIR/nexal-server.service"
systemctl --user daemon-reload

test -x "$APP_DIR/target/release/nexal-gateway"
test -x "$APP_DIR/target/release/nexal-agent"
test -d "$APP_DIR/web/dist"
rsync -a --delete web/dist/ "$WEB_ROOT"/

systemctl --user enable nexal-gateway nexal-server
systemctl --user restart nexal-gateway
systemctl --user restart nexal-server

systemctl --user --no-pager --full status nexal-gateway nexal-server
