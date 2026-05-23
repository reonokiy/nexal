/**
 * Transport layer — binary frame delivery over WebSocket.
 *
 * Each WS message is one frame. Includes optional application-level
 * heartbeat (ping/pong) to detect dead connections.
 */

import { encodeFrame, decodeFrame } from "./codec.ts";

export interface Transport {
	send(data: Uint8Array): void;
	close(): void;
	/** Start application-level heartbeat. Calls `onDead` if pong times out. */
	startHeartbeat(onDead?: () => void): void;
	/** Stop heartbeat. */
	stopHeartbeat(): void;
}

export interface TransportOptions {
	connectTimeoutMs?: number;
	heartbeat?: HeartbeatOptions;
}

export interface HeartbeatOptions {
	/** Interval in ms between ping messages. Default 30_000. */
	intervalMs?: number;
	/** Time in ms to wait for pong before considering dead. Default 10_000. */
	timeoutMs?: number;
}

export function createWebSocketTransport(
	url: string,
	options: TransportOptions,
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

		let heartbeatTimer: ReturnType<typeof setInterval> | null = null;
		let heartbeatTimeout: ReturnType<typeof setTimeout> | null = null;
		let onDead: (() => void) | null = null;
		const intervalMs = options.heartbeat?.intervalMs ?? 30_000;
		const timeoutMs = options.heartbeat?.timeoutMs ?? 10_000;

		function sendPing(): void {
			ws.send(encodeFrame({ method: "ping" }));
			if (heartbeatTimeout) clearTimeout(heartbeatTimeout);
			heartbeatTimeout = setTimeout(() => {
				onDead?.();
			}, timeoutMs);
		}

		function handlePong(): void {
			if (heartbeatTimeout) {
				clearTimeout(heartbeatTimeout);
				heartbeatTimeout = null;
			}
		}

		const onOpen = () => {
			clearTimeout(timeout);
			ws.removeEventListener("error", onError);
			resolve({
				send: (data: Uint8Array) => {
					if (ws.readyState === 1) ws.send(data);
				},
				close: () => {
					stopHeartbeat();
					ws.close();
				},
				startHeartbeat: (dead?: () => void) => {
					stopHeartbeat();
					onDead = dead ?? null;
					heartbeatTimer = setInterval(sendPing, intervalMs);
				},
				stopHeartbeat,
			});
		};

		function stopHeartbeat(): void {
			if (heartbeatTimer) {
				clearInterval(heartbeatTimer);
				heartbeatTimer = null;
			}
			if (heartbeatTimeout) {
				clearTimeout(heartbeatTimeout);
				heartbeatTimeout = null;
			}
		}

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

			// Intercept ping/pong at transport level.
			try {
				const msg = decodeFrame<Record<string, unknown>>(bytes);
				if (msg.method === "ping") {
					ws.send(encodeFrame({ method: "pong" }));
					return;
				}
				if (msg.method === "pong") {
					handlePong();
					return;
				}
			} catch {
				// Not decodable — pass through as raw frame.
			}

			onMessage(bytes);
		});

		ws.addEventListener("close", () => {
			stopHeartbeat();
			onDisconnect();
		});
	});
}
