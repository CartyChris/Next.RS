//! WebSocket routes. An API route that exports `socket` accepts upgrades; every socket for
//! the same URL path is pinned to the same JS worker, so module-level state in the route is
//! one consistent room. Topic fan-out (`publish`) happens here in Rust, across workers.

use std::{sync::Arc, time::Duration};

use axum::{
    extract::{
        FromRequestParts, Request,
        ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;

use super::{App, header_list, request_url};
use crate::{
    build::manifest::RouteEntry,
    render,
    routes::Params,
    runtime::{SOCKET_QUEUE, SocketJob, WsData, WsOut},
};

/// Sustained incoming messages per second per socket, with bursts up to `BURST`.
const RATE: f64 = 120.0;
const BURST: f64 = 240.0;
const PING_EVERY: Duration = Duration::from_secs(25);

pub fn is_upgrade(headers: &HeaderMap) -> bool {
    headers.get(header::UPGRADE).and_then(|v| v.to_str().ok()).is_some_and(|v| v.eq_ignore_ascii_case("websocket"))
}

pub async fn upgrade(app: Arc<App>, route: &RouteEntry, params: Params, req: Request) -> Response {
    let (mut parts, _) = req.into_parts();
    let ws = match WebSocketUpgrade::from_request_parts(&mut parts, &()).await {
        Ok(ws) => ws,
        Err(rejection) => return rejection.into_response(),
    };
    let pathname = parts.uri.path().to_string();
    let url = request_url(&parts.uri, &parts.headers);
    let headers = header_list(&parts.headers);
    let request = render::Request {
        kind: "socket",
        route: &route.id,
        params: render::params_json(&params),
        pathname: &pathname,
        url: &url,
        method: "GET",
        headers: &headers,
        nonce: "",
        stream: false,
    };
    let request = serde_json::to_string(&request).expect("request serializes");
    let worker = app.pool.socket_worker(&pathname);
    let max = app.project.config.server.max_body_bytes;
    ws.max_message_size(max).on_upgrade(move |socket| run(app, worker, request, socket))
}

async fn run(app: Arc<App>, worker: usize, request: String, socket: WebSocket) {
    let sid = app.pool.next_id();
    let (tx, mut rx) = mpsc::channel(SOCKET_QUEUE);
    let sockets = &app.pool.shared.sockets;
    sockets.register(sid, worker, tx);
    if !app.pool.send_socket(worker, SocketJob::Open { sid, request }) {
        sockets.unregister(sid);
        return;
    }
    let (mut sink, mut stream) = socket.split();
    let mut ping = tokio::time::interval(PING_EVERY);
    ping.tick().await;
    let mut tokens = BURST;
    let mut refilled = tokio::time::Instant::now();
    let (mut code, mut reason) = (1005u16, String::new());
    loop {
        tokio::select! {
            out = rx.recv() => match out {
                Some(WsOut::Data(WsData::Text(t))) => if sink.send(Message::Text(t.into())).await.is_err() { break },
                Some(WsOut::Data(WsData::Binary(b))) => if sink.send(Message::Binary(b)).await.is_err() { break },
                Some(WsOut::Close(c, r)) => {
                    let _ = sink.send(Message::Close(Some(CloseFrame { code: c, reason: r.clone().into() }))).await;
                    (code, reason) = (c, r);
                    break;
                }
                None => break,
            },
            msg = stream.next() => {
                let data = match msg {
                    Some(Ok(Message::Text(t))) => WsData::Text(t.to_string()),
                    Some(Ok(Message::Binary(b))) => WsData::Binary(b),
                    Some(Ok(Message::Close(frame))) => {
                        if let Some(f) = frame {
                            (code, reason) = (f.code, f.reason.to_string());
                        }
                        break;
                    }
                    Some(Ok(_)) => continue, // ping/pong: answered by axum
                    Some(Err(_)) | None => {
                        code = 1006;
                        break;
                    }
                };
                let now = tokio::time::Instant::now();
                tokens = (tokens + (now - refilled).as_secs_f64() * RATE).min(BURST);
                refilled = now;
                if tokens < 1.0 {
                    let _ = sink.send(Message::Close(Some(CloseFrame { code: 1008, reason: "rate limited".into() }))).await;
                    (code, reason) = (1008, "rate limited".into());
                    break;
                }
                tokens -= 1.0;
                app.pool.send_socket(worker, SocketJob::Message { sid, data });
            }
            _ = ping.tick() => if sink.send(Message::Ping(Bytes::new())).await.is_err() { break },
        }
    }
    sockets.unregister(sid);
    app.pool.send_socket(worker, SocketJob::Close { sid, code, reason });
}
