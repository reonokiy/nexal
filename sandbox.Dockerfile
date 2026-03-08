FROM docker.io/library/python:3.13-slim-trixie

RUN apt-get update && apt-get install -y --no-install-recommends \
    git \
    curl \
    wget \
    jq \
    unzip \
    ripgrep \
    && rm -rf /var/lib/apt/lists/*

ENV UV_VERSION=0.10.9 PIXI_VERSION=0.65.0
RUN curl -fsSL https://astral.sh/uv/$UV_VERSION/install.sh | sh \
    && mv /root/.local/bin/uv /usr/local/bin/uv \
    && mv /root/.local/bin/uvx /usr/local/bin/uvx \
    && curl -fsSL https://pixi.sh/install.sh | PIXI_VERSION=v$PIXI_VERSION PIXI_NO_PATH_UPDATE=1 bash \
    && mv /root/.pixi/bin/pixi /usr/local/bin/pixi

ENV HOME=/workspace
ENV PIXI_HOME=/workspace/.pixi
ENV UV_CACHE_DIR=/workspace/.cache/uv
WORKDIR /workspace
