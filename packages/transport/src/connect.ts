import { Connection } from "./connection.ts";
import { createAcceptedWebSocketTransport, createWebSocketTransport } from "./transport.ts";
import type { AcceptedWebSocketTransport, Transport, TransportOptions, WebSocketPeer } from "./transport.ts";

export interface WebSocketConnection {
	transport: Transport;
	connection: Connection;
}

export interface AcceptedWebSocketConnection {
	transport: AcceptedWebSocketTransport;
	connection: Connection;
}

export interface WebSocketConnectionOptions extends Omit<TransportOptions, "onHeartbeatDead"> {
	onDisconnect?: () => void;
	onHeartbeatDead?: (conn: WebSocketConnection) => void;
}

export async function createWebSocketConnection(
	url: string,
	options: WebSocketConnectionOptions = {},
): Promise<WebSocketConnection> {
	let result: WebSocketConnection | null = null;
	const pendingFrames: Uint8Array[] = [];

	const transport = await createWebSocketTransport(
		url,
		{
			...options,
			onHeartbeatDead: () => {
				if (result) options.onHeartbeatDead?.(result);
			},
		},
		(data) => {
			if (result) {
				result.connection.handleMessage(data);
			} else {
				pendingFrames.push(data);
			}
		},
		() => options.onDisconnect?.(),
	);

	const connection = new Connection(transport);
	result = { transport, connection };

	for (const frame of pendingFrames) {
		connection.handleMessage(frame);
	}
	pendingFrames.length = 0;

	return result;
}

export function createAcceptedWebSocketConnection(
	peer: WebSocketPeer,
	options: WebSocketConnectionOptions = {},
): AcceptedWebSocketConnection {
	let result: AcceptedWebSocketConnection | null = null;
	const pendingFrames: Uint8Array[] = [];

	const transport = createAcceptedWebSocketTransport(
		peer,
		{
			...options,
			onHeartbeatDead: () => {
				if (result) options.onHeartbeatDead?.(result);
			},
		},
		(data) => {
			if (result) {
				result.connection.handleMessage(data);
			} else {
				pendingFrames.push(data);
			}
		},
		() => options.onDisconnect?.(),
	);

	const connection = new Connection(transport);
	result = { transport, connection };

	for (const frame of pendingFrames) {
		connection.handleMessage(frame);
	}
	pendingFrames.length = 0;

	return result;
}
