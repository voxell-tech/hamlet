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

const ADDRESS: &str = "An agent's address: `hand`, `mayor`, \
                       `mayor@<hamlet id>`, `hire#<id>` or \
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

/// Send a notification to an agent you have a road to. It goes into
/// their conversation as a turn of its own; you do not wait, and
/// expect no response. `to` takes an address, with or without the
/// name beside it.
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

/// Send a request to an agent you have a road to. It goes into their
/// conversation as a turn of its own and this call returns at once:
/// their final answer comes back to you later as the response, which
/// starts a turn of yours. A request that fails comes back as an
/// error response you read on your next turn, without starting one.
/// End your turn if you have nothing else to do meanwhile. Request
/// only what you need.
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
/// you wait for its answer; without, the answer comes to you later as
/// a response.
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

/// Ask the librarian which skills in the library help with something.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct RequestLibrarian {
    /// What you need a skill for.
    pub text: String,
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

/// Propose to your lead a road to an agent of your hamlet you need to
/// reach.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct ProposeRoad {
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

/// Make a scholar of the city of what a hire under you learned, to
/// hire again. A blank hire's own scholar joins the city with that
/// learning; a hire of a city scholar starts a new scholar from its
/// memory and skills, with its scholar's character, while it stays
/// that scholar's hire. A scholar you make may wait at the harbor
/// until it is let into the city.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct MakeScholar {
    #[schemars(description = ADDRESS)]
    pub hire: String,
    /// The scholar's name; the hire's when left out.
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    #[schemars(description = REASON)]
    pub reason: String,
}

/// End a hire under you. Its lodge and branch are archived with what
/// it learned. With `dream`, it dreams once more to bring that home
/// to its scholar, so later hires of the scholar start with it;
/// without, its scholar gets none of it.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct EndHire {
    #[schemars(description = ADDRESS)]
    pub hire: String,
    /// Dream once more to bring what it learned home to its scholar.
    #[serde(default)]
    pub dream: bool,
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

/// Lay a road between two hires under you, so they can send each
/// other notifications and requests.
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

/// As a mayor, agree a road between a hire of your hamlet and one of
/// another. It opens once both mayors agree.
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

/// Found a hamlet with a scholar as its mayor, or a blank agent when
/// `scholar` is left out.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct FoundHamlet {
    /// The name it is shown by: one line, up to 40 characters, no
    /// colon. Its id is made for it.
    pub name: String,
    /// The address, `scholar#<id>`, of the scholar its mayor is a
    /// hire of.
    #[serde(default)]
    pub scholar: String,
    /// What the hamlet is for.
    pub purpose: String,
}

/// Seat a hire as its hamlet's mayor.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct SeatMayor {
    #[schemars(description = ADDRESS)]
    pub hire: String,
    #[serde(default)]
    #[schemars(description = REASON)]
    pub reason: String,
}

/// Remove a hamlet and end its hires; a chief approves it first.
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

/// Set how many hires a hamlet holds; a chief approves it first.
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

/// Choose whether a hamlet's edits and commands in its folder ask a
/// chief (`manual`) or not (`auto`); a chief approves it first.
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

/// Set one limit of a hamlet, or of the realm, as the Settings show
/// them: a key of a section, such as `cost` of `budget`; a chief
/// approves it first.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct EditLimits {
    /// The hamlet's id; the realm's limits when left out.
    #[serde(default)]
    pub hamlet: String,
    /// The section, such as `budget`, `road`, `reserved` or
    /// `asking`; empty for a key outside every section.
    #[serde(default)]
    pub section: String,
    /// The limit.
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

/// List a scholar's versions and the latest commits of its `main`.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct Versions {
    #[schemars(description = SCHOLAR)]
    pub scholar: String,
}

/// Make a new scholar from a version of another.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct Fork {
    #[schemars(description = SCHOLAR)]
    pub scholar: String,
    /// A version or commit; `main` when left out.
    #[serde(default)]
    pub version: String,
    /// The new scholar's name.
    pub name: String,
}

/// Take a file or folder out of a scholar's whole history and its
/// hires' lodges; a chief approves it first.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct Purge {
    #[schemars(description = SCHOLAR)]
    pub scholar: String,
    /// The path in the scholar.
    pub path: String,
}

/// Let a scholar waiting at the harbor into the city.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct Welcome {
    #[schemars(description = SCHOLAR)]
    pub scholar: String,
    #[serde(default)]
    #[schemars(description = REASON)]
    pub reason: String,
}

/// Keep a scholar at the harbor, or send one back there from the
/// city. Its hires work all the same.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct TurnAway {
    #[schemars(description = SCHOLAR)]
    pub scholar: String,
    #[serde(default)]
    #[schemars(description = REASON)]
    pub reason: String,
}

/// Give a scholar a new name.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct NameScholar {
    #[schemars(description = SCHOLAR)]
    pub scholar: String,
    /// One line, up to 40 characters, no colon.
    pub name: String,
}

/// Set a scholar's house down elsewhere in the city.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct MoveHouse {
    #[schemars(description = SCHOLAR)]
    pub scholar: String,
    pub x: f64,
    pub z: f64,
    /// Which way its door faces.
    #[serde(default)]
    pub degrees: Option<f64>,
}

/// Archive a scholar and end its hires; a chief approves it first.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct RemoveScholar {
    #[schemars(description = SCHOLAR)]
    pub scholar: String,
    #[serde(default)]
    #[schemars(description = REASON)]
    pub reason: String,
}

/// Bring an archived scholar back with its hires.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct RestoreScholar {
    #[schemars(description = SCHOLAR)]
    pub scholar: String,
    #[serde(default)]
    #[schemars(description = REASON)]
    pub reason: String,
}

/// Erase an archived scholar for good; a chief approves it first.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct EraseScholar {
    #[schemars(description = SCHOLAR)]
    pub scholar: String,
}

/// Hold a hire at a version of its scholar, or let it follow `main`
/// again.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct Pin {
    #[schemars(description = ADDRESS)]
    pub hire: String,
    /// The version, or `main`.
    pub version: String,
    #[serde(default)]
    #[schemars(description = REASON)]
    pub reason: String,
}

/// Bring an ended hire back, or split a merged one out again.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct RestoreHire {
    #[schemars(description = ADDRESS)]
    pub hire: String,
    #[serde(default)]
    #[schemars(description = REASON)]
    pub reason: String,
}

/// Erase an ended hire for good; a chief approves it first.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct EraseHire {
    #[schemars(description = ADDRESS)]
    pub hire: String,
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

/// Have a hire bring what it learned home to its scholar, or a
/// scholar's hires do so and the scholar tidy its `main`. It runs
/// whether dreams are on or not; one dreams at a time, and the
/// others wait their turn.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct Dream {
    /// A hire's address, or a scholar's, `scholar#<id>`.
    pub who: String,
}

/// Ask an agent to keep what its chat with the chief taught it.
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

/// Write a memory file of an agent's lodge; empty content removes it.
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

/// Read what an agent knows in its lodge: its character, memory and
/// skills, and what it learned that its scholar's `main` lacks.
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
    /// The character a blank scholar starts from.
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

/// Says each society tool's arguments once: the calls they are read
/// into, and the schemas a model is offered. `request_chief` reads
/// its own questions.
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
                    Society::RequestChief => {
                        Err(Refusal::Unsupported {
                            what: format!("the tool {}", tool.name()),
                        })
                    }
                }
            }
        }

        /// The schema of `tool`'s arguments; none for
        /// `request_chief`.
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
    ProposeRoad,
    CreateScholar,
    MakeScholar,
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
    SeatMayor,
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
    ReadScholar,
    EditScholar,
    Tag,
    Revert,
    Versions,
    Fork,
    Purge,
    Welcome,
    TurnAway,
    NameScholar,
    MoveHouse,
    RemoveScholar,
    RestoreScholar,
    EraseScholar,
    Pin,
    RestoreHire,
    EraseHire,
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
