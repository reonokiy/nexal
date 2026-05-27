# Self-hosted deployment

This deployment mode runs Nexal directly on your server with systemd:

- `nexal-server.service` runs the Bun LLM server.
- `nexal-gateway.service` runs the Rust gateway on the host.
- The gateway uses the host Podman backend with `runtime = "krun"` for sandboxes.
- Web assets are built in GitHub Actions, synced to `web/dist`, and served by Caddy.

The gateway intentionally runs on the host instead of inside a container so it
can call `/usr/bin/podman` and create sandbox containers with the krun runtime.
The GitHub Actions deploy path does not require sudo. A server administrator
must do the one-time bootstrap below.

## AlmaLinux 10 bootstrap

Run this once as root or an admin user:

```bash
dnf install -y caddy podman rsync
# Install Bun for the deploy user, or provide it through your preferred
# package manager. Bun is still needed to run the TypeScript server.
```

Install `krun` for AlmaLinux 10 and verify Podman can see it:

```bash
sudo -iu nexal podman info --format '{{ json .Host.OCIRuntime }}'
sudo -iu nexal podman run --rm --runtime=krun docker.io/library/alpine:latest uname -a
```

Create deploy-owned directories and enable user services for the deploy user:

```bash
useradd --create-home --shell /bin/bash nexal
mkdir -p /opt/nexal
chown -R nexal:nexal /opt/nexal
loginctl enable-linger nexal
```

Install Bun for the deploy user:

```bash
sudo -iu nexal bash -lc 'curl -fsSL https://bun.sh/install | bash'
```

Install Caddy config once. Edit the email first if you want Caddy to use a
specific ACME contact:

```bash
cp /opt/nexal/deploy/self-hosted/Caddyfile.example /etc/caddy/Caddyfile
vi /etc/caddy/Caddyfile
systemctl enable --now caddy
caddy validate --config /etc/caddy/Caddyfile
systemctl reload caddy
```

If SELinux is enforcing, allow Caddy to read the built web assets:

```bash
dnf install -y policycoreutils-python-utils
semanage fcontext -a -t httpd_sys_content_t '/opt/nexal/web-dist(/.*)?'
restorecon -Rv /opt/nexal/web-dist
setsebool -P httpd_can_network_connect 1
```

After this bootstrap, the `nexal` deploy user does not need sudo.

## GitHub variables and secrets

Set these repository variables:

- `SELF_HOSTED_HOST`: server hostname or IP.
- `SELF_HOSTED_USER`: SSH user.
- `SELF_HOSTED_PORT`: optional SSH port, defaults to `22`.
- `SELF_HOSTED_DEPLOY_DIR`: optional deploy path, defaults to `/opt/nexal`.
- `NEXAL_SANDBOX_IMAGE`: optional sandbox image, defaults to the repository GHCR image.

Set this repository secret:

- `SELF_HOSTED_SSH_KEY`: private SSH key for the deploy user.

The workflow assembles `server.env` and `gateway.toml` from the existing Nexal
repository secrets such as `DATABASE_URL`, `NEXAL_GATEWAY_ACCESS_KEY`,
`NEXAL_GATEWAY_SECRET_KEY`, `NEXAL_SUPABASE_URL`, `SUPABASE_JWT_SECRET`, and
`STORAGE_*`.

## First deploy

Run the `Deploy Self-hosted` GitHub Actions workflow manually. It will:

1. install dependencies and build Rust/web assets in GitHub Actions,
2. rsync the repository, root `node_modules`, web dist, and release binaries to the server,
3. write config files under `/opt/nexal/.config/nexal`,
4. install/update user systemd units under `~/.config/systemd/user`,
5. restart `nexal-gateway` and `nexal-server` as user services.

Caddy serves `/opt/nexal/web-dist` and reverse proxies `/api/*` and `/ws*` to
the Bun server on `127.0.0.1:3000`. The Bun server should bind to localhost
through `NEXAL_CHANNEL__WS__HOST=127.0.0.1` so port 3000 is not exposed
directly.
