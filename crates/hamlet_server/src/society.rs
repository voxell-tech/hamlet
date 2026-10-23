//! Agents together (H3, H27-H30, H35): the society tools each role is
//! offered, carried out in this server under the markers of their
//! calls, and the chief's deeds on the record.

mod agents;
pub(crate) mod errands;
pub(crate) mod leaving;
pub(crate) mod letters;
pub(crate) mod library;
pub(crate) mod merging;
mod objections;
pub(crate) mod reviews;
pub(crate) mod votes;
pub(crate) mod work;

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use hamlet_protocol::runtime::{
    ConfigureTools, Diagnostic, DiagnosticSeverity, ToolSpec,
    ToolSpecReplay,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub(crate) use self::agents::deed;
use crate::chats::records;
use crate::harness::{
    Answer, Binding, BindingId, Harnesses, InputMark,
};
use crate::operation::Request;
use crate::protocol::Refusable;
use crate::record::{Actor, AgentRef};
use crate::service::Service;
use crate::tools::Outcome;
use crate::{OperationId, Store};

/// A tool of the society, which this server answers itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SocietyTool {
    CreateAgent,
    BuildRoad,
    SetModel,
    RemoveAgent,
    MergeAgents,
    AgreeRoad,
    AgreeMove,
    AskRemoval,
    TellAgent,
    AskAgent,
    RequestRoad,
    CallVote,
    CastBallot,
    SubmitSkill,
    AskLibrarian,
    Object,
    AskChief,
}

/// The conversations a [`SocietyTool`] is offered in.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    /// The mayor's own.
    Mayor,
    /// The mayor's, while more than one hamlet lives.
    Realm,
    /// Every agent's own.
    Every,
    /// An agent's own besides the mayor's.
    Member,
    /// One a chief instructs.
    Instructed,
    /// A ballot errand's, which offers it alone.
    Ballot,
}

impl SocietyTool {
    const ALL: [Self; 17] = [
        Self::CreateAgent,
        Self::BuildRoad,
        Self::SetModel,
        Self::RemoveAgent,
        Self::MergeAgents,
        Self::AgreeRoad,
        Self::AgreeMove,
        Self::AskRemoval,
        Self::TellAgent,
        Self::AskAgent,
        Self::RequestRoad,
        Self::CallVote,
        Self::CastBallot,
        Self::SubmitSkill,
        Self::AskLibrarian,
        Self::Object,
        Self::AskChief,
    ];

    /// The name of every society tool.
    pub(crate) fn names() -> impl Iterator<Item = &'static str> {
        Self::ALL.into_iter().map(Self::name)
    }

    /// The society tool named `name`.
    pub(crate) fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tool| tool.name() == name)
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::CreateAgent => "create_agent",
            Self::BuildRoad => "build_road",
            Self::SetModel => "set_model",
            Self::RemoveAgent => "remove_agent",
            Self::MergeAgents => "merge_agents",
            Self::AgreeRoad => "agree_road",
            Self::AgreeMove => "agree_move",
            Self::AskRemoval => "ask_removal",
            Self::TellAgent => "tell_agent",
            Self::AskAgent => "ask_agent",
            Self::RequestRoad => "request_road",
            Self::CallVote => "call_vote",
            Self::CastBallot => "cast_ballot",
            Self::SubmitSkill => "submit_skill",
            Self::AskLibrarian => "ask_librarian",
            Self::Object => "object",
            Self::AskChief => "ask_chief",
        }
    }

    fn role(self) -> Role {
        match self {
            Self::CreateAgent
            | Self::BuildRoad
            | Self::SetModel
            | Self::RemoveAgent
            | Self::MergeAgents => Role::Mayor,
            Self::AgreeRoad | Self::AgreeMove | Self::AskRemoval => {
                Role::Realm
            }
            Self::TellAgent
            | Self::AskAgent
            | Self::CallVote
            | Self::SubmitSkill
            | Self::AskLibrarian
            | Self::AskChief => Role::Every,
            Self::RequestRoad => Role::Member,
            Self::Object => Role::Instructed,
            Self::CastBallot => Role::Ballot,
        }
    }

    /// The tool as runtimes are given it; `None` for those given by
    /// their owners.
    fn spec(self) -> Option<ToolSpec> {
        let string = || json!({"type": "string"});
        let strings =
            || json!({"type": "array", "items": {"type": "string"}});
        let (description, properties, required): (_, _, &[&str]) =
            match self {
                Self::BuildRoad | Self::AskChief => return None,
                Self::CreateAgent => (
                    "Create an agent with a purpose, roads, model \
                     and character.",
                    json!({"name": string(), "purpose": string(), "reason": string(), "roads": strings(), "model": string(), "character": string()}),
                    &["name", "purpose", "reason"],
                ),
                Self::SetModel => (
                    "Choose an allowed model for an agent, or empty \
                     for the hamlet's own.",
                    json!({"name": string(), "model": string(), "reason": string()}),
                    &["name", "model", "reason"],
                ),
                Self::RemoveAgent => (
                    "Remove an agent, archiving its house and \
                     roads. A reason is kept in the record.",
                    json!({"name": string(), "reason": string()}),
                    &["name", "reason"],
                ),
                Self::MergeAgents => (
                    "Merge two agents' houses and roads under the \
                     kept agent's new purpose. A reason is kept in \
                     the record.",
                    json!({"keep": string(), "absorb": string(), "purpose": string(), "reason": string()}),
                    &["keep", "absorb", "purpose", "reason"],
                ),
                Self::AgreeRoad => (
                    "Agree to a road between agents of different \
                     hamlets. Both mayors must agree.",
                    json!({"a": string(), "b": string(), "reason": string()}),
                    &["a", "b", "reason"],
                ),
                Self::AgreeMove => (
                    "Agree that an agent moves with its house. Only \
                     roads both mayors name are kept.",
                    json!({"agent": string(), "to": string(), "keep_roads": strings(), "reason": string()}),
                    &["agent", "to", "reason"],
                ),
                Self::AskRemoval => (
                    "Ask the chief to remove a hamlet. Nothing is \
                     removed until a chief confirms.",
                    json!({"hamlet": string(), "reason": string()}),
                    &["hamlet", "reason"],
                ),
                Self::TellAgent => (
                    "Send a letter along a road. Its reply comes as \
                     a new message.",
                    json!({"name": string(), "message": string()}),
                    &["name", "message"],
                ),
                Self::AskAgent => (
                    "Ask an agent along a road and wait for its \
                     answer.",
                    json!({"name": string(), "message": string()}),
                    &["name", "message"],
                ),
                Self::RequestRoad => (
                    "Ask the mayor to build a road to another agent.",
                    json!({"to": string(), "reason": string()}),
                    &["to", "reason"],
                ),
                Self::CallVote => (
                    "Call a vote to replace the mayor or found a \
                     hamlet. Every agent besides the mayor votes \
                     from its own house. Returns the count; the \
                     chief may overturn it.",
                    json!({"kind": string(), "reason": string(), "candidate": string(), "hamlet": string(), "mayor": string(), "purpose": string()}),
                    &["kind", "reason"],
                ),
                Self::CastBallot => (
                    "Cast your ballot in the vote you were asked \
                     about, with words copied from your own notes \
                     or history.",
                    json!({"choice": string(), "reason": string(), "evidence": string()}),
                    &["choice", "reason", "evidence"],
                ),
                Self::SubmitSkill => (
                    "Offer one of your own skills to the shared \
                     library. The librarian takes it or says why \
                     not. Once taken it moves out of your house; \
                     use the library's copy.",
                    json!({"skill": string(), "reason": string()}),
                    &["skill", "reason"],
                ),
                Self::AskLibrarian => (
                    "Ask which skill on the library's shelves does \
                     what you need, and the command that runs it. \
                     Ask before writing a skill of your own.",
                    json!({"question": string()}),
                    &["question"],
                ),
                Self::Object => (
                    "Tell the chief why the instruction you were \
                     just given is a mistake. The work waits for \
                     its reply. You may object once; after go \
                     ahead, do the work.",
                    json!({"reason": string()}),
                    &["reason"],
                ),
            };
        Some(ToolSpec {
            name: self.name().into(),
            description: description.into(),
            parameters: object(json!({
                "type": "object",
                "properties": properties,
                "required": required,
            })),
            replay: ToolSpecReplay::Safe,
        })
    }
}

/// The society tools runtimes are given; `build_road` and `ask_chief`
/// are given by their owners.
pub(crate) fn specs() -> Vec<ToolSpec> {
    SocietyTool::ALL
        .into_iter()
        .filter_map(SocietyTool::spec)
        .collect()
}

fn object(value: Value) -> BTreeMap<String, Value> {
    match value {
        Value::Object(map) => map.into_iter().collect(),
        _ => BTreeMap::new(),
    }
}

/// The tools the conversation of `binding` offers (H35): an errand
/// none but a ballot's one, and an agent's own conversation the
/// society's tools of its role now.
pub(crate) fn tools_of(
    service: &mut Service,
    binding: BindingId,
) -> Result<ConfigureTools> {
    let db = service.db();
    if let Some(kind) = errand_of(db, binding)? {
        return Ok(ConfigureTools::Only {
            only: SocietyTool::ALL
                .into_iter()
                .filter(|tool| {
                    tool.role() == Role::Ballot && kind == "ballot"
                })
                .map(|tool| tool.name().into())
                .collect(),
        });
    }
    let Some(kept) = records::kept(db, binding)? else {
        return Ok(ConfigureTools::Remove { remove: Vec::new() });
    };
    let place = kept.place;
    let mayor = service
        .home
        .store()
        .record(&place.agent.hamlet)?
        .is_some_and(|record| record.mayor == place.agent.number);
    let realm = crate::realm::live(service.db())?.len() > 1;
    let allowed = |tool: SocietyTool| match tool.role() {
        Role::Every => true,
        Role::Instructed => place.chief.is_some(),
        Role::Mayor => mayor,
        Role::Realm => mayor && realm,
        Role::Member => !mayor,
        Role::Ballot => false,
    };
    Ok(ConfigureTools::Remove {
        remove: SocietyTool::ALL
            .into_iter()
            .filter(|tool| !allowed(*tool))
            .map(|tool| tool.name().into())
            .collect(),
    })
}

/// Whether an errand's conversation of `binding` may not call
/// `tool`; the server's check of a call that came before its runtime
/// was told the errand's tools.
pub(crate) fn withheld(
    service: &mut Service,
    binding: BindingId,
    tool: &str,
) -> Result<bool> {
    let db = service.db();
    Ok(errand_of(db, binding)?.is_some_and(|kind| {
        !(kind == "ballot"
            && SocietyTool::named(tool)
                .is_some_and(|tool| tool.role() == Role::Ballot))
    }))
}

/// Tells the runtime of `binding` the tools it offers now.
pub(crate) async fn configure(
    service: &mut Service,
    harnesses: &mut Harnesses,
    binding: BindingId,
) -> Result<()> {
    let tools = tools_of(service, binding)?;
    harnesses
        .configure(service.home.store(), binding, &tools)
        .await
}

/// Tells every living agent's conversations the tools of its role
/// now, as an office or the realm changed.
pub(crate) async fn reconfigure(
    service: &mut Service,
    harnesses: &mut Harnesses,
) -> Result<()> {
    let db = service.db();
    let live = crate::realm::live(db)?;
    for binding in records::all(db)? {
        let db = service.db();
        let Some(kept) = records::kept(db, binding)? else {
            continue;
        };
        if !live.contains(&kept.place.agent.hamlet) {
            continue;
        }
        configure(service, harnesses, binding).await?;
    }
    Ok(())
}

/// The result of a call of a tool the conversation is not offered, as
/// pi-durable gives it.
pub(crate) fn unavailable(tool: &str) -> SocietyOutcome {
    SocietyOutcome::from(Outcome {
        text: String::new(),
        error: true,
        details: None,
        diagnostics: vec![Diagnostic {
            code: "tool_unavailable".into(),
            message: format!("Tool {tool} is not available"),
            severity: DiagnosticSeverity::Error,
        }],
    })
}

/// The kind of errand the conversation of `binding` is, if it is one.
pub(crate) fn errand_of(
    db: &Connection,
    binding: BindingId,
) -> Result<Option<String>> {
    Ok(db
        .query_row(
            "SELECT errand FROM society_binding WHERE binding = ?1",
            [binding.0],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten())
}

/// What a society tool call is given: who calls it, from which
/// conversation, in which turn.
pub(crate) struct Caller<'a> {
    pub binding: &'a Binding,
    pub place: records::Place,
    pub id: OperationId,
    /// The call's tool and arguments, as its marker keeps them.
    pub request: Request,
    pub input: Option<&'a str>,
    /// The H12 task of the turn that called it.
    pub task: Option<String>,
}

/// Carries out the society call `key` of `binding`, which the gate
/// let run; `None` while it waits.
pub(crate) async fn operation(
    service: &mut Service,
    harnesses: &mut Harnesses,
    binding: &Binding,
    key: &str,
    input: Option<&str>,
    tool: &str,
    arguments: &BTreeMap<String, Value>,
) -> Result<Answer> {
    let db = service.db();
    let place = match records::kept(db, binding.id)? {
        Some(kept) => kept.place,
        None => records::Place {
            agent: binding.agent.clone(),
            chief: None,
            session: 0,
        },
    };
    let task = marked_task(service.home.store(), binding.id, input)?;
    let caller = Caller {
        binding,
        place,
        id: OperationId {
            runtime: binding.runtime.clone(),
            key: key.into(),
        },
        request: Request::tool(tool, arguments)?,
        input,
        task,
    };
    let args = Value::Object(arguments.clone().into_iter().collect());
    let answered = match SocietyTool::named(tool) {
        // The owners of these answer them.
        None
        | Some(SocietyTool::BuildRoad | SocietyTool::AskChief) => {
            Some(SocietyOutcome::failed(format!(
                "There is no tool named {tool}."
            )))
        }
        Some(SocietyTool::Object) => {
            return match parsed(tool, &args) {
                Ok(given) => objections::object(
                    service, binding, key, input, given,
                ),
                Err(Unanswered::Refused(refused)) => {
                    Ok(SocietyOutcome::failed(refused).into())
                }
                Err(Unanswered::Failed(error)) => Err(error),
            };
        }
        Some(tool) => {
            match answer(service, harnesses, &caller, tool, &args)
                .await
            {
                Ok(answered) => answered,
                Err(Unanswered::Refused(refused)) => {
                    Some(SocietyOutcome::failed(refused))
                }
                Err(Unanswered::Failed(error)) => return Err(error),
            }
        }
    };
    let Some(outcome) = answered else {
        return Ok(letters::waiting());
    };
    let outcome = once(
        service.home.store(),
        &caller.id,
        &caller.request,
        |_| Ok(outcome),
    )?;
    if service.house_work.is_pending(&caller.id) {
        return Ok(letters::waiting());
    }
    Ok(outcome.into())
}

/// Why a society call ends before its tool answers.
enum Unanswered {
    /// Its arguments do not parse; the words say why, and the call
    /// fails with them.
    Refused(String),
    Failed(anyhow::Error),
}

impl From<anyhow::Error> for Unanswered {
    fn from(error: anyhow::Error) -> Self {
        Self::Failed(error)
    }
}

/// The arguments of a call to `tool`.
fn parsed<T: DeserializeOwned>(
    tool: &str,
    args: &Value,
) -> std::result::Result<T, Unanswered> {
    serde_json::from_value(args.clone()).map_err(|error| {
        Unanswered::Refused(format!(
            "The arguments of {tool} are not valid: {error}."
        ))
    })
}

/// What the society call of `caller` to `tool` answers, with its
/// arguments typed; `None` while it waits.
async fn answer(
    service: &mut Service,
    harnesses: &mut Harnesses,
    caller: &Caller<'_>,
    tool: SocietyTool,
    args: &Value,
) -> std::result::Result<Option<SocietyOutcome>, Unanswered> {
    use crate::realm::dealings;
    let name = tool.name();
    Ok(match tool {
        SocietyTool::BuildRoad
        | SocietyTool::AskChief
        | SocietyTool::Object => {
            unreachable!("{name} is answered before its arguments")
        }
        SocietyTool::CastBallot => {
            Some(votes::ballot(service, caller, parsed(name, args)?)?)
        }
        SocietyTool::MergeAgents => {
            let given = parsed(name, args)?;
            merging::operation(service, harnesses, caller, given)
                .await?
        }
        SocietyTool::CallVote => {
            let given = parsed(name, args)?;
            service
                .db()
                .execute(
                    "INSERT INTO society_call \
                     (runtime,key,binding,tool,input,data) VALUES \
                     (?1,?2,?3,'call_vote',?4,?5) ON CONFLICT DO \
                     NOTHING",
                    params![
                        caller.id.runtime.as_str(),
                        caller.id.key,
                        caller.binding.id.0,
                        caller.input,
                        args.to_string()
                    ],
                )
                .context("keeping the vote's call")?;
            votes::operation(service, harnesses, caller, &given)
                .await?
        }
        SocietyTool::CreateAgent => {
            let given = parsed(name, args)?;
            Some(agents::create(service, caller, given).await?)
        }
        SocietyTool::SetModel => Some(agents::set_model(
            service,
            caller,
            parsed(name, args)?,
        )?),
        SocietyTool::SubmitSkill => {
            let given: library::SubmitSkill = parsed(name, args)?;
            let skill = Some(given.skill.as_str());
            library::consult(
                service,
                harnesses,
                caller,
                skill,
                &given.reason,
            )
            .await?
        }
        SocietyTool::AskLibrarian => {
            let given: library::AskLibrarian = parsed(name, args)?;
            let question = &given.question;
            library::consult(
                service, harnesses, caller, None, question,
            )
            .await?
        }
        SocietyTool::AgreeRoad => {
            let given = parsed(name, args)?;
            dealings::agree_road(service, harnesses, caller, given)
                .await?
        }
        SocietyTool::AgreeMove => {
            let given = parsed(name, args)?;
            dealings::agree_move(service, harnesses, caller, given)
                .await?
        }
        SocietyTool::AskRemoval => {
            let given = parsed(name, args)?;
            dealings::ask_removal(service, harnesses, caller, given)
                .await?
        }
        SocietyTool::RemoveAgent => {
            let given = parsed(name, args)?;
            Some(
                leaving::remove(service, harnesses, caller, given)
                    .await?,
            )
        }
        SocietyTool::TellAgent => {
            let given = parsed(name, args)?;
            Some(
                letters::tell(service, harnesses, caller, given)
                    .await?,
            )
        }
        SocietyTool::AskAgent => {
            let given = parsed(name, args)?;
            letters::ask(service, harnesses, caller, given).await?
        }
        SocietyTool::RequestRoad => {
            let given = parsed(name, args)?;
            letters::request_road(service, harnesses, caller, given)
                .await?
        }
    })
}

/// A society tool's outcome as its marker keeps it: the tool's
/// outcome the agent reads, and what this server does once it is
/// kept. Each of the rest is a key of the marker only when it is set,
/// so a marker without them reads as a tool's outcome too.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct SocietyOutcome {
    /// The tool's outcome, which the agent reads.
    #[serde(flatten)]
    pub tool: Outcome,
    /// The mayor a dealing tells, and what it is told.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notice: Option<Notice>,
    /// The other mayor a dealing asks to agree, and the question.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ask: Option<letters::Message>,
    /// The number of the agent a removal took out.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number: Option<u64>,
    /// The archive of the agent a merge took in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archive: Option<String>,
}

impl SocietyOutcome {
    /// The outcome of a call that answers with `text`.
    pub(crate) fn answered(text: impl Into<String>) -> Self {
        Self::from(Outcome {
            text: text.into(),
            error: false,
            details: None,
            diagnostics: Vec::new(),
        })
    }

    /// The outcome of a call that failed, saying why in `text`.
    pub(crate) fn failed(text: impl Into<String>) -> Self {
        let mut outcome = Self::answered(text);
        outcome.tool.error = true;
        outcome
    }
}

impl From<Outcome> for SocietyOutcome {
    fn from(tool: Outcome) -> Self {
        Self {
            tool,
            notice: None,
            ask: None,
            number: None,
            archive: None,
        }
    }
}

impl From<SocietyOutcome> for Answer {
    fn from(outcome: SocietyOutcome) -> Self {
        outcome.tool.into()
    }
}

/// A dealing's notice to the mayor of `hamlet`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Notice {
    pub hamlet: String,
    pub text: String,
}

/// The outcome of the call `id`: the one its marker keeps, else the
/// one `make` decides now, kept with its marker in the same
/// transaction as what it changed. A call whose identity is marked
/// for another request runs nothing.
pub(crate) fn once(
    store: &mut Store,
    id: &OperationId,
    request: &Request,
    make: impl FnOnce(&Connection) -> Result<SocietyOutcome>,
) -> Result<SocietyOutcome> {
    let made = store.once(id, request, make)?;
    Ok(made.unwrap_or_else(|| {
        SocietyOutcome::failed(crate::harness::tools::CONFLICT)
    }))
}

/// The author of a change made by `place`: the mayor while it holds
/// the office, else a member.
pub(crate) fn actor(
    db: &Connection,
    place: &AgentRef,
) -> Result<Actor> {
    let mayor = crate::record::read(db, &place.hamlet)?
        .is_some_and(|record| record.mayor == place.number);
    Ok(if mayor {
        Actor::Mayor(place.clone())
    } else {
        Actor::Member(place.clone())
    })
}

/// The society document `kind` of `key`, or its first value.
pub(crate) fn document<T: DeserializeOwned + Default>(
    db: &Connection,
    kind: &str,
    key: &str,
) -> Result<T> {
    db.query_row(
        "SELECT value FROM society_document WHERE kind = ?1 AND key \
         = ?2",
        params![kind, key],
        |row| row.get::<_, String>(0),
    )
    .optional()?
    .map_or_else(
        || Ok(T::default()),
        |v| {
            serde_json::from_str(&v).with_context(|| {
                format!("the stored {kind} document of {key}")
            })
        },
    )
}

/// Keeps the society document `kind` of `key` whole.
pub(crate) fn keep<T: Serialize>(
    db: &Connection,
    kind: &str,
    key: &str,
    value: &T,
) -> Result<()> {
    db.execute(
        "INSERT INTO society_document (kind, key, value) VALUES \
         (?1, ?2, ?3) ON CONFLICT (kind, key) DO UPDATE SET value = \
         excluded.value",
        params![kind, key, serde_json::to_string(value)?],
    )?;
    Ok(())
}

/// The H12 task of the turn whose input `input` of `binding` made a
/// call, as the input's mark keeps it.
pub(crate) fn marked_task(
    store: &Store,
    binding: BindingId,
    input: Option<&str>,
) -> Result<Option<String>> {
    Ok(store
        .input_mark(binding, input)?
        .as_ref()
        .and_then(InputMark::task)
        .map(String::from))
}

/// The H12 task of the turn whose input `input` of `binding` made a
/// call: the marked one, else one of the input's own.
pub(crate) fn task_of(
    store: &Store,
    binding: &Binding,
    input: Option<&str>,
) -> Result<Option<String>> {
    let Some(input) = input else {
        return Ok(None);
    };
    Ok(Some(
        marked_task(store, binding.id, Some(input))?.unwrap_or_else(
            || format!("{}:{input}", binding.runtime.as_str()),
        ),
    ))
}

pub(crate) fn payer(
    db: &Connection,
    binding: &Binding,
) -> Result<String> {
    Ok(db
        .query_row(
            "SELECT payer FROM society_binding WHERE binding = ?1",
            [binding.id.0],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten()
        .unwrap_or_else(|| binding.agent.hamlet.clone()))
}

/// A chief deed's words, or its refusal.
pub(crate) type Deed = Refusable<String>;

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::SocietyOutcome;

    /// Reads `marker` as a society outcome and writes it back.
    fn round_trip(marker: &Value) -> Value {
        let outcome: SocietyOutcome =
            serde_json::from_value(marker.clone()).unwrap();
        serde_json::to_value(outcome).unwrap()
    }

    #[test]
    fn a_kept_marker_reads_and_writes_back_as_it_was_stored() {
        let markers = [
            json!({"details": null, "diagnostics": [], "error": false,
                "text": "Your ballot is cast."}),
            json!({"details": null, "diagnostics": [{"code":
                "tool_unavailable", "message":
                "Tool merge_agents is not available", "severity":
                "error"}], "error": true, "text": ""}),
            json!({"details": null, "diagnostics": [], "error": false,
                "notice": {"hamlet": "b", "text": "Enough have asked."},
                "text": "Enough have asked."}),
            json!({"ask": {"message": "Please consider agreeing.",
                "name": "mayor@b"}, "details": null, "diagnostics": [],
                "error": false, "text": "The move is proposed."}),
            json!({"details": null, "diagnostics": [], "error": false,
                "number": 3, "text": "Removed `ana`."}),
            json!({"archive": "a1", "details": null, "diagnostics": [],
                "error": false, "text": "Merged `bo` into `ana`."}),
        ];
        for marker in markers {
            assert_eq!(round_trip(&marker), marker);
        }
    }

    #[test]
    fn an_outcome_without_its_extra_keys_writes_none_of_them() {
        let written =
            serde_json::to_value(SocietyOutcome::failed("No."))
                .unwrap();
        assert_eq!(
            written,
            json!({"text": "No.", "error": true, "details": null,
                "diagnostics": []})
        );
    }
}
