//! forge:ai chat() + runAgent() against fake OpenAI, Anthropic, Gemini and Ollama servers.

mod common;

use std::sync::{Arc, Mutex};

use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    response::Response,
};
use common::*;
use serde_json::{Value, json};

type Log = Arc<Mutex<Vec<(String, Value, Vec<(String, String)>)>>>;

fn sse(events: &[Value]) -> String {
    events.iter().map(|e| format!("data: {e}\n\n")).collect()
}

/// First call: ask for the weather tool (arguments split across deltas). Second call
/// (after a tool result): answer in text.
async fn fake(State(log): State<Log>, req: Request) -> Response {
    let path = req.uri().path().to_string();
    let headers = req.headers().iter().map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string())).collect();
    let body: Value =
        serde_json::from_slice(&axum::body::to_bytes(req.into_body(), usize::MAX).await.unwrap()).unwrap();
    let after_tool = body.to_string().contains("tempC");
    log.lock().unwrap().push((path.clone(), body, headers));
    let (ctype, out) = if path.starts_with("/openai") {
        let events = if after_tool {
            vec![
                json!({"choices":[{"delta":{"content":"It is 21C "}}]}),
                json!({"choices":[{"delta":{"content":"in Paris."},"finish_reason":"stop"}]}),
                json!({"choices":[],"usage":{"prompt_tokens":30,"completion_tokens":6}}),
            ]
        } else {
            vec![
                json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"get_weather","arguments":"{\"ci"}}]}}]}),
                json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"ty\":\"Paris\"}"}}]},"finish_reason":"tool_calls"}]}),
            ]
        };
        ("text/event-stream", sse(&events) + "data: [DONE]\n\n")
    } else if path.starts_with("/anthropic") {
        let events = if after_tool {
            vec![
                json!({"type":"message_start","message":{"usage":{"input_tokens":30}}}),
                json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
                json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"It is 21C in Paris."}}),
                json!({"type":"content_block_stop","index":0}),
                json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":6}}),
                json!({"type":"message_stop"}),
            ]
        } else {
            vec![
                json!({"type":"message_start","message":{"usage":{"input_tokens":20}}}),
                json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_1","name":"get_weather","input":{}}}),
                json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"city\":"}}),
                json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"\"Paris\"}"}}),
                json!({"type":"content_block_stop","index":0}),
                json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":9}}),
            ]
        };
        ("text/event-stream", sse(&events))
    } else if path.starts_with("/gemini") {
        let events = if after_tool {
            vec![
                json!({"candidates":[{"content":{"parts":[{"text":"It is 21C in Paris."}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":30,"candidatesTokenCount":6}}),
            ]
        } else {
            vec![
                json!({"candidates":[{"content":{"parts":[{"functionCall":{"name":"get_weather","args":{"city":"Paris"}}}]},"finishReason":"STOP"}]}),
            ]
        };
        ("text/event-stream", sse(&events))
    } else {
        let lines = if after_tool {
            vec![
                json!({"message":{"content":"It is 21C "},"done":false}),
                json!({"message":{"content":"in Paris."},"done":true,"prompt_eval_count":30,"eval_count":6}),
            ]
        } else {
            vec![
                json!({"message":{"content":"","tool_calls":[{"function":{"name":"get_weather","arguments":{"city":"Paris"}}}]},"done":true}),
            ]
        };
        ("application/x-ndjson", lines.iter().map(|l| format!("{l}\n")).collect())
    };
    Response::builder().header("content-type", ctype).body(Body::from(out)).unwrap()
}

const AGENT_ROUTE: &str = r#"
import { runAgent, chat } from "forge:ai";
export async function GET(request, { searchParams }) {
  const provider = searchParams.get("p");
  const events = [];
  for await (const ev of runAgent({
    provider, baseUrl: searchParams.get("base"), model: "m", apiKey: "k",
    system: "Be brief.",
    messages: [{ role: "user", content: "Weather in Paris?" }],
    tools: { get_weather: { description: "Current weather", parameters: { type: "object", properties: { city: { type: "string" } } }, run: ({ city }) => ({ city, tempC: 21 }) } },
  })) events.push(ev.type === "final" ? { type: "final", text: ev.text, steps: ev.steps } : ev);
  const mock = await chat({ provider: "mock", messages: [{ role: "user", content: "hi" }] }).text();
  return { events, mock };
}
"#;

#[tokio::test(flavor = "multi_thread")]
async fn agents_work_across_all_providers() {
    let log: Log = Arc::default();
    let upstream = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let up = format!("http://{}", upstream.local_addr().unwrap());
    let router = Router::new().fallback(fake).with_state(log.clone());
    tokio::spawn(async move { axum::serve(upstream, router).await });

    let root = project(
        "ai",
        &[
            ("app/page.js", "export const render = () => '';"),
            ("app/api/agent/route.js", AGENT_ROUTE),
            ("forge.config.json", r#"{"server":{"allowFetch":["127.0.0.1"]}}"#),
        ],
    );
    let app = serve(&root).await;
    for provider in ["openai", "anthropic", "gemini", "ollama"] {
        log.lock().unwrap().clear();
        let res = get(&app, &format!("/api/agent?p={provider}&base={up}/{provider}")).await;
        let v: Value = serde_json::from_str(&res.body).unwrap_or_else(|_| panic!("{provider}: {}", res.body));
        let events = v["events"].as_array().unwrap();
        let call = events
            .iter()
            .find(|e| e["type"] == "tool_call")
            .unwrap_or_else(|| panic!("{provider}: no tool call in {events:?}"));
        assert_eq!(
            (call["name"].as_str(), &call["arguments"]),
            (Some("get_weather"), &json!({"city": "Paris"})),
            "{provider}"
        );
        let result = events.iter().find(|e| e["type"] == "tool_result").unwrap();
        assert_eq!(result["result"], json!({"city": "Paris", "tempC": 21}), "{provider}");
        let text: String = events.iter().filter(|e| e["type"] == "text").map(|e| e["text"].as_str().unwrap()).collect();
        assert_eq!(text, "It is 21C in Paris.", "{provider}");
        let last = events.last().unwrap();
        assert_eq!((last["type"].as_str(), last["steps"].as_i64()), (Some("final"), Some(2)), "{provider}");
        assert!(events.iter().any(|e| e["type"] == "usage" && e["output"] == 6), "{provider}: usage");
        assert_eq!(v["mock"], "(mock model) You said: hi");

        let requests = log.lock().unwrap().clone();
        assert_eq!(requests.len(), 2, "{provider}: two model calls");
        let (_, second, headers) = &requests[1];
        let has = |k: &str, v: &str| headers.iter().any(|(hk, hv)| hk == k && hv == v);
        match provider {
            "openai" => {
                assert!(has("authorization", "Bearer k"));
                let msgs = second["messages"].as_array().unwrap();
                assert_eq!(msgs[0], json!({"role": "system", "content": "Be brief."}));
                assert_eq!(msgs[2]["tool_calls"][0]["function"]["arguments"], "{\"city\":\"Paris\"}");
                assert_eq!(msgs[3]["role"], "tool");
                assert_eq!(msgs[3]["tool_call_id"], "call_1");
                assert_eq!(second["tools"][0]["function"]["name"], "get_weather");
            }
            "anthropic" => {
                assert!(has("x-api-key", "k") && has("anthropic-version", "2023-06-01"));
                assert_eq!(second["system"], "Be brief.");
                assert_eq!(second["messages"][1]["content"][0]["type"], "tool_use");
                assert_eq!(second["messages"][2]["content"][0]["tool_use_id"], "toolu_1");
                assert_eq!(second["tools"][0]["input_schema"]["type"], "object");
            }
            "gemini" => {
                assert!(has("x-goog-api-key", "k"));
                assert_eq!(second["systemInstruction"]["parts"][0]["text"], "Be brief.");
                assert_eq!(second["contents"][1]["parts"][0]["functionCall"]["name"], "get_weather");
                assert_eq!(second["contents"][2]["parts"][0]["functionResponse"]["response"]["content"]["tempC"], 21);
            }
            _ => {
                assert_eq!(second["messages"][3]["role"], "tool");
                assert_eq!(second["messages"][2]["tool_calls"][0]["function"]["arguments"], json!({"city": "Paris"}));
            }
        }
    }
    std::fs::remove_dir_all(root).ok();
}
