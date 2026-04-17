/**
 * System clipboard image detection.
 *
 * Checks if the system clipboard contains an image and returns it as PNG bytes.
 * Supports:
 *   - WSL → PowerShell fallback to read Windows clipboard
 *   - X11 → xclip
 *   - Wayland → wl-paste
 *   - macOS → osascript + pngpaste / pbpaste
 */
import { existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

type ClipboardEnv = "wsl" | "x11" | "wayland" | "macos" | "unknown";

let _env: ClipboardEnv | null = null;

function detectEnv(): ClipboardEnv {
	if (_env) return _env;

	if (process.platform === "darwin") {
		_env = "macos";
	} else if (process.platform === "linux") {
		// Check WSL first.
		try {
			const ver = require("fs").readFileSync("/proc/version", "utf-8") as string;
			if (/microsoft|wsl/i.test(ver)) {
				_env = "wsl";
				return _env;
			}
		} catch { /* not WSL */ }

		if (process.env.WAYLAND_DISPLAY) {
			_env = "wayland";
		} else if (process.env.DISPLAY) {
			_env = "x11";
		} else {
			_env = "unknown";
		}
	} else {
		_env = "unknown";
	}
	return _env;
}

/**
 * Try to read an image from the system clipboard.
 * Returns PNG bytes if an image is found, null otherwise.
 */
export async function readClipboardImage(): Promise<Uint8Array | null> {
	const env = detectEnv();

	switch (env) {
		case "wsl":
			return readWsl();
		case "x11":
			return readX11();
		case "wayland":
			return readWayland();
		case "macos":
			return readMacos();
		default:
			return null;
	}
}

// ── WSL: use PowerShell to grab Windows clipboard image ────────────

async function readWsl(): Promise<Uint8Array | null> {
	const tmp = join(tmpdir(), `nexal-clip-${process.pid}.png`);
	// PowerShell script: save clipboard image to a temp file.
	const ps = `
		Add-Type -AssemblyName System.Windows.Forms
		$img = [System.Windows.Forms.Clipboard]::GetImage()
		if ($img -ne $null) {
			$img.Save('$(wslToWin(tmp))', [System.Drawing.Imaging.ImageFormat]::Png)
			Write-Output 'ok'
		} else {
			Write-Output 'no-image'
		}
	`.trim();

	try {
		const proc = Bun.spawnSync(["powershell.exe", "-NoProfile", "-Command", ps], {
			timeout: 5_000,
		});
		const out = proc.stdout.toString().trim();
		if (out.includes("ok") && existsSync(tmp)) {
			const data = await Bun.file(tmp).arrayBuffer();
			try { require("fs").unlinkSync(tmp); } catch { /* ok */ }
			return new Uint8Array(data);
		}
	} catch { /* powershell not available or failed */ }
	return null;
}

function wslToWin(linuxPath: string): string {
	// Convert /tmp/foo.png → \\wsl$\... or use wslpath
	try {
		const proc = Bun.spawnSync(["wslpath", "-w", linuxPath]);
		return proc.stdout.toString().trim();
	} catch {
		return linuxPath;
	}
}

// ── X11: xclip ─────────────────────────────────────────────────────

async function readX11(): Promise<Uint8Array | null> {
	try {
		const proc = Bun.spawnSync(
			["xclip", "-selection", "clipboard", "-t", "image/png", "-o"],
			{ timeout: 3_000 },
		);
		if (proc.exitCode === 0 && proc.stdout.length > 0) {
			return new Uint8Array(proc.stdout);
		}
	} catch { /* xclip not installed */ }
	return null;
}

// ── Wayland: wl-paste ──────────────────────────────────────────────

async function readWayland(): Promise<Uint8Array | null> {
	try {
		const proc = Bun.spawnSync(
			["wl-paste", "--type", "image/png"],
			{ timeout: 3_000 },
		);
		if (proc.exitCode === 0 && proc.stdout.length > 0) {
			return new Uint8Array(proc.stdout);
		}
	} catch { /* wl-paste not installed */ }
	return null;
}

// ── macOS: pngpaste or osascript ───────────────────────────────────

async function readMacos(): Promise<Uint8Array | null> {
	const tmp = join(tmpdir(), `nexal-clip-${process.pid}.png`);

	// Try pngpaste first (brew install pngpaste).
	try {
		const proc = Bun.spawnSync(["pngpaste", tmp], { timeout: 3_000 });
		if (proc.exitCode === 0 && existsSync(tmp)) {
			const data = await Bun.file(tmp).arrayBuffer();
			try { require("fs").unlinkSync(tmp); } catch { /* ok */ }
			return new Uint8Array(data);
		}
	} catch { /* pngpaste not installed */ }

	// Fallback: osascript.
	try {
		const script = `
			set theFile to POSIX file "${tmp}"
			try
				set theImage to the clipboard as «class PNGf»
				set fh to open for access theFile with write permission
				write theImage to fh
				close access fh
				return "ok"
			on error
				return "no-image"
			end try
		`;
		const proc = Bun.spawnSync(["osascript", "-e", script], { timeout: 3_000 });
		const out = proc.stdout.toString().trim();
		if (out === "ok" && existsSync(tmp)) {
			const data = await Bun.file(tmp).arrayBuffer();
			try { require("fs").unlinkSync(tmp); } catch { /* ok */ }
			return new Uint8Array(data);
		}
	} catch { /* osascript failed */ }

	return null;
}
