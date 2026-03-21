FROM docker.io/library/python:3.13-slim-trixie

# Install podman (for sandbox containers) and runtime deps.
RUN apt-get update && apt-get install -y --no-install-recommends \
    podman \
    fuse-overlayfs \
    slirp4netns \
    uidmap \
    && rm -rf /var/lib/apt/lists/*

# Configure rootless podman storage (fuse-overlayfs for nested containers).
RUN mkdir -p /etc/containers && \
    printf '[storage]\ndriver = "overlay"\nrunroot = "/run/containers/storage"\ngraphroot = "/var/lib/containers/storage"\n\n[storage.options.overlay]\nmount_program = "/usr/bin/fuse-overlayfs"\n' \
    > /etc/containers/storage.conf

# Ensure runtime dirs exist and podman can find its runroot.
ENV XDG_RUNTIME_DIR=/run/user/0
RUN mkdir -p /run/user/0 /run/containers/storage /var/lib/containers/storage

WORKDIR /app
COPY pyproject.toml uv.lock README.md ./
COPY nexal/ nexal/
COPY sandbox.Dockerfile ./

# Install nexal with all optional deps.
RUN pip install --no-cache-dir '.[bots]'

# Pre-pull the sandbox image so first exec doesn't wait.
RUN podman pull ghcr.io/reonokiy/nexal-sandbox:python3.13-debian13 || true

ENTRYPOINT ["nexal"]
