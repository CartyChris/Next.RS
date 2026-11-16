//! `forge mcp` end to end: a JSON-RPC session over stdio with the real binary, then HTTP.

use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

use serde_json::{Value, json};

struct Session {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next: u64,
}

impl Session {
    fn start(dir: &std::path::Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_forge"))
            .arg("mcp")
            .current_dir(dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Self { child, stdin, stdout, next: 0 }
    }

    fn send(&mut self, message: &Value) {
        writeln!(self.stdin, "{message}").unwrap();
    }

    /// Every line on stdout must be one JSON-RPC message.
    fn read(&mut self) -> Value {
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap_or_else(|e| panic!("stdout is not JSON ({e}): {line:?}"))
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = self.next;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        let reply = self.read();
        assert_eq!(reply["id"], id, "{reply}");
        reply
    }

    /// Call a tool; returns `(text, structuredContent, isError)`.
    fn tool(&mut self, name: &str, arguments: Value) -> (String, Value, bool) {
        let reply = self.request("tools/call", json!({ "name": name, "arguments": arguments }));
        let result = &reply["result"];
        let text = result["content"][0]["text"].as_str().unwrap_or_else(|| panic!("no text in {reply}")).to_string();
        (text, result["structuredContent"].clone(), result["isError"].as_bool().unwrap_or(false))
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn mcp_session_over_stdio() {
    let dir = std::env::temp_dir().join(format!("forge-mcp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = Session::start(&dir);

    let init = s.request("initialize", json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "0" } }));
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(init["result"]["serverInfo"]["name"], "forge");
    assert!(init["result"]["capabilities"]["tools"].is_object());
    // An unknown revision gets the newest one this server speaks.
    let other = s.request("initialize", json!({ "protocolVersion": "1999-01-01" }));
    assert_eq!(other["result"]["protocolVersion"], "2025-11-25");
    // Notifications get no reply: the next line answers the ping.
    s.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
    assert_eq!(s.request("ping", json!({}))["result"], json!({}));

    let tools = s.request("tools/list", json!({}));
    let tools = tools["result"]["tools"].as_array().unwrap().clone();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    for expected in [
        "forge_docs_search",
        "forge_docs_read",
        "forge_ui_component",
        "forge_ui_themes",
        "forge_templates",
        "forge_new",
        "forge_routes",
        "forge_check",
        "forge_build",
        "forge_test",
        "forge_pack",
    ] {
        assert!(names.contains(&expected), "{expected} missing from {names:?}");
    }
    for tool in &tools {
        assert!(tool["name"].as_str().unwrap().starts_with("forge_"));
        assert!(tool["description"].as_str().unwrap().len() > 40, "{tool}");
        assert_eq!(tool["inputSchema"]["type"], "object");
        assert!(tool["annotations"]["readOnlyHint"].is_boolean(), "{tool}");
    }

    // Docs: search, outline, section.
    let (text, structured, _) =
        s.tool("forge_docs_search", json!({ "query": "loader redirect notFound", "doc": "guide", "limit": 1 }));
    assert!(text.contains("redirect(") && structured["sections"][0]["doc"] == "guide", "{text}");
    assert_eq!(structured["sections"].as_array().unwrap().len(), 1);
    assert_eq!(structured["hasMore"], true);
    let (outline, structured, _) = s.tool("forge_docs_read", json!({ "doc": "ui" }));
    assert!(outline.contains("- Theme") && structured["sections"].as_array().unwrap().len() > 3, "{outline}");
    let (section, _, _) = s.tool("forge_docs_read", json!({ "doc": "design", "section": "what to avoid" }));
    assert!(section.contains("Emoji"), "{section}");
    let (miss, _, is_error) = s.tool("forge_docs_search", json!({ "query": "qqqqzzzz" }));
    assert!(is_error && miss.contains("forge_docs_read"), "{miss}");

    // forge:ui lookup.
    let (list, structured, _) = s.tool("forge_ui_component", json!({}));
    assert!(list.contains("CommandPalette") && structured["components"].as_array().unwrap().len() > 40, "{list}");
    let (button, _, _) = s.tool("forge_ui_component", json!({ "name": "Button" }));
    assert!(button.contains("loading?: boolean") && button.contains("ButtonVariant"), "{button}");
    let (typo, _, is_error) = s.tool("forge_ui_component", json!({ "name": "Buton" }));
    assert!(is_error && typo.contains("Did you mean") && typo.contains("Button"), "{typo}");

    // Create, inspect, check and build a project (the vanilla template needs no npm install).
    let (templates, _, _) = s.tool("forge_templates", json!({}));
    assert!(templates.contains("- starter:") && templates.contains("- vanilla:"), "{templates}");
    let (created, structured, is_error) = s.tool("forge_new", json!({ "dir": "site", "template": "vanilla" }));
    assert!(!is_error && structured["files"].as_array().unwrap().len() > 3, "{created}");
    let (again, _, is_error) = s.tool("forge_new", json!({ "dir": "site", "template": "vanilla" }));
    assert!(is_error && again.contains("not empty"), "{again}");
    let (_, routes, _) = s.tool("forge_routes", json!({ "root": "site" }));
    let patterns: Vec<&str> =
        routes["routes"].as_array().unwrap().iter().map(|r| r["pattern"].as_str().unwrap()).collect();
    assert!(
        patterns.contains(&"/") && patterns.contains(&"/notes/[id]") && patterns.contains(&"/api/time"),
        "{patterns:?}"
    );
    let (_, checked, is_error) = s.tool("forge_check", json!({ "root": "site" }));
    assert!(!is_error && checked["ok"] == true);
    assert!(dir.join("site/.forge/types/forge-ui.d.ts").is_file());
    let (_, built, is_error) = s.tool("forge_build", json!({ "root": "site", "target": "single-html" }));
    assert!(!is_error && built["files"][0]["bytes"].as_u64().unwrap() > 1000, "{built}");
    assert!(dir.join("site/dist/index.html").is_file());

    // Tests run in the engine; failures are listed with their message.
    std::fs::write(
        dir.join("site/math.test.js"),
        "import { test, expect } from 'forge:test';\ntest('adds', () => expect(1 + 1).toBe(2));\ntest('fails', () => expect(1).toBe(3));\n",
    )
    .unwrap();
    let (report, tested, is_error) = s.tool("forge_test", json!({ "root": "site" }));
    assert!(!is_error && tested["passed"] == 1 && tested["failed"] == 1 && tested["ok"] == false, "{tested}");
    assert!(report.contains("fails"), "{report}");
    std::fs::remove_file(dir.join("site/math.test.js")).unwrap();

    // Failures are readable results with a next step, not protocol errors.
    std::fs::write(dir.join("site/app/page.js"), "export function mount() { eval('1'); }").unwrap();
    let (denied, _, is_error) = s.tool("forge_check", json!({ "root": "site" }));
    assert!(is_error && denied.contains("`eval` is denied") && denied.contains("app/page.js:1"), "{denied}");
    let (missing, _, is_error) = s.tool("forge_routes", json!({ "root": "nowhere" }));
    assert!(is_error && missing.contains("forge_new"), "{missing}");
    let (unknown, _, is_error) = s.tool("forge_nope", json!({}));
    assert!(is_error && unknown.contains("tools/list"), "{unknown}");
    assert_eq!(s.request("no/such/method", json!({}))["error"]["code"], -32601);

    // Resources and prompts.
    let resources = s.request("resources/list", json!({}));
    assert!(resources["result"]["resources"].as_array().unwrap().iter().any(|r| r["uri"] == "forge://docs/guide"));
    let guide = s.request("resources/read", json!({ "uri": "forge://docs/design" }));
    assert!(guide["result"]["contents"][0]["text"].as_str().unwrap().contains("# Design rules"));
    let prompt =
        s.request("prompts/get", json!({ "name": "forge_build_app", "arguments": { "idea": "a habit tracker" } }));
    assert!(prompt["result"]["messages"][0]["content"]["text"].as_str().unwrap().contains("a habit tracker"));
    // Malformed input is answered, and the session goes on.
    writeln!(s.stdin, "{{not json").unwrap();
    assert_eq!(s.read()["error"]["code"], -32700);
    assert_eq!(s.request("ping", json!({}))["result"], json!({}));
    drop(s);
    std::fs::remove_dir_all(dir).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn mcp_over_http_for_network_clients() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_forge"))
        .args(["mcp", "--http", "0"])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stderr.take().unwrap()).read_line(&mut line).unwrap();
    let url = line.trim().rsplit(' ').next().unwrap().to_string();
    assert!(url.starts_with("http://127.0.0.1:") && url.ends_with("/mcp"), "{line}");
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let post = |body: Value| client.post(&url).json(&body).send();

    let reply: Value =
        post(json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" })).await.unwrap().json().await.unwrap();
    assert!(reply["result"]["tools"].as_array().unwrap().len() >= 10);
    let call = json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": { "name": "forge_ui_component", "arguments": { "name": "Theme" } } });
    let reply: Value = post(call).await.unwrap().json().await.unwrap();
    assert!(reply["result"]["content"][0]["text"].as_str().unwrap().contains("accent?: Accent"));
    // A notification is accepted without a body; a foreign web origin is refused.
    let note = post(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).await.unwrap();
    assert_eq!(note.status(), 202);
    let foreign = client
        .post(&url)
        .header("origin", "https://evil.example")
        .json(&json!({ "jsonrpc": "2.0", "id": 3, "method": "ping" }))
        .send()
        .await
        .unwrap();
    assert_eq!(foreign.status(), 403);
    let _ = child.kill();
    let _ = child.wait();
}

/// The evaluation set stays answerable: every answer appears in what the server can return.
#[test]
fn evaluation_answers_are_in_the_servers_knowledge() {
    let xml = include_str!("fixtures/mcp/evaluation.xml");
    let mut corpus: String = nextrs::mcp::DOCS.iter().map(|d| d.2).collect();
    corpus.push_str(&serde_json::to_string(&nextrs::mcp::ui_themes()).unwrap());
    let between = |text: &str, open: &str, close: &str| -> Vec<String> {
        text.split(open).skip(1).filter_map(|rest| rest.split(close).next()).map(|v| v.trim().to_string()).collect()
    };
    let questions = between(xml, "<question>", "</question>");
    let answers = between(xml, "<answer>", "</answer>");
    assert_eq!((questions.len(), answers.len()), (10, 10));
    for (question, answer) in questions.iter().zip(&answers) {
        assert!(question.len() > 40 && !answer.is_empty(), "{question}");
        assert!(
            corpus.contains(answer.as_str()),
            "the answer `{answer}` is not in the docs or the theme catalog: {question}"
        );
    }
}
