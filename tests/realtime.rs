//! WebSocket rooms, pub/sub, and request cancellation.

mod common;

use std::time::Duration;

use axum::http::StatusCode;
use common::*;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{connect_async, tungstenite::Message};

const ROOM: &str = r#"
import { publish, subscribers } from "forge:realtime";

// Module state is shared by every socket on the same path (they share a worker).
const rooms = new Map();

export const socket = {
  open(ws) {
    const name = ws.params.room;
    const room = rooms.get(name) ?? { members: new Set(), count: 0 };
    rooms.set(name, room);
    room.members.add(ws);
    ws.data.room = room;
    ws.subscribe("room:" + name);
    ws.send({ type: "hello", members: room.members.size });
  },
  message(ws, msg) {
    const room = ws.data.room;
    room.count += msg.add ?? 0;
    ws.publish("room:" + ws.params.room, { type: "count", count: room.count, from: ws.id });
    ws.send({ type: "count", count: room.count, self: true });
  },
  close(ws) {
    ws.data.room.members.delete(ws);
  },
};

export function POST(request, { params }) {
  return { delivered: publish("room:" + params.room, { type: "announce" }), subscribers: subscribers("room:" + params.room) };
}
"#;

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn recv(ws: &mut Ws) -> serde_json::Value {
    loop {
        let msg =
            tokio::time::timeout(Duration::from_secs(5), ws.next()).await.expect("message in time").unwrap().unwrap();
        if let Message::Text(t) = msg {
            return serde_json::from_str(&t).unwrap();
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn socket_rooms_share_state_and_publish_across_workers() {
    let root = project(
        "rooms",
        &[
            ("app/page.js", "export const render = () => '';"),
            ("app/api/room/[room]/route.js", ROOM),
            ("forge.config.json", r#"{"server":{"workers":4}}"#),
        ],
    );
    let base = listen(&root).await;
    let ws_url = |room: &str| format!("{}/api/room/{room}", base.replace("http", "ws"));

    let (mut a, _) = connect_async(ws_url("red")).await.unwrap();
    assert_eq!(recv(&mut a).await["members"], 1);
    let (mut b, _) = connect_async(ws_url("red")).await.unwrap();
    assert_eq!(recv(&mut b).await["members"], 2, "same path, same room state");
    let (mut c, _) = connect_async(ws_url("blue")).await.unwrap();
    assert_eq!(recv(&mut c).await["members"], 1, "different room");

    a.send(Message::text(r#"{"add":5}"#)).await.unwrap();
    assert_eq!(recv(&mut a).await, serde_json::json!({"type": "count", "count": 5, "self": true}));
    let seen = recv(&mut b).await;
    assert_eq!((seen["type"].as_str(), seen["count"].as_i64()), (Some("count"), Some(5)));
    b.send(Message::text(r#"{"add":2}"#)).await.unwrap();
    assert_eq!(recv(&mut b).await["count"], 7);
    assert_eq!(recv(&mut a).await["count"], 7);

    // An HTTP request on any worker publishes to sockets on another.
    let client = reqwest::Client::new();
    let res: serde_json::Value =
        client.post(format!("{base}/api/room/red")).send().await.unwrap().json().await.unwrap();
    assert_eq!(res, serde_json::json!({"delivered": 2, "subscribers": 2}));
    assert_eq!(recv(&mut a).await["type"], "announce");
    assert_eq!(recv(&mut b).await["type"], "announce");

    b.close(None).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    let res: serde_json::Value =
        client.post(format!("{base}/api/room/red")).send().await.unwrap().json().await.unwrap();
    assert_eq!(res["subscribers"], 1, "closed sockets leave their topics");

    let ready: serde_json::Value =
        client.get(format!("{base}/health/ready")).send().await.unwrap().json().await.unwrap();
    assert_eq!(ready["sockets"], 2);
    drop((a, c));
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn socket_errors_close_with_1011_and_floods_are_cut_off() {
    let root = project(
        "sockerr",
        &[
            ("app/page.js", "export const render = () => '';"),
            (
                "app/api/bad/route.js",
                "export const socket = { message(ws, m) { if (m === 'boom') throw new Error('nope'); ws.send(m); } };",
            ),
        ],
    );
    let base = listen(&root).await;
    let (mut ws, _) = connect_async(format!("{}/api/bad", base.replace("http", "ws"))).await.unwrap();
    ws.send(Message::text("hi")).await.unwrap();
    assert_eq!(ws.next().await.unwrap().unwrap(), Message::text("hi"));
    ws.send(Message::text("boom")).await.unwrap();
    match ws.next().await.unwrap().unwrap() {
        Message::Close(Some(f)) => assert_eq!(u16::from(f.code), 1011),
        other => panic!("expected close, got {other:?}"),
    }

    let (mut flood, _) = connect_async(format!("{}/api/bad", base.replace("http", "ws"))).await.unwrap();
    for _ in 0..1000 {
        if flood.send(Message::text("x")).await.is_err() {
            break;
        }
    }
    let mut closed = None;
    while let Some(Ok(m)) = flood.next().await {
        if let Message::Close(f) = m {
            closed = f.map(|f| u16::from(f.code));
            break;
        }
    }
    assert_eq!(closed, Some(1008));
    // Plain HTTP to a socket-only route answers 405.
    assert_eq!(
        reqwest::get(format!("{base}/api/bad")).await.unwrap().status(),
        StatusCode::METHOD_NOT_ALLOWED.as_u16()
    );
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn disconnect_and_timeout_abort_server_work() {
    let root = project(
        "abort",
        &[
            ("app/page.js", "export const render = () => '';"),
            (
                "app/api/slow/route.js",
                r#"import { cache } from "forge:cache";
export async function GET(request, { signal, searchParams }) {
  const key = searchParams.get("key");
  signal.addEventListener("abort", () => cache(key, () => "aborted:" + (request.signal === signal)));
  await new Promise((r) => setTimeout(r, 3000));
  return "done";
}"#,
            ),
            (
                "app/api/upstream/route.js",
                r#"import { cache } from "forge:cache";
export async function GET(request, { signal, searchParams }) {
  try {
    await fetch(searchParams.get("url"), { signal });
    return "fetched";
  } catch (e) {
    await cache("fetch", () => e.name + ":" + e.message.slice(0, 20));
    throw e;
  }
}"#,
            ),
            (
                "app/api/flag/route.js",
                "import { cache } from 'forge:cache';\nexport const GET = async (r, { searchParams }) => (await cache(searchParams.get('key'), () => 'none'));",
            ),
            ("forge.config.json", r#"{"server":{"workers":1,"timeoutMs":600,"allowFetch":["127.0.0.1"]}}"#),
        ],
    );
    let base = listen(&root).await;
    let client = reqwest::Client::new();

    // Client disconnects mid-request.
    let r = client.get(format!("{base}/api/slow?key=gone")).timeout(Duration::from_millis(150)).send().await;
    assert!(r.is_err());
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        client.get(format!("{base}/api/flag?key=gone")).send().await.unwrap().text().await.unwrap(),
        r#""aborted:true""#
    );

    // Server-side timeout: 504 and the signal fires.
    let r = client.get(format!("{base}/api/slow?key=late")).send().await.unwrap();
    assert_eq!(r.status(), 504);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        client.get(format!("{base}/api/flag?key=late")).send().await.unwrap().text().await.unwrap(),
        r#""aborted:true""#
    );

    // An outbound fetch is cancelled with its request.
    let hang = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let hang_addr = hang.local_addr().unwrap();
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((s, _)) = hang.accept().await {
            held.push(s);
        }
    });
    let r = client
        .get(format!("{base}/api/upstream?url=http://{hang_addr}/"))
        .timeout(Duration::from_millis(150))
        .send()
        .await;
    assert!(r.is_err());
    tokio::time::sleep(Duration::from_millis(200)).await;
    let flag = client.get(format!("{base}/api/flag?key=fetch")).send().await.unwrap().text().await.unwrap();
    assert!(flag.starts_with(r#""AbortError:"#), "{flag}");
    std::fs::remove_dir_all(root).ok();
}
