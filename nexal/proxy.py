"""Host-side token proxy — one Unix socket per upstream API.

Each service gets its own socket under /workspace/agents/proxy/:
  - proxy/api.telegram.org  — Telegram Bot API
  - proxy/discord.com       — Discord API

Skill scripts connect to the socket matching the API they need.
The proxy injects the real token and forwards the request.
Tokens never enter the container.
"""

from __future__ import annotations

import json
import logging
import threading
from http.server import BaseHTTPRequestHandler
from pathlib import Path
from socketserver import UnixStreamServer
from typing import Any

import httpx

logger = logging.getLogger("nexal.proxy")

_PROXY_DIR = "proxy"


# ---------------------------------------------------------------------------
# Server
# ---------------------------------------------------------------------------

class _UnixHTTPServer(UnixStreamServer):
    """HTTPServer that listens on a Unix socket."""

    allow_reuse_address = True

    def get_request(self):
        request, client_address = super().get_request()
        return request, ("", 0)


def _start_server(sock_path: Path, handler_cls: type) -> _UnixHTTPServer:
    sock_path.parent.mkdir(parents=True, exist_ok=True)
    sock_path.unlink(missing_ok=True)
    server = _UnixHTTPServer(str(sock_path), handler_cls)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    logger.info("proxy_started socket=%s", sock_path)
    return server


# ---------------------------------------------------------------------------
# Telegram proxy
# ---------------------------------------------------------------------------

_TELEGRAM_UPSTREAM = "https://api.telegram.org"


def _make_telegram_handler(token: str) -> type[BaseHTTPRequestHandler]:

    class TelegramHandler(BaseHTTPRequestHandler):
        def log_message(self, fmt: str, *args: Any) -> None:
            logger.debug(fmt, *args)

        def do_POST(self) -> None:
            content_length = int(self.headers.get("Content-Length", 0))
            body = self.rfile.read(content_length) if content_length else b""
            try:
                data = json.loads(body) if body else {}
            except json.JSONDecodeError:
                return self._respond(400, {"error": "Invalid JSON"})

            # Map path to upstream: POST /sendMessage → api.telegram.org/bot<token>/sendMessage
            method = self.path.strip("/")
            if not method:
                return self._respond(400, {"error": "Missing API method in path"})
            url = f"{_TELEGRAM_UPSTREAM}/bot{token}/{method}"

            try:
                resp = httpx.post(url, json=data, timeout=30)
                self._respond(resp.status_code, resp.json())
            except Exception as e:
                self._respond(502, {"error": str(e)})

        def do_GET(self) -> None:
            method = self.path.strip("/")
            if not method:
                return self._respond(200, {"status": "ok"})
            url = f"{_TELEGRAM_UPSTREAM}/bot{token}/{method}"
            try:
                resp = httpx.get(url, timeout=30)
                self._respond(resp.status_code, resp.json())
            except Exception as e:
                self._respond(502, {"error": str(e)})

        def _respond(self, status: int, data: dict) -> None:
            body = json.dumps(data, ensure_ascii=False).encode()
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

    return TelegramHandler


# ---------------------------------------------------------------------------
# Discord proxy
# ---------------------------------------------------------------------------

_DISCORD_UPSTREAM = "https://discord.com/api/v10"


def _make_discord_handler(token: str) -> type[BaseHTTPRequestHandler]:

    class DiscordHandler(BaseHTTPRequestHandler):
        def log_message(self, fmt: str, *args: Any) -> None:
            logger.debug(fmt, *args)

        def do_POST(self) -> None:
            content_length = int(self.headers.get("Content-Length", 0))
            body = self.rfile.read(content_length) if content_length else b""
            try:
                data = json.loads(body) if body else {}
            except json.JSONDecodeError:
                return self._respond(400, {"error": "Invalid JSON"})

            path = self.path.rstrip("/")
            if not path:
                return self._respond(400, {"error": "Missing API path"})
            url = f"{_DISCORD_UPSTREAM}{path}"
            headers = {"Authorization": f"Bot {token}"}

            try:
                resp = httpx.post(url, json=data, headers=headers, timeout=30)
                self._respond(resp.status_code, resp.json())
            except Exception as e:
                self._respond(502, {"error": str(e)})

        def do_GET(self) -> None:
            path = self.path.rstrip("/")
            if not path:
                return self._respond(200, {"status": "ok"})
            url = f"{_DISCORD_UPSTREAM}{path}"
            headers = {"Authorization": f"Bot {token}"}
            try:
                resp = httpx.get(url, headers=headers, timeout=30)
                self._respond(resp.status_code, resp.json())
            except Exception as e:
                self._respond(502, {"error": str(e)})

        def _respond(self, status: int, data: dict) -> None:
            body = json.dumps(data, ensure_ascii=False).encode()
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

    return DiscordHandler


# ---------------------------------------------------------------------------
# Public API
# ---------------------------------------------------------------------------

def _proxy_dir(workspace_dir: str) -> Path:
    return Path(workspace_dir).joinpath("agents", _PROXY_DIR)


def start_proxies(
    workspace_dir: str,
    telegram_token: str | None = None,
    discord_token: str | None = None,
) -> list[_UnixHTTPServer]:
    """Start per-service proxy servers. Returns list of servers for shutdown."""
    base = _proxy_dir(workspace_dir)
    servers: list[_UnixHTTPServer] = []

    if telegram_token:
        handler = _make_telegram_handler(telegram_token)
        servers.append(_start_server(base / "api.telegram.org", handler))

    if discord_token:
        handler = _make_discord_handler(discord_token)
        servers.append(_start_server(base / "discord.com", handler))

    return servers
