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

export interface AcceptedWebSocketTransport extends Transport {
	/** Feed an inbound server WebSocket message into this transport. */
	receive(data: ArrayBuffer | Uint8Array | string): void;
	/** Notify this transport that the accepted WebSocket closed. */
	disconnect(): void;
}

export interface WebSocketPeer {
	readonly readyState?: number;
	send(data: Uint8Array): unknown;
	close(): unknown;
}

export interface TransportOptions {
	connectTimeoutMs?: number;
	heartbeat?: HeartbeatOptions;
	onHeartbeatDead?: () => void;
	reconnect?: ReconnectOptions;
	onReconnectAttempt?: (attempt: number, delayMs: number) => void;
	onReconnect?: () => void;
	onReconnectFailed?: (error: Error) => void;
}

export interface HeartbeatOptions {
	/** Interval in ms between ping messages. Default 30_000. */
	intervalMs?: number;
	/** Time in ms to wait for pong before considering dead. Default 10_000. */
	timeoutMs?: number;
}

export interface ReconnectOptions {
	/** Max reconnect attempts. Default: Infinity. */
	attempts?: number;
	/** Initial backoff delay in ms. Default: 500. */
	minDelayMs?: number;
	/** Max backoff delay in ms. Default: 10_000. */
	maxDelayMs?: number;
	/** Backoff multiplier. Default: 2. */
	factor?: number;
	/** Random jitter ratio, 0..1. Default: 0.2. */
	jitter?: number;
}

const WS_OPEN = 1;

function toBytes(data: ArrayBuffer | Uint8Array | string): Uint8Array {
	if (data instanceof Uint8Array) return data;
	if (data instanceof ArrayBuffer) return new Uint8Array(data);
	return new TextEncoder().encode(data);
}

function handleHeartbeatFrame(
	peer: WebSocketPeer,
	bytes: Uint8Array,
	handlePong: () => void,
): boolean {
	try {
		const msg = decodeFrame<Record<string, unknown>>(bytes);
		if (msg.method === "ping") {
			peer.send(encodeFrame({ method: "pong" }));
			return true;
		}
		if (msg.method === "pong") {
			handlePong();
			return true;
		}
	} catch {
		// Not decodable — pass through as raw frame.
	}
	return false;
}

export function createAcceptedWebSocketTransport(
	peer: WebSocketPeer,
	options: TransportOptions,
	onMessage: (data: Uint8Array) => void,
	onDisconnect: () => void = () => {},
): AcceptedWebSocketTransport {
	let closed = false;
	let heartbeatTimer: ReturnType<typeof setInterval> | null = null;
	let heartbeatTimeout: ReturnType<typeof setTimeout> | null = null;
	let heartbeatEnabled = options.heartbeat !== undefined;
	let onDead: (() => void) | null = options.onHeartbeatDead ?? null;
	const intervalMs = options.heartbeat?.intervalMs ?? 30_000;
	const timeoutMs = options.heartbeat?.timeoutMs ?? 10_000;

	function sendPing(): void {
		if (peer.readyState !== undefined && peer.readyState !== WS_OPEN) return;
		peer.send(encodeFrame({ method: "ping" }));
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

	function startHeartbeatTimer(): void {
		stopHeartbeat();
		if (!heartbeatEnabled) return;
		heartbeatTimer = setInterval(sendPing, intervalMs);
	}

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

	const transport: AcceptedWebSocketTransport = {
		send: (data: Uint8Array) => {
			if (peer.readyState === undefined || peer.readyState === WS_OPEN) peer.send(data);
		},
		close: () => {
			closed = true;
			stopHeartbeat();
			peer.close();
		},
		startHeartbeat: (dead?: () => void) => {
			heartbeatEnabled = true;
			onDead = dead ?? null;
			startHeartbeatTimer();
		},
		stopHeartbeat: () => {
			heartbeatEnabled = false;
			stopHeartbeat();
		},
		receive: (data: ArrayBuffer | Uint8Array | string) => {
			const bytes = toBytes(data);
			if (handleHeartbeatFrame(peer, bytes, handlePong)) return;
			onMessage(bytes);
		},
		disconnect: () => {
			stopHeartbeat();
			if (!closed) onDisconnect();
		},
	};

	startHeartbeatTimer();
	return transport;
}

export function createWebSocketTransport(
	url: string,
	options: TransportOptions,
	onMessage: (data: Uint8Array) => void,
	onDisconnect: () => void,
): Promise<Transport> {
	const wsUrl = url.replace(/^http/, "ws");
	return new Promise<Transport>((resolve, reject) => {
		let ws: WebSocket | null = null;
		let closed = false;
		let settled = false;
		let wasConnected = false;
		let reconnectAttempts = 0;
		let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
		let heartbeatTimer: ReturnType<typeof setInterval> | null = null;
		let heartbeatTimeout: ReturnType<typeof setTimeout> | null = null;
		let heartbeatEnabled = options.heartbeat !== undefined;
		let onDead: (() => void) | null = options.onHeartbeatDead ?? null;
		const intervalMs = options.heartbeat?.intervalMs ?? 30_000;
		const timeoutMs = options.heartbeat?.timeoutMs ?? 10_000;
		const reconnect = options.reconnect;
		const transport: Transport = {
			send: (data: Uint8Array) => {
				if (ws?.readyState === WebSocket.OPEN) ws.send(data);
			},
			close: () => {
				closed = true;
				if (reconnectTimer) clearTimeout(reconnectTimer);
				stopHeartbeat();
				ws?.close();
			},
			startHeartbeat: (dead?: () => void) => {
				heartbeatEnabled = true;
				onDead = dead ?? null;
				startHeartbeatTimer();
			},
			stopHeartbeat: () => {
				heartbeatEnabled = false;
				stopHeartbeat();
			},
		};

		function sendPing(): void {
			if (ws?.readyState !== WebSocket.OPEN) return;
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

		function startHeartbeatTimer(): void {
			stopHeartbeat();
			if (!heartbeatEnabled || ws?.readyState !== WebSocket.OPEN) return;
			heartbeatTimer = setInterval(sendPing, intervalMs);
		}

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

		function reconnectDelay(attempt: number): number {
			const min = reconnect?.minDelayMs ?? 500;
			const max = reconnect?.maxDelayMs ?? 10_000;
			const factor = reconnect?.factor ?? 2;
			const jitter = reconnect?.jitter ?? 0.2;
			const base = Math.min(max, min * factor ** Math.max(0, attempt - 1));
			return Math.round(base + base * jitter * Math.random());
		}

		function failReconnect(error: Error): void {
			options.onReconnectFailed?.(error);
			if (!settled) reject(error);
		}

		function scheduleReconnect(error: Error): void {
			if (closed || !reconnect) {
				failReconnect(error);
				return;
			}

			const maxAttempts = reconnect.attempts ?? Number.POSITIVE_INFINITY;
			if (reconnectAttempts >= maxAttempts) {
				failReconnect(error);
				return;
			}

			reconnectAttempts += 1;
			const delay = reconnectDelay(reconnectAttempts);
			options.onReconnectAttempt?.(reconnectAttempts, delay);
			reconnectTimer = setTimeout(connect, delay);
		}

		function connect(): void {
			const socket = new WebSocket(wsUrl);
			ws = socket;
			socket.binaryType = "arraybuffer";
			let opened = false;
			let failedBeforeOpen = false;

			const connectTimeout = setTimeout(() => {
				failedBeforeOpen = true;
				scheduleReconnect(new Error(`WebSocket connect timeout to ${wsUrl}`));
				socket.close();
			}, options.connectTimeoutMs ?? 10_000);

			const onOpen = () => {
				opened = true;
				clearTimeout(connectTimeout);
				socket.removeEventListener?.("error", onError);
				const reconnected = wasConnected;
				wasConnected = true;
				reconnectAttempts = 0;
				startHeartbeatTimer();
				if (!settled) {
					settled = true;
					resolve(transport);
				} else if (reconnected) {
					options.onReconnect?.();
				}
			};

			const onError = () => {
				failedBeforeOpen = true;
				clearTimeout(connectTimeout);
				socket.removeEventListener?.("open", onOpen);
				if (!settled) scheduleReconnect(new Error(`WebSocket error connecting to ${wsUrl}`));
			};

			socket.addEventListener("open", onOpen, { once: true });
			socket.addEventListener("error", onError, { once: true });

			socket.addEventListener("message", (ev: MessageEvent) => {
				const bytes = toBytes(ev.data as ArrayBuffer | Uint8Array | string);

				// Intercept ping/pong at transport level.
				try {
					const msg = decodeFrame<Record<string, unknown>>(bytes);
					if (msg.method === "ping") {
						socket.send(encodeFrame({ method: "pong" }));
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

			socket.addEventListener("close", () => {
				clearTimeout(connectTimeout);
				stopHeartbeat();
				if (!opened && failedBeforeOpen) return;
				if (closed) return;
				if (settled) onDisconnect();
				scheduleReconnect(new Error(`WebSocket disconnected from ${wsUrl}`));
			});
		}

		connect();
	});
}
