/**
 * Transport layer — binary frame delivery over WebSocket and Unix socket.
 *
 * Both transports deliver raw `Uint8Array` frames. No encoding or
 * framing logic lives here — that belongs in the codec layer.
 *
 * WebSocket: each WS message is one frame.
 * Unix socket: 4-byte big-endian length prefix + payload per frame.
 */

export interface Transport {
	send(data: Uint8Array): void;
	close(): void;
}

export function createWebSocketTransport(
	url: string,
	options: { connectTimeoutMs?: number },
	onMessage: (data: Uint8Array) => void,
	onDisconnect: () => void,
): Promise<Transport> {
	const wsUrl = url.replace(/^http/, "ws");
	return new Promise<Transport>((resolve, reject) => {
		const timeout = setTimeout(() => {
			reject(new Error(`WebSocket connect timeout to ${wsUrl}`));
		}, options.connectTimeoutMs ?? 10_000);

		const ws = new WebSocket(wsUrl);
		ws.binaryType = "arraybuffer";

		const onOpen = () => {
			clearTimeout(timeout);
			ws.removeEventListener("error", onError);
			resolve({
				send: (data: Uint8Array) => {
					if (ws.readyState === 1) ws.send(data);
				},
				close: () => ws.close(),
			});
		};

		const onError = () => {
			clearTimeout(timeout);
			ws.removeEventListener("open", onOpen);
			reject(new Error(`WebSocket error connecting to ${wsUrl}`));
		};

		ws.addEventListener("open", onOpen, { once: true });
		ws.addEventListener("error", onError, { once: true });

		ws.addEventListener("message", (ev: MessageEvent) => {
			const bytes =
				ev.data instanceof ArrayBuffer
					? new Uint8Array(ev.data)
					: new TextEncoder().encode(ev.data);
			onMessage(bytes);
		});

		ws.addEventListener("close", () => onDisconnect());
	});
}
