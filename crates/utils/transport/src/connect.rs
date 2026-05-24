//! Connect helpers — pair a [`Transport`] with a [`Connection`].
//!
//! Mirrors the TypeScript `packages/transport/src/connect.ts` helpers
//! [`createWebSocketConnection`] / [`createAcceptedWebSocketConnection`].

use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tungstenite::WebSocketStream;

use crate::connection::Connection;
use crate::transport::{
    ClientTransportError, Transport, TransportOptions, accepted_websocket_transport,
    connect_websocket_transport,
};

/// A [`Transport`] paired with the [`Connection`] driving it.
pub struct WebSocketConnection {
    pub transport: Transport,
    pub connection: Connection,
}

/// Dial `url`, build a transport, and wrap it in a [`Connection`].
pub async fn create_websocket_connection(
    url: impl Into<String>,
    options: TransportOptions,
) -> Result<WebSocketConnection, ClientTransportError> {
    let (transport, events) = connect_websocket_transport(url, options).await?;
    let connection = Connection::new(transport.clone(), events);
    Ok(WebSocketConnection {
        transport,
        connection,
    })
}

/// Wrap a server-accepted [`WebSocketStream`] in a transport + connection.
pub fn create_accepted_websocket_connection<S>(
    stream: WebSocketStream<S>,
    options: TransportOptions,
) -> WebSocketConnection
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (transport, events) = accepted_websocket_transport(stream, options);
    let connection = Connection::new(transport.clone(), events);
    WebSocketConnection {
        transport,
        connection,
    }
}
