//! The arguments of each society tool, read once when its call runs.
//! Each type is both what a call is read into and the JSON Schema a
//! model is offered: its doc comment is the tool's description, and
//! each field's is the argument's.

use std::collections::BTreeMap;

use hamlet_protocol::runtime::{ToolSpec, ToolSpecReplay};
use schemars::generate::SchemaSettings;
use schemars::{JsonSchema, Schema};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::society::refusal::Refusal;
use crate::tools::catalogue::{FACTS, Society};

const ADDRESS: &str = "An agent's address: `hand`, `lead`, \
                       `lead@<hamlet id>`, `hire#<id>` or \
                       `worker#<id>`.";
const REASON: &str =
    "Why, in a sentence, if it helps; the record keeps it.";
const PATHS: &str =
    "Files or folders to send with it, in your lodge or your \
     hamlet's folder. They are copied into the receiver's inbox, \
     and the message says where. Links are not followed. A chief \
     approves a send of more than 32 MiB or 1000 files.";
const SCHOLAR: &str = "The scholar's address, `scholar#<id>`.";
const HAMLET: &str = "The hamlet's id.";

/// Send a letter to an agent you have a road to. It is queued for
/// them; you do not wait for an answer.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct Tell {
    #[schemars(description = ADDRESS)]
    pub to: String,
    /// The letter.
    pub text: String,
    #[serde(default)]
    #[schemars(description = PATHS)]
    pub paths: Vec<String>,
}

/// Put a question to an agent you have a road to and wait for its
/// answer. Put only what you need; a question to someone waiting on
/// you is refused.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct Ask {
    #[schemars(description = ADDRESS)]
    pub to: String,
    /// The question.
    pub question: String,
    #[serde(default)]
    #[schemars(description = PATHS)]
    pub paths: Vec<String>,
}

/// Start a worker on a task: a helper with your tools that works in a
/// scratch folder and where you work, then answers once. With `wait`,
/// you wait for its answer; without, the answer comes to you as a
/// reply.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct StartWorker {
    /// What the worker is to do, and what to answer.
    pub task: String,
    /// Your built-in tools it may use; all of them when left out.
    #[serde(default)]
    pub tools: Vec<String>,
    /// Wait for its answer.
    #[serde(default)]
    pub wait: bool,
}

/// Offer a skill from your skills/ folder to the library, where every
/// agent can read it. The librarian judges it.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct SubmitSkill {
    /// The skill's folder name under skills/.
    pub skill: String,
    #[serde(default)]
    #[schemars(description = REASON)]
    pub reason: String,
}

/// Put a question to the librarian about which skills in the library
/// help with something.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct AskLibrarian {
    /// What you need a skill for.
    pub question: String,
}

/// Read what you may spend: your hamlet's or the realm's spend this
/// period against its budget, your own, your allowance and your
/// task's cap. A call over any of them is refused.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct ReadBudget {}

/// Object to the chief's instruction you are working on, with your
/// reason. The chief rules: go on, or withdraw it.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct Object {
    /// Why the instruction should not be done.
    pub reason: String,
}

/// Request from your parent a road to an agent of your hamlet you
/// need to reach.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct RequestRoad {
    #[schemars(description = ADDRESS)]
    pub to: String,
    #[serde(default)]
    #[schemars(description = REASON)]
    pub reason: String,
}

/// Make a new scholar of the city, to hire next. Without a character
/// it starts from the realm's own. A scholar you make may wait at the
/// harbor until it is let into the city; its hires work all the same.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct CreateScholar {
    /// A short name, one line with no colon. It is a label and may
    /// repeat.
    pub name: String,
    /// Who the scholar is and how it works.
    #[serde(default)]
    pub character: String,
}

/// Hire a scholar into your hamlet under you, or under a hire of your
/// tree. Leave out `scholar` to hire a blank agent instead, which
/// starts from the realm's character and the library's skills and is
/// not a scholar of the city.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct Hire {
    /// The scholar's address, `scholar#<id>`, with `@<version>` to
    /// pin a version.
    #[serde(default)]
    pub scholar: String,
    /// A blank agent's name, in place of `scholar`.
    #[serde(default)]
    pub name: String,
    /// What the hire is for.
    pub purpose: String,
    /// The hire it answers to; you when left out.
    #[serde(default)]
    pub under: String,
    /// The hamlet, for the Hand.
    #[serde(default)]
    pub hamlet: String,
}

/// End a hire under you. Its work stays in its scholar.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct EndHire {
    #[schemars(description = ADDRESS)]
    pub hire: String,
    #[serde(default)]
    #[schemars(description = REASON)]
    pub reason: String,
}

/// Merge one hire under you into another: the kept hire takes what
/// the other learned, its roads and its hires.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct MergeHires {
    #[schemars(description = ADDRESS)]
    pub absorbed: String,
    #[schemars(description = ADDRESS)]
    pub into: String,
    #[serde(default)]
    #[schemars(description = REASON)]
    pub reason: String,
}

/// Choose the model a hire under you runs; leave out `model` for its
/// hamlet's.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct SetModel {
    #[schemars(description = ADDRESS)]
    pub hire: String,
    /// The model, as `provider::id`.
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    #[schemars(description = REASON)]
    pub reason: String,
}

/// Turn built-in tools off or back on for a hire under you.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct SetTools {
    #[schemars(description = ADDRESS)]
    pub hire: String,
    #[serde(default)]
    pub off: Vec<String>,
    #[serde(default)]
    pub on: Vec<String>,
    #[serde(default)]
    #[schemars(description = REASON)]
    pub reason: String,
}

/// Give a hire under you an allowance in dollars for the budget's
/// period, or none.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct SetAllowance {
    #[schemars(description = ADDRESS)]
    pub hire: String,
    #[serde(default)]
    pub dollars: Option<f64>,
    #[serde(default)]
    #[schemars(description = REASON)]
    pub reason: String,
}

/// Give a hire under you a short nickname it is shown by in the
/// world; leave out `nickname` for its own name.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct Nickname {
    #[schemars(description = ADDRESS)]
    pub hire: String,
    /// At most 24 letters.
    #[serde(default)]
    pub nickname: String,
    #[serde(default)]
    #[schemars(description = REASON)]
    pub reason: String,
}

/// Lay a road between two hires under you, so they can write and put
/// questions to each other.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct LayRoad {
    #[schemars(description = ADDRESS)]
    pub a: String,
    #[schemars(description = ADDRESS)]
    pub b: String,
    #[serde(default)]
    #[schemars(description = REASON)]
    pub reason: String,
}

/// Take up a road you laid between two hires under you.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct TakeUpRoad {
    #[schemars(description = ADDRESS)]
    pub a: String,
    #[schemars(description = ADDRESS)]
    pub b: String,
    #[serde(default)]
    #[schemars(description = REASON)]
    pub reason: String,
}

/// As a lead, agree a road between a hire of your hamlet and one of
/// another. It opens once both leads agree.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct AgreeRoad {
    #[schemars(description = ADDRESS)]
    pub mine: String,
    #[schemars(description = ADDRESS)]
    pub theirs: String,
    #[serde(default)]
    #[schemars(description = REASON)]
    pub reason: String,
}

/// Found a hamlet with a scholar as its lead, or a blank agent when
/// `scholar` is left out.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct FoundHamlet {
    /// The name it is shown by: one line, up to 40 characters, no
    /// colon. Its id is made for it.
    pub name: String,
    /// The address, `scholar#<id>`, of the scholar its lead is a
    /// hire of.
    #[serde(default)]
    pub scholar: String,
    /// What the hamlet is for.
    pub purpose: String,
}

/// Make a hire its hamlet's lead.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct SetLead {
    #[schemars(description = ADDRESS)]
    pub hire: String,
    #[serde(default)]
    #[schemars(description = REASON)]
    pub reason: String,
}

/// Remove a hamlet; a chief approves it first.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct RemoveHamlet {
    #[schemars(description = HAMLET)]
    pub hamlet: String,
    #[serde(default)]
    #[schemars(description = REASON)]
    pub reason: String,
}

/// Read a hamlet's record: every decision with its reason.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct ReadRecord {
    #[schemars(description = HAMLET)]
    pub hamlet: String,
}

/// List a scholar's files, or read one, at its `main` or a version.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct ReadScholar {
    #[schemars(description = SCHOLAR)]
    pub scholar: String,
    /// A file to read.
    #[serde(default)]
    pub path: String,
    /// A version or commit.
    #[serde(default)]
    pub version: String,
}

/// Write a file of a scholar's `main`; a chief approves it first.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct EditScholar {
    #[schemars(description = SCHOLAR)]
    pub scholar: String,
    /// The file.
    pub path: String,
    /// Its new text; empty removes it.
    pub content: String,
    /// What the change does.
    pub message: String,
}

/// Name the scholar's `main` as a version; a chief approves it first.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct Tag {
    #[schemars(description = SCHOLAR)]
    pub scholar: String,
    /// The version's name.
    pub tag: String,
    /// What it holds.
    #[serde(default)]
    pub note: String,
}

/// Undo a commit of a scholar's `main`; a chief approves it first.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct Revert {
    #[schemars(description = SCHOLAR)]
    pub scholar: String,
    /// The commit or version to undo.
    pub commit: String,
    #[serde(default)]
    #[schemars(description = REASON)]
    pub reason: String,
}

/// Says each society tool's arguments once: the calls they are read
/// into, and the schemas a model is offered. `ask_chief` reads its
/// own questions.
macro_rules! called {
    ($($tool:ident),* $(,)?) => {
        /// A call of a society tool, with its arguments read.
        pub(crate) enum Called {
            $($tool($tool),)*
        }

        impl Called {
            /// Reads the call of `tool` with `arguments`, refusing
            /// arguments that do not fit it.
            pub fn read(
                tool: Society,
                arguments: &BTreeMap<String, Value>,
            ) -> Result<Self, Refusal> {
                let given = given(arguments);
                match tool {
                    $(Society::$tool => {
                        Ok(Self::$tool(parse(tool, given)?))
                    })*
                    Society::AskChief => Err(Refusal::Unsupported {
                        what: format!("the tool {}", tool.name()),
                    }),
                }
            }
        }

        /// The schema of `tool`'s arguments; none for `ask_chief`.
        fn schema(tool: Society) -> Option<Schema> {
            let generator = SchemaSettings::draft07()
                .with(|settings| settings.meta_schema = None)
                .into_generator();
            match tool {
                $(Society::$tool => {
                    Some(generator.into_root_schema_for::<$tool>())
                })*
                Society::AskChief => None,
            }
        }
    };
}

called!(
    Tell,
    Ask,
    StartWorker,
    SubmitSkill,
    AskLibrarian,
    ReadBudget,
    Object,
    RequestRoad,
    CreateScholar,
    Hire,
    EndHire,
    MergeHires,
    SetModel,
    SetTools,
    SetAllowance,
    Nickname,
    LayRoad,
    TakeUpRoad,
    AgreeRoad,
    FoundHamlet,
    SetLead,
    RemoveHamlet,
    ReadRecord,
    ReadScholar,
    EditScholar,
    Tag,
    Revert,
);

/// The arguments kept as written, untrimmed: a file's content.
const VERBATIM: [&str; 1] = ["content"];

/// The arguments as given, read leniently as a model writes them:
/// each text trimmed but those [`VERBATIM`], and a null left out as
/// if not given.
fn given(arguments: &BTreeMap<String, Value>) -> Value {
    Value::Object(
        arguments
            .iter()
            .filter(|(_, value)| !value.is_null())
            .map(|(key, value)| {
                let value = match value {
                    Value::String(text)
                        if !VERBATIM.contains(&key.as_str()) =>
                    {
                        Value::String(text.trim().to_owned())
                    }
                    other => other.clone(),
                };
                (key.clone(), value)
            })
            .collect(),
    )
}

fn parse<A: DeserializeOwned>(
    tool: Society,
    given: Value,
) -> Result<A, Refusal> {
    serde_json::from_value(given).map_err(|error| {
        Refusal::ArgumentsUnfit {
            tool: tool.name().into(),
            error: error.to_string(),
        }
    })
}

/// Every society tool but `ask_chief`, as a model is offered it.
pub(crate) fn specs() -> Vec<ToolSpec> {
    FACTS
        .iter()
        .filter_map(|facts| {
            let schema = schema(facts.tool)?;
            let (description, parameters) = offered(schema);
            Some(ToolSpec {
                name: facts.name.into(),
                description,
                parameters,
                replay: if facts.replays {
                    ToolSpecReplay::Safe
                } else {
                    ToolSpecReplay::Unsafe
                },
                retain: None,
            })
        })
        .collect()
}

/// A schema as a model is offered it: its description apart, as the
/// tool's, and only what the arguments' shape needs, with each
/// description's wrapped lines joined.
fn offered(schema: Schema) -> (String, BTreeMap<String, Value>) {
    let mut value = schema.to_value();
    plain(&mut value);
    let Value::Object(mut map) = value else {
        return (String::new(), BTreeMap::new());
    };
    let description = match map.remove("description") {
        Some(Value::String(text)) => text,
        _ => String::new(),
    };
    map.remove("title");
    // Some providers want an object's properties even when it has
    // none.
    map.entry("properties")
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    (description, map.into_iter().collect())
}

/// Leaves out of `schema` what no model needs, the formats, the
/// defaults and the null an argument left out may be, and joins each
/// description's wrapped lines; then so for each schema inside it.
fn plain(schema: &mut Value) {
    let Value::Object(map) = schema else {
        return;
    };
    map.remove("format");
    map.remove("default");
    if let Some(Value::Array(types)) = map.get("type")
        && let [kind] = types
            .iter()
            .filter(|kind| kind.as_str() != Some("null"))
            .collect::<Vec<_>>()[..]
    {
        let kind = kind.clone();
        map.insert("type".into(), kind);
    }
    if let Some(Value::String(text)) = map.get_mut("description") {
        *text = unwrapped(text);
    }
    if let Some(Value::Object(properties)) = map.get_mut("properties")
    {
        properties.values_mut().for_each(plain);
    }
    if let Some(items) = map.get_mut("items") {
        plain(items);
    }
}

/// `text` with the line breaks inside each paragraph made spaces.
fn unwrapped(text: &str) -> String {
    text.split("\n\n")
        .map(|paragraph| {
            paragraph.split('\n').collect::<Vec<_>>().join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}
