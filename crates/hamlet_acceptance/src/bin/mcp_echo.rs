//! An MCP server for the suite, over its standard input and output:
//! `echo` gives back its `text` and keeps it in the file `echoed`,
//! and `token` gives its `TOKEN`. It leaves the file `listed` once it
//! lists its tools. Its files are in its working folder.

use std::io::{BufRead, Write};

use serde_json::{Value, json};

fn main() {
    let mut out = std::io::stdout().lock();
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else {
            return;
        };
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        // A notification is answered with nothing.
        let Some(id) = message.get("id").cloned() else {
            continue;
        };
        let params = &message["params"];
        let answer = match message["method"]
            .as_str()
            .unwrap_or_default()
        {
            "initialize" => Ok(json!({
                "protocolVersion": params["protocolVersion"],
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "echo", "version": "0.1.0"},
            })),
            "tools/list" => {
                std::fs::write("listed", "")
                    .expect("its folder is its own");
                Ok(json!({"tools": [
                    {
                        "name": "echo",
                        "description": "Gives back the text.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {"text": {"type": "string"}},
                            "required": ["text"],
                        },
                    },
                    {
                        "name": "token",
                        "inputSchema": {"type": "object"},
                        "annotations": {"readOnlyHint": true},
                    },
                ]}))
            }
            "tools/call" => {
                let text = match params["name"].as_str() {
                    Some("echo") => {
                        let text = &params["arguments"]["text"];
                        std::fs::write(
                            "echoed",
                            text.as_str().unwrap_or_default(),
                        )
                        .expect("its folder is its own");
                        text.clone()
                    }
                    _ => json!(
                        std::env::var("TOKEN").unwrap_or_default()
                    ),
                };
                Ok(
                    json!({"content": [{"type": "text", "text": text}]}),
                )
            }
            "ping" => Ok(json!({})),
            _ => Err(
                json!({"code": -32601, "message": "method not found"}),
            ),
        };
        let reply = match answer {
            Ok(result) => {
                json!({"jsonrpc": "2.0", "id": id, "result": result})
            }
            Err(error) => {
                json!({"jsonrpc": "2.0", "id": id, "error": error})
            }
        };
        writeln!(out, "{reply}").expect("the client reads");
        out.flush().expect("the client reads");
    }
}
