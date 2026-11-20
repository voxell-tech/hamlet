//! Each society tool's arguments, whose doc comments a model reads.

use std::collections::BTreeMap;

use hamlet_protocol::runtime::{ToolSpec, ToolSpecReplay};
use schemars::generate::SchemaSettings;
use schemars::{JsonSchema, Schema};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::society::rejection::Rejection;
use crate::tools::catalogue::{FACTS, Society};

const ADDRESS: &str = "An agent's address: `hand`, `lead`, \
                       `lead@<hamlet id>`, `agent#<id>` or \
                       `worker#<id>`.";
const REASON: &str =
    "Why, in a sentence, if it helps; the record keeps it.";
const PATHS: &str = "Files or folders in your worktree or your \
                     hamlet's folder, copied into the receiver's \
                     inbox; the message says where. Links are not \
                     followed. Over 32 MiB or 1000 files waits for \
                     a chief.";
const PROFILE: &str = "The profile's address, `profile#<id>`.";
const HAMLET: &str = "The hamlet's id.";

/// Send a notification to an agent you have a channel to. It starts
/// a turn of theirs; you do not wait, and expect no response.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct Notify {
    #[schemars(description = ADDRESS)]
    pub to: String,
    /// The notification.
    pub text: String,
    #[serde(default)]
    #[schemars(description = PATHS)]
    pub paths: Vec<String>,
}

/// Send a request to an agent you have a channel to. It returns at
/// once; the answer comes later as a response that starts a turn of
/// yours, or as an error response on your next turn. End your turn if
/// you have nothing else to do.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct Request {
    #[schemars(description = ADDRESS)]
    pub to: String,
    /// The request.
    pub text: String,
    #[serde(default)]
    #[schemars(description = PATHS)]
    pub paths: Vec<String>,
}

/// Start a worker on a task: a helper with your tools that works in a
/// scratch folder and where you work, then answers once. With `wait`,
/// you wait for its answer; else it comes later as a response.
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

/// Send the librarian a request for the skills in the library that
/// help with something.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct RequestLibrarian {
    /// What you need a skill for.
    pub text: String,
}

/// Read what you may spend: your hamlet's or the realm's spend this
/// period against its budget, your own, your allowance and your
/// task's cap. A call over any of them is rejected.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct ReadBudget {}

/// Object to the chief's instruction you are working on, with your
/// reason. The chief rules: go on, or withdraw it.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct Object {
    /// Why the instruction should not be done.
    pub reason: String,
}

/// Propose to your parent a channel to an agent of your hamlet you
/// need to reach.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct ProposeChannel {
    #[schemars(description = ADDRESS)]
    pub to: String,
    #[serde(default)]
    #[schemars(description = REASON)]
    pub reason: String,
}

/// Make a new profile of the city, to add an agent of next. Without
/// a character it starts from the realm's own. A profile you make may
/// wait at the harbor until it is let into the city; its agents work
/// all the same.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct CreateProfile {
    /// A short name, one line with no colon. It is a label and may
    /// repeat.
    pub name: String,
    /// Who the profile is and how it works.
    #[serde(default)]
    pub character: String,
}

/// Add an agent of a profile to your hamlet under you, or under an
/// agent of your tree. Leave out `profile` to add a blank agent
/// instead, which starts from the realm's character and the library's
/// skills and is not a profile of the city.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct AddAgent {
    /// The profile's address, `profile#<id>`, with `@<version>` to
    /// pin a version.
    #[serde(default)]
    pub profile: String,
    /// A blank agent's name, in place of `profile`.
    #[serde(default)]
    pub name: String,
    /// What the agent is for.
    pub purpose: String,
    /// The agent it answers to; you when left out.
    #[serde(default)]
    pub under: String,
    /// The hamlet, for the Hand.
    #[serde(default)]
    pub hamlet: String,
}

/// Make a profile of the city from what an agent under you learned: a
/// blank agent's own profile joins it; an agent of a city profile
/// starts a new profile with its memory and skills, and stays where
/// it is. It may wait at the harbor until let in.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct MakeProfile {
    #[schemars(description = ADDRESS)]
    pub agent: String,
    /// The profile's name; the agent's when left out.
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    #[schemars(description = REASON)]
    pub reason: String,
}

/// End an agent under you; its worktree and branch are archived. With
/// `dream`, it reflects and dreams once more to bring what it learned
/// home to its profile, and you are notified of what it brought.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct EndAgent {
    #[schemars(description = ADDRESS)]
    pub agent: String,
    #[serde(default)]
    pub dream: bool,
    #[serde(default)]
    #[schemars(description = REASON)]
    pub reason: String,
}

/// Merge one agent under you into another: the kept agent takes what
/// the other learned, its channels and its agents.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct MergeAgents {
    #[schemars(description = ADDRESS)]
    pub absorbed: String,
    #[schemars(description = ADDRESS)]
    pub into: String,
    #[serde(default)]
    #[schemars(description = REASON)]
    pub reason: String,
}

/// Choose the model an agent under you runs; leave out `model` for
/// its hamlet's.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct SetModel {
    #[schemars(description = ADDRESS)]
    pub agent: String,
    /// The model, as `provider::id`.
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    #[schemars(description = REASON)]
    pub reason: String,
}

/// Turn tools off or back on for an agent under you; `mcp__<server>`
/// names every tool of an MCP server.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct SetTools {
    #[schemars(description = ADDRESS)]
    pub agent: String,
    #[serde(default)]
    pub off: Vec<String>,
    #[serde(default)]
    pub on: Vec<String>,
    #[serde(default)]
    #[schemars(description = REASON)]
    pub reason: String,
}

/// Give an agent under you an allowance in dollars for the budget's
/// period, or none.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct SetAllowance {
    #[schemars(description = ADDRESS)]
    pub agent: String,
    #[serde(default)]
    pub dollars: Option<f64>,
    #[serde(default)]
    #[schemars(description = REASON)]
    pub reason: String,
}

/// Give yourself, or an agent under you, a short name shown in the
/// world; leave out `name` for the profile's name.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct RenameAgent {
    #[schemars(description = ADDRESS)]
    pub agent: String,
    /// At most 24 letters.
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    #[schemars(description = REASON)]
    pub reason: String,
}

/// Open a channel between two agents under you, so they can send
/// each other notifications and requests.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct OpenChannel {
    #[schemars(description = ADDRESS)]
    pub a: String,
    #[schemars(description = ADDRESS)]
    pub b: String,
    #[serde(default)]
    #[schemars(description = REASON)]
    pub reason: String,
}

/// Close a channel you opened between two agents under you.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct CloseChannel {
    #[schemars(description = ADDRESS)]
    pub a: String,
    #[schemars(description = ADDRESS)]
    pub b: String,
    #[serde(default)]
    #[schemars(description = REASON)]
    pub reason: String,
}

/// Move an agent, and the agents under it, under another agent of
/// the same hamlet.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct ReparentAgent {
    #[schemars(description = ADDRESS)]
    pub agent: String,
    #[schemars(description = ADDRESS)]
    pub under: String,
    #[serde(default)]
    #[schemars(description = REASON)]
    pub reason: String,
}

/// As a lead, agree a channel between an agent of your hamlet and
/// one of another. It opens once both leads agree.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct AgreeChannel {
    #[schemars(description = ADDRESS)]
    pub mine: String,
    #[schemars(description = ADDRESS)]
    pub theirs: String,
    #[serde(default)]
    #[schemars(description = REASON)]
    pub reason: String,
}

/// Found a hamlet with a profile as its lead, or a blank agent when
/// `profile` is left out.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct FoundHamlet {
    /// The name it is shown by: one line, up to 40 characters, no
    /// colon. Its id is made for it.
    pub name: String,
    /// The address, `profile#<id>`, of the profile its lead is an
    /// agent of.
    #[serde(default)]
    pub profile: String,
    /// What the hamlet is for.
    pub purpose: String,
}

/// Make an agent its hamlet's lead.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct SetLead {
    #[schemars(description = ADDRESS)]
    pub agent: String,
    #[serde(default)]
    #[schemars(description = REASON)]
    pub reason: String,
}

/// Remove a hamlet and end its agents; a chief approves it first.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct RemoveHamlet {
    #[schemars(description = HAMLET)]
    pub hamlet: String,
    #[serde(default)]
    #[schemars(description = REASON)]
    pub reason: String,
}

/// Bring a removed hamlet back.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct RestoreHamlet {
    #[schemars(description = HAMLET)]
    pub hamlet: String,
    #[serde(default)]
    #[schemars(description = REASON)]
    pub reason: String,
}

/// Erase a removed hamlet for good; a chief approves it first.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct EraseHamlet {
    #[schemars(description = HAMLET)]
    pub hamlet: String,
}

/// Give a hamlet another name to be shown by.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct NameHamlet {
    #[schemars(description = HAMLET)]
    pub hamlet: String,
    /// One line, up to 40 characters, no colon.
    pub name: String,
}

/// Set a hamlet down elsewhere on the hamlet island.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct MoveHamlet {
    #[schemars(description = HAMLET)]
    pub hamlet: String,
    pub x: f64,
    pub z: f64,
}

/// Set how many agents a hamlet holds; a chief approves it first.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct SetTier {
    #[schemars(description = HAMLET)]
    pub hamlet: String,
    #[schemars(extend("enum" = [50, 100, 200, 500]))]
    pub tier: u32,
}

/// Choose the folder a hamlet works on, or none; a chief approves it
/// first.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct SetFolder {
    #[schemars(description = HAMLET)]
    pub hamlet: String,
    /// A folder on this machine; none when left out.
    #[serde(default)]
    pub folder: String,
}

/// Choose whether a hamlet's edits and commands in its folder wait
/// for a chief (`manual`) or not (`auto`); a chief approves it first.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct SetMode {
    #[schemars(description = HAMLET)]
    pub hamlet: String,
    #[schemars(extend("enum" = ["manual", "auto"]))]
    pub mode: String,
}

/// Start a hamlet's budget period again, or the realm's; a chief
/// approves it first.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct ResetBudget {
    /// The hamlet's id; the realm's budget when left out.
    #[serde(default)]
    pub hamlet: String,
}

/// Set one limit of a hamlet, or of the realm, by its section and key
/// in the Settings; a chief approves it first.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct EditLimits {
    /// The hamlet's id; the realm's limits when left out.
    #[serde(default)]
    pub hamlet: String,
    /// The section, such as `budget`, `channel`, `reserved` or
    /// `asking`; empty for a key outside every section.
    #[serde(default)]
    pub section: String,
    /// The limit, such as `cost` of `budget`.
    pub key: String,
    /// Its new value.
    pub value: Value,
}

/// Read a hamlet's record: every decision with its reason.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct ReadRecord {
    #[schemars(description = HAMLET)]
    pub hamlet: String,
}

/// List a profile's files, or read one, at its `main` or a version.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct ReadProfile {
    #[schemars(description = PROFILE)]
    pub profile: String,
    /// A file to read.
    #[serde(default)]
    pub path: String,
    /// A version or commit.
    #[serde(default)]
    pub version: String,
}

/// Write a file of a profile's `main`; a chief approves it first.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct EditProfile {
    #[schemars(description = PROFILE)]
    pub profile: String,
    /// The file.
    pub path: String,
    /// Its new text; empty removes it.
    pub content: String,
    /// What the change does.
    pub message: String,
}

/// Name the profile's `main` as a version; a chief approves it first.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct Tag {
    #[schemars(description = PROFILE)]
    pub profile: String,
    /// The version's name.
    pub tag: String,
    /// What it holds.
    #[serde(default)]
    pub note: String,
}

/// Undo a commit of a profile's `main`; a chief approves it first.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct Revert {
    #[schemars(description = PROFILE)]
    pub profile: String,
    /// The commit or version to undo.
    pub commit: String,
    #[serde(default)]
    #[schemars(description = REASON)]
    pub reason: String,
}

/// List a profile's versions and the latest commits of its `main`.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct Versions {
    #[schemars(description = PROFILE)]
    pub profile: String,
}

/// Make a new profile from a version of another.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct Fork {
    #[schemars(description = PROFILE)]
    pub profile: String,
    /// A version or commit; `main` when left out.
    #[serde(default)]
    pub version: String,
    /// The new profile's name.
    pub name: String,
}

/// Take a file or folder out of a profile's whole history and its
/// agents' worktrees; a chief approves it first.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct Purge {
    #[schemars(description = PROFILE)]
    pub profile: String,
    /// The path in the profile.
    pub path: String,
}

/// Let a profile waiting at the harbor into the city.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct Welcome {
    #[schemars(description = PROFILE)]
    pub profile: String,
    #[serde(default)]
    #[schemars(description = REASON)]
    pub reason: String,
}

/// Keep a profile at the harbor, or send one back there from the
/// city. Its agents work all the same.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct TurnAway {
    #[schemars(description = PROFILE)]
    pub profile: String,
    #[serde(default)]
    #[schemars(description = REASON)]
    pub reason: String,
}

/// Give a profile a new name.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct NameProfile {
    #[schemars(description = PROFILE)]
    pub profile: String,
    /// One line, up to 40 characters, no colon.
    pub name: String,
}

/// Set a profile's house down elsewhere in the city.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct MoveHouse {
    #[schemars(description = PROFILE)]
    pub profile: String,
    pub x: f64,
    pub z: f64,
    /// Which way its door faces.
    #[serde(default)]
    pub degrees: Option<f64>,
}

/// Archive a profile and end its agents; a chief approves it first.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct RemoveProfile {
    #[schemars(description = PROFILE)]
    pub profile: String,
    #[serde(default)]
    #[schemars(description = REASON)]
    pub reason: String,
}

/// Bring an archived profile back with its agents.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct RestoreProfile {
    #[schemars(description = PROFILE)]
    pub profile: String,
    #[serde(default)]
    #[schemars(description = REASON)]
    pub reason: String,
}

/// Erase an archived profile for good; a chief approves it first.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct EraseProfile {
    #[schemars(description = PROFILE)]
    pub profile: String,
}

/// Hold an agent at a version of its profile, or let it follow `main`
/// again.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct Pin {
    #[schemars(description = ADDRESS)]
    pub agent: String,
    /// The version, or `main`.
    pub version: String,
    #[serde(default)]
    #[schemars(description = REASON)]
    pub reason: String,
}

/// Bring an ended agent back, or split a merged one out again.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct RestoreAgent {
    #[schemars(description = ADDRESS)]
    pub agent: String,
    #[serde(default)]
    #[schemars(description = REASON)]
    pub reason: String,
}

/// Erase an ended agent for good; a chief approves it first.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct EraseAgent {
    #[schemars(description = ADDRESS)]
    pub agent: String,
}

/// Choose how hard an agent's model thinks; `none` for the realm's
/// setting.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct SetEffort {
    #[schemars(description = ADDRESS)]
    pub agent: String,
    #[schemars(extend("enum" = [
        "off", "minimal", "low", "medium", "high", "xhigh", "max", "none"
    ]))]
    pub effort: String,
}

/// Turn an agent's fast tier on or off.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct SetFast {
    #[schemars(description = ADDRESS)]
    pub agent: String,
    pub fast: bool,
}

/// Have an agent bring what it learned home to its profile, or a
/// profile's agents do so and the profile tidy its `main`, even with
/// dreams off.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct Dream {
    /// An agent's address, or a profile's, `profile#<id>`.
    pub who: String,
}

/// Have an agent keep what its conversation taught it now.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct Reflect {
    #[schemars(description = ADDRESS)]
    pub agent: String,
}

/// Stop an agent's running work, or every agent's but yours with
/// `all`. A stopped agent takes nothing it is sent until a chief's
/// line or `resume`.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct Stop {
    /// An agent's address, or `all`.
    pub agent: String,
}

/// Let a stopped agent take what waits for it again, or every
/// stopped agent but you with `all`.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct Resume {
    /// An agent's address, or `all`.
    pub agent: String,
}

/// Write a memory file of an agent's worktree; empty content removes
/// it.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct EditMemory {
    #[schemars(description = ADDRESS)]
    pub agent: String,
    /// The path under `memory/`.
    pub path: String,
    /// Its new text.
    pub content: String,
}

/// Read the prompt of an agent's conversation.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct ReadPrompt {
    #[schemars(description = ADDRESS)]
    pub agent: String,
}

/// Read what an agent knows in its worktree: its character, memory
/// and skills, and what it learned that its profile's `main` lacks.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct ReadPapers {
    #[schemars(description = ADDRESS)]
    pub agent: String,
}

/// Change the realm's settings; what is left out stays. A chief
/// approves it first.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct EditSettings {
    /// The realm's model, as `provider::id`.
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    #[schemars(extend("enum" = [
        "off", "minimal", "low", "medium", "high", "xhigh", "max", "none"
    ]))]
    pub effort: Option<String>,
    /// Turns of a chat before its agent reflects; 0 never.
    #[serde(default)]
    pub reflect_every: Option<u64>,
    /// Who lets a newcomer into the city.
    #[serde(default)]
    #[schemars(extend("enum" = ["open", "chief", "hand"]))]
    pub newcomers: Option<String>,
    /// The character a blank profile starts from.
    #[serde(default)]
    pub character: Option<String>,
}

/// Replace the realm's charter, which every agent's prompt begins
/// with; empty for the default. A chief approves it first.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct EditCharter {
    /// The whole charter.
    pub text: String,
}

/// Read the realm's charter.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct ReadCharter {}

/// List the models agents can run, with what each takes and costs.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct ReadModels {}

/// Take a skill off the library's shelves.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct Unshelve {
    /// The skill's name.
    pub skill: String,
    #[serde(default)]
    #[schemars(description = REASON)]
    pub reason: String,
}

/// [`Called`] and each tool's schema, from the argument types.
macro_rules! called {
    ($($tool:ident),* $(,)?) => {
        /// A call of a society tool, with its arguments read.
        pub(crate) enum Called {
            $($tool($tool),)*
        }

        impl Called {
            pub fn read(
                tool: Society,
                arguments: &BTreeMap<String, Value>,
            ) -> Result<Self, Rejection> {
                let given = given(arguments);
                match tool {
                    $(Society::$tool => {
                        Ok(Self::$tool(parse(tool, given)?))
                    })*
                    // `request_chief` reads its own arguments.
                    Society::RequestChief => {
                        Err(Rejection::Unsupported {
                            what: format!("the tool {}", tool.name()),
                        })
                    }
                }
            }
        }

        fn schema(tool: Society) -> Option<Schema> {
            let generator = SchemaSettings::draft07()
                .with(|settings| settings.meta_schema = None)
                .into_generator();
            match tool {
                $(Society::$tool => {
                    Some(generator.into_root_schema_for::<$tool>())
                })*
                Society::RequestChief => None,
            }
        }
    };
}

called!(
    Notify,
    Request,
    StartWorker,
    SubmitSkill,
    RequestLibrarian,
    ReadBudget,
    Object,
    ProposeChannel,
    CreateProfile,
    MakeProfile,
    AddAgent,
    EndAgent,
    MergeAgents,
    SetModel,
    SetTools,
    SetAllowance,
    RenameAgent,
    OpenChannel,
    CloseChannel,
    ReparentAgent,
    AgreeChannel,
    FoundHamlet,
    SetLead,
    RemoveHamlet,
    RestoreHamlet,
    EraseHamlet,
    NameHamlet,
    MoveHamlet,
    SetTier,
    SetFolder,
    SetMode,
    ResetBudget,
    EditLimits,
    ReadRecord,
    ReadProfile,
    EditProfile,
    Tag,
    Revert,
    Versions,
    Fork,
    Purge,
    Welcome,
    TurnAway,
    NameProfile,
    MoveHouse,
    RemoveProfile,
    RestoreProfile,
    EraseProfile,
    Pin,
    RestoreAgent,
    EraseAgent,
    SetEffort,
    SetFast,
    Dream,
    Reflect,
    Stop,
    Resume,
    EditMemory,
    ReadPrompt,
    ReadPapers,
    EditSettings,
    EditCharter,
    ReadCharter,
    ReadModels,
    Unshelve,
);

/// The arguments kept untrimmed.
const VERBATIM: [&str; 1] = ["content"];

/// `arguments` trimmed, without nulls.
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
) -> Result<A, Rejection> {
    serde_json::from_value(given).map_err(|error| {
        Rejection::ArgumentsUnfit {
            tool: tool.name().into(),
            error: error.to_string(),
        }
    })
}

/// Every society tool but `request_chief`, as a model is offered it.
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

/// The tool's description and parameters from `schema`.
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
    // Some providers want `properties` even when empty.
    map.entry("properties")
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    (description, map.into_iter().collect())
}

/// Strips formats, defaults and nullable types from `schema` and
/// unwraps its descriptions.
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

fn unwrapped(text: &str) -> String {
    text.split("\n\n")
        .map(|paragraph| {
            paragraph.split('\n').collect::<Vec<_>>().join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}
