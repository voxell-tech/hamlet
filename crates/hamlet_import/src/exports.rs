//! What ChatGPT and Claude export: a zip holding
//! `conversations.json`, a list of every conversation. ChatGPT keeps
//! each as a tree of messages (`mapping`) with the one last shown at
//! `current_node`; Claude keeps a list (`chat_messages`).

use std::fs::File;
use std::io::Read as _;
use std::path::Path;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, bail};
use hamlet_protocol::client::ImportSource;
use serde_json::Value;

use crate::{Draft, Found, summary, time};

/// The name of the list of conversations in an export.
const CONVERSATIONS: &str = "conversations.json";

pub(crate) fn find(path: &Path) -> Result<Vec<Found>> {
    let listed = conversations(path)?
        .iter()
        .filter_map(|conversation| {
            let source = source_of(conversation)?;
            let (session, title, modified) = match source {
                ImportSource::Chatgpt => (
                    conversation["conversation_id"]
                        .as_str()
                        .or_else(|| conversation["id"].as_str())?,
                    conversation["title"].as_str(),
                    seconds(&conversation["update_time"]),
                ),
                _ => (
                    conversation["uuid"].as_str()?,
                    conversation["name"].as_str(),
                    time(&conversation["updated_at"]),
                ),
            };
            Some(Found {
                source,
                session: session.to_owned(),
                title: summary(title.unwrap_or(source.name())),
                folder: None,
                modified: SystemTime::UNIX_EPOCH
                    + Duration::from_millis(modified.unwrap_or(0)),
                path: path.to_owned(),
            })
        })
        .collect();
    Ok(listed)
}

pub(crate) fn read(path: &Path, session: &str) -> Result<Draft> {
    let conversations = conversations(path)?;
    let conversation = conversations
        .iter()
        .find(|conversation| {
            [
                &conversation["conversation_id"],
                &conversation["id"],
                &conversation["uuid"],
            ]
            .iter()
            .any(|id| id.as_str() == Some(session))
        })
        .with_context(|| {
            format!("no conversation {session} in {}", path.display())
        })?;
    match source_of(conversation) {
        Some(ImportSource::Chatgpt) => {
            Ok(chatgpt(session, conversation))
        }
        Some(_) => Ok(claude(session, conversation)),
        None => {
            bail!("{} is not an export Hamlet reads", path.display())
        }
    }
}

/// The conversations of the export at `path`: its zip, or the
/// `conversations.json` taken out of it.
fn conversations(path: &Path) -> Result<Vec<Value>> {
    let mut file = File::open(path)
        .with_context(|| format!("opening {}", path.display()))?;
    let mut magic = [0; 2];
    let is_zip = file.read(&mut magic)? == 2 && magic == *b"PK";
    let text = if is_zip {
        let mut archive = zip::ZipArchive::new(File::open(path)?)
            .with_context(|| {
                format!("reading the zip {}", path.display())
            })?;
        let mut list =
            archive.by_name(CONVERSATIONS).with_context(|| {
                format!("{} holds no {CONVERSATIONS}", path.display())
            })?;
        let mut text = String::new();
        list.read_to_string(&mut text)?;
        text
    } else {
        std::fs::read_to_string(path)?
    };
    match serde_json::from_str(&text) {
        Ok(Value::Array(conversations)) => Ok(conversations),
        _ => {
            bail!("{} is not a list of conversations", path.display())
        }
    }
}

fn source_of(conversation: &Value) -> Option<ImportSource> {
    if conversation["mapping"].is_object() {
        Some(ImportSource::Chatgpt)
    } else if conversation["chat_messages"].is_array() {
        Some(ImportSource::Claude)
    } else {
        None
    }
}

fn chatgpt(session: &str, conversation: &Value) -> Draft {
    let mapping = &conversation["mapping"];
    // A parent that loops back ends the walk once every node is seen.
    let most = mapping.as_object().map_or(0, serde_json::Map::len);
    let mut nodes = Vec::new();
    let mut at = conversation["current_node"].as_str();
    while let Some(id) = at
        && nodes.len() <= most
    {
        let node = &mapping[id];
        nodes.push(node);
        at = node["parent"].as_str();
    }
    let mut read =
        Draft::new(ImportSource::Chatgpt, session.to_owned());
    read.title = conversation["title"].as_str().map(str::to_owned);
    let began = seconds(&conversation["create_time"]).unwrap_or(0);
    for node in nodes.iter().rev() {
        let message = &node["message"];
        if message.is_null()
            || message["metadata"]
                ["is_visually_hidden_from_conversation"]
                == true
        {
            continue;
        }
        let at = seconds(&message["create_time"]).unwrap_or(began);
        let content = &message["content"];
        let text = content["parts"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join("\n\n");
        let to_tool = message["recipient"]
            .as_str()
            .is_some_and(|recipient| recipient != "all");
        match message["author"]["role"].as_str() {
            Some("user")
                if content["content_type"]
                    != "user_editable_context" =>
            {
                read.user(at, &text);
            }
            Some("assistant") if to_tool => {
                let tool =
                    message["recipient"].as_str().unwrap_or_default();
                let input = content["text"].as_str().unwrap_or(&text);
                read.step(at, tool, &Value::String(input.to_owned()));
            }
            Some("assistant")
                if content["content_type"] == "text" =>
            {
                if let Some(model) =
                    message["metadata"]["model_slug"].as_str()
                {
                    read.model = Some(model.to_owned());
                }
                read.assistant(at, &text);
            }
            _ => {}
        }
    }
    read
}

fn claude(session: &str, conversation: &Value) -> Draft {
    let mut read =
        Draft::new(ImportSource::Claude, session.to_owned());
    read.title = conversation["name"].as_str().map(str::to_owned);
    for message in conversation["chat_messages"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let at = time(&message["created_at"]).unwrap_or(0);
        let human = message["sender"] == "human";
        let blocks = message["content"]
            .as_array()
            .filter(|blocks| !blocks.is_empty());
        let Some(blocks) = blocks else {
            let text = message["text"].as_str().unwrap_or_default();
            if human {
                read.user(at, text);
            } else {
                read.assistant(at, text);
            }
            continue;
        };
        let mut said = Vec::new();
        for block in blocks {
            match block["type"].as_str() {
                Some("text") => said.extend(block["text"].as_str()),
                Some("tool_use") if !human => read.step(
                    at,
                    block["name"].as_str().unwrap_or_default(),
                    &block["input"],
                ),
                _ => {}
            }
            if !human {
                for text in said.drain(..) {
                    read.assistant(at, text);
                }
            }
        }
        if human {
            read.user(at, &said.join("\n\n"));
        }
    }
    read
}

/// A time in seconds since the epoch, with a fraction, in
/// milliseconds.
fn seconds(value: &Value) -> Option<u64> {
    let seconds = value.as_f64().filter(|seconds| *seconds >= 0.0)?;
    Some((seconds * 1000.0) as u64)
}

#[cfg(test)]
mod tests {
    use hamlet_protocol::client::ImportedEntry;
    use serde_json::json;

    use super::*;

    #[test]
    fn follows_the_branch_chatgpt_shows() {
        let message = |role: &str, text: &str, at: f64| {
            json!({"author": {"role": role}, "create_time": at, "recipient": "all",
                   "content": {"content_type": "text", "parts": [text]}})
        };
        let conversation = json!({
            "title": "Bread", "create_time": 100.0, "current_node": "d",
            "mapping": {
                "root": {"message": null, "parent": null},
                "a": {"message": message("user", "How long to proof?", 100.0), "parent": "root"},
                "b": {"message": message("assistant", "An hour.", 101.0), "parent": "a"},
                "c": {"message": message("assistant", "Two hours.", 102.0), "parent": "a"},
                "d": {"message": message("user", "And at 30 degrees?", 103.0), "parent": "c"},
            }
        });
        let imported = chatgpt("x", &conversation).finish();
        assert_eq!(imported.title, "Bread");
        assert_eq!(
            imported.entries,
            [
                ImportedEntry::User {
                    at: 100_000,
                    text: "How long to proof?".into()
                },
                ImportedEntry::Assistant {
                    at: 102_000,
                    text: "Two hours.".into()
                },
                ImportedEntry::User {
                    at: 103_000,
                    text: "And at 30 degrees?".into()
                },
            ]
        );
    }
}
