//! Tools added by hand: a program and its manifest in a folder of
//! its own.

use std::path::{Path, PathBuf};

use hamlet_rules::chief::rulings;
use hamlet_rules::hamlet::Hamlet;
use hamlet_rules::realm::Realm;
use hamlet_rules::vote;
use hamlet_sandbox::{Limits, Policy};
use serde_json::{Value, json};

use super::reading;

/// A program plus `manifest.json` in its own folder under `tools/`.
pub(super) struct CustomTool {
    pub(super) definition: Value,
    pub(super) name: String,
    pub(super) dir: PathBuf,
    pub(super) exec: Vec<String>,
    pub(super) policy: Policy,
    /// True when its manifest asks for the network.
    pub(super) net: bool,
}

impl CustomTool {
    /// Reads the tool in `dir`. `env` is what the sandbox gives
    /// every command.
    pub(super) fn load(
        dir: &Path,
        mut env: Vec<(String, String)>,
    ) -> anyhow::Result<CustomTool> {
        let m = serde_json::from_str::<Value>(
            &std::fs::read_to_string(dir.join("manifest.json"))?,
        )?;
        let name = m["name"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing name"))?
            .to_string();
        let exec = m["exec"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("exec must be an array"))?
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect::<Vec<_>>();
        anyhow::ensure!(!exec.is_empty(), "exec is empty");
        let sb = &m["sandbox"];
        let paths = |key: &str| {
            sb[key]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str())
                .map(|p| dir.join(p))
                .collect::<Vec<_>>()
        };
        let mut limits = Limits::default();
        if let Some(w) = sb["limits"]["wall_secs"].as_u64() {
            limits.wall_secs = w;
        }
        if let Some(c) = sb["limits"]["cpu_secs"].as_u64() {
            limits.cpu_secs = c;
        }
        for (k, v) in sb["env"].as_object().into_iter().flatten() {
            env.push((
                k.clone(),
                v.as_str().unwrap_or_default().to_string(),
            ));
        }
        let mut ro = paths("fs_ro");
        ro.push(dir.to_path_buf());
        let policy = Policy {
            ro,
            rw: paths("fs_rw"),
            // Decided when it is called.
            net: false,
            env,
            limits,
            ..Policy::default()
        };
        let definition = json!({"type": "function", "function": {
            "name": name,
            "description": m["description"].as_str().unwrap_or_default(),
            "parameters": m.get("parameters").cloned().unwrap_or(json!({"type": "object", "properties": {}})),
        }});
        Ok(CustomTool {
            definition,
            name,
            dir: dir.to_path_buf(),
            exec,
            policy,
            net: sb["net"].as_bool().unwrap_or(false),
        })
    }
}

/// True when `name` belongs to a built-in tool or one of the
/// hamlet's.
pub(super) fn reserved(name: &str) -> bool {
    let named = |defs: Vec<Value>| {
        defs.iter().any(|d| d["function"]["name"] == name)
    };
    ["bash", "powershell", "edit"].contains(&name)
        || reading::NAMES.contains(&name)
        || named(Hamlet::definitions())
        || named(Hamlet::resident_definitions())
        || named(Realm::definitions())
        || named(vec![vote::ballot_definition()])
        || [rulings::OBJECT, rulings::ASK_CHIEF].contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hamlets_tool_names_are_reserved() {
        for name in [
            "bash",
            "edit",
            "ask_agent",
            "tell_agent",
            "create_agent",
            "build_road",
            "request_road",
            "call_vote",
            "cast_ballot",
            "agree_road",
            "agree_move",
            "ask_removal",
            "object",
            "ask_chief",
            "submit_skill",
            "ask_librarian",
        ] {
            assert!(reserved(name), "{name}");
        }
        assert!(!reserved("calendar"));
    }
}
