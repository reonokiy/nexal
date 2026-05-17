#!/bin/sh
# Render ~/.nexal/gateway.toml from the environment, then exec the
# gateway. Frontend auth is HMAC (access/secret) — the gateway reads the
# single credential from NEXAL_GATEWAY_ACCESS_KEY / NEXAL_GATEWAY_SECRET_KEY
# (Fly secrets), so no token / [[credentials]] is written here.
set -e

HOME="${HOME:-/root}"
mkdir -p "$HOME/.nexal"

cat > "$HOME/.nexal/gateway.toml" <<EOF
listen = "[::]:${NEXAL_GATEWAY_PORT:-5500}"

[defaults]
image     = "${NEXAL_SANDBOX_IMAGE:-ghcr.io/reonokiy/nexal-sandbox:python3.13-debian13}"
agent_bin = "/usr/local/bin/nexal-agent"

[backend]
kind          = "fly"
fly_api_token = "${FLY_API_TOKEN}"
fly_app       = "${FLY_APP_NAME}"
fly_region    = "${FLY_REGION:-ams}"
EOF

exec /usr/local/bin/nexal-gateway --config "$HOME/.nexal/gateway.toml"
