import type { Transport } from "./errors.ts";

export interface TransportHandle {
	transport: Transport;
	cleanup: () => void;
}

export function createWebSocketTransport(
	url: string,
	options: { connectTimeoutMs?: number },
	decoder: TextDecoder,
	onLine: (line: string) => void,
	onDisconnect: () => void,
): Promise<TransportHandle> {
	const wsUrl = url.replace(/^http/, "ws");
	return new Promise<TransportHandle>((resolve, reject) => {
		const timeout = setTimeout(() => {
			reject(new Error(`gateway WebSocket connect timeout to ${wsUrl}`));
		}, options.connectTimeoutMs ?? 10_000);

		const ws = new WebSocket(wsUrl);
		ws.binaryType = "arraybuffer";

		ws.onopen = () => {
			clearTimeout(timeout);
			resolve({
				transport: {
					send: (data: string) => {
						if (ws.readyState === 1) ws.send(data);
					},
					close: () => ws.close(),
				},
				cleanup: () => ws.close(),
			});
		};

		ws.onerror = () => {
			clearTimeout(timeout);
			reject(new Error(`gateway WebSocket error connecting to ${wsUrl}`));
		};

		ws.onmessage = (ev: MessageEvent) => {
			const text =
				typeof ev.data === "string"
					? ev.data
					: decoder.decode(ev.data as ArrayBuffer);
			for (const line of text.split("\n")) {
				if (line.trim()) onLine(line);
			}
		};

		ws.onclose = () => onDisconnect();
	});
}

export async function createUnixTransport(
	path: string,
	onLine: (line: string) => void,
	onDisconnect: () => void,
): Promise<TransportHandle> {
	const { createConnection } = await import("node:net");
	return new Promise<TransportHandle>((resolve, reject) => {
		const sock = createConnection(path, () => {
			resolve({
				transport: {
					send: (data: string) => sock.write(data + "\n"),
					close: () => sock.destroy(),
				},
				cleanup: () => sock.destroy(),
			});
		});
		sock.on("error", (err: Error) => {
			reject(new Error(`gateway unix connect error: ${err.message}`));
		});

		let buffer = "";
		sock.on("data", (chunk: Buffer) => {
			buffer += chunk.toString("utf-8");
			const lines = buffer.split("\n");
			buffer = lines.pop()!;
			for (const line of lines) {
				if (line.trim()) onLine(line);
			}
		});
		sock.on("close", () => onDisconnect());
	});
}
