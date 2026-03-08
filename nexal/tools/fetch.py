import ipaddress
import json
import socket
from dataclasses import dataclass, field
from typing import Any, ClassVar
from urllib.parse import urlparse

import httpx
import html_to_markdown

from nexal.tools.base import FunctionTool


def _check_url_scheme(url: str) -> str | None:
    """Return an error message if the URL scheme is not http/https, else None."""
    parsed = urlparse(url)
    if parsed.scheme not in ("http", "https"):
        return "Only http/https URLs are allowed"
    if not parsed.hostname:
        return "URL has no hostname"
    lower = parsed.hostname.lower()
    _BLOCKED_HOSTS = {
        "localhost", "0.0.0.0", "metadata.google.internal",
        "169.254.169.254", "[::1]", "100.100.100.200",
    }
    _BLOCKED_SUFFIXES = (".local", ".internal", ".localhost")
    if lower in _BLOCKED_HOSTS or any(lower.endswith(s) for s in _BLOCKED_SUFFIXES):
        return "Local/private network URLs are not allowed"
    return None


def _assert_public_address(hostname: str) -> None:
    """Resolve hostname and raise ValueError if any address is non-public."""
    try:
        addr = ipaddress.ip_address(hostname)
        if not addr.is_global:
            raise ValueError(f"Non-public IP address: {hostname}")
        return
    except ValueError as e:
        if "Non-public" in str(e):
            raise
        # Not an IP literal, resolve via DNS.

    try:
        addrinfos = socket.getaddrinfo(hostname, None)
    except socket.gaierror as e:
        raise ValueError(f"DNS resolution failed for {hostname}: {e}") from e

    if not addrinfos:
        raise ValueError(f"DNS resolution returned no addresses for {hostname}")

    for info in addrinfos:
        addr = ipaddress.ip_address(info[4][0])
        if not addr.is_global:
            raise ValueError(f"Non-public IP address {addr} resolved from {hostname}")


USER_AGENT = (
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) "
    "AppleWebKit/537.36 (KHTML, like Gecko) "
    "Chrome/133.0.0.0 Safari/537.36"
)

_BROWSER_HEADERS = {
    "User-Agent": USER_AGENT,
    "Accept": "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
    "Accept-Language": "en-US,en;q=0.9,zh-CN;q=0.8,zh;q=0.7",
}


@dataclass
class FetchParams:
    url: str


@dataclass
class WebFetchTool(FunctionTool):
    name: str = "web_fetch"
    description: str = (
        "Fetch a public web page and return its content as clean Markdown. "
        "Only external http/https URLs are allowed; local and private network addresses are blocked."
    )
    parameters: dict[str, Any] = field(
        default_factory=lambda: {
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "Public http/https URL to fetch. Local/private network URLs are not allowed.",
                },
            },
            "required": ["url"],
            "additionalProperties": False,
        },
        init=False,
    )
    params_type: ClassVar[type] = FetchParams

    MAX_CONTENT_LENGTH: ClassVar[int] = 128_000
    MAX_DOWNLOAD_BYTES: ClassVar[int] = 5_000_000  # 5 MB

    def execute(self, params: FetchParams) -> str:
        error = _check_url_scheme(params.url)
        if error:
            return json.dumps({"error": error})
        try:
            _assert_public_address(urlparse(params.url).hostname)  # type: ignore[arg-type]

            def _check_redirect(response: httpx.Response) -> None:
                if response.is_redirect:
                    location = response.headers.get("location", "")
                    parsed = urlparse(location)
                    if parsed.scheme and parsed.scheme not in ("http", "https"):
                        raise ValueError(f"Redirect to disallowed scheme: {parsed.scheme}")
                    if parsed.hostname:
                        _assert_public_address(parsed.hostname)

            with httpx.Client(
                headers=_BROWSER_HEADERS,
                follow_redirects=True,
                timeout=30,
                event_hooks={"response": [_check_redirect]},
            ) as client, client.stream("GET", params.url) as response:
                # Also verify the final resolved destination.
                final_hostname = urlparse(str(response.url)).hostname
                if final_hostname:
                    _assert_public_address(final_hostname)
                response.raise_for_status()
                encoding = response.charset_encoding or "utf-8"
                content_type = response.headers.get("content-type", "")
                # Read with size limit to avoid memory exhaustion.
                chunks: list[bytes] = []
                total = 0
                for chunk in response.iter_bytes(chunk_size=65536):
                    total += len(chunk)
                    if total > self.MAX_DOWNLOAD_BYTES:
                        break
                    chunks.append(chunk)
                raw_bytes = b"".join(chunks)
        except ValueError as e:
            return json.dumps({"error": str(e)})
        except httpx.HTTPError as e:
            return json.dumps({"error": str(e)})

        try:
            raw_text = raw_bytes.decode(encoding, errors="replace")
        except (LookupError, UnicodeDecodeError):
            raw_text = raw_bytes.decode("utf-8", errors="replace")
        if "html" in content_type or "xml" in content_type:
            text = html_to_markdown.convert(raw_text)
        elif content_type.startswith("text/") or "json" in content_type:
            text = raw_text
        else:
            return json.dumps({"error": f"Unsupported content type: {content_type}"})
        if len(text) > self.MAX_CONTENT_LENGTH:
            text = text[: self.MAX_CONTENT_LENGTH] + "\n\n[Content truncated]"
        return text
