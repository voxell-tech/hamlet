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

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

pub(crate) use self::agents::deed;
use crate::chats::records;
use crate::harness::link::{
    ConfigureTools, Diagnostic, DiagnosticSeverity, ToolSpec,
    ToolSpecReplay,
};
use crate::harness::{Answer, Binding, BindingId, Harnesses};
use crate::operation::Request;
use crate::protocol::Refusal;
use crate::record::{Actor, AgentRef};
use crate::service::Service;
use crate::{OperationId, Store};

/// The tools the mayor alone is offered.
const MAYOR: &[&str] = &[
    "create_agent",
    "build_road",
    "set_model",
    "remove_agent",
    "merge_agents",
];
/// The mayor's tools for dealing with other hamlets, offered while
/// more than one lives.
const REALM: &[&str] = &["agree_road", "agree_move", "ask_removal"];
/// The tools every agent is offered in its own conversations.
const EVERY: &[&str] = &[
    "tell_agent",
    "ask_agent",
    "call_vote",
    "submit_skill",
    "ask_librarian",
    "ask_chief",
];

/// The society tools runtimes are given; `build_road` and `ask_chief`
/// are given by their owners.
pub(crate) fn specs() -> Vec<ToolSpec> {
    let string = || json!({"type": "string"});
    let strings =
        || json!({"type": "array", "items": {"type": "string"}});
    let tool = |name: &str,
                description: &str,
                properties: Value,
                required: &[&str]| ToolSpec {
        name: name.into(),
        description: description.into(),
        parameters: object(json!({
            "type": "object",
            "properties": properties,
            "required": required,
        })),
        replay: ToolSpecReplay::Safe,
    };
    vec![
        tool(
            "create_agent",
            "Create an agent with a purpose, roads, model and \
             character.",
            json!({"name": string(), "purpose": string(), "reason": string(), "roads": strings(), "model": string(), "character": string()}),
            &["name", "purpose", "reason"],
        ),
        tool(
            "set_model",
            "Choose an allowed model for an agent, or empty for the \
             hamlet's own.",
            json!({"name": string(), "model": string(), "reason": string()}),
            &["name", "model", "reason"],
        ),
        tool(
            "remove_agent",
            "Remove an agent, archiving its house and roads. A \
             reason is kept in the record.",
            json!({"name": string(), "reason": string()}),
            &["name", "reason"],
        ),
        tool(
            "merge_agents",
            "Merge two agents' houses and roads under the kept \
             agent's new purpose. A reason is kept in the record.",
            json!({"keep": string(), "absorb": string(), "purpose": string(), "reason": string()}),
            &["keep", "absorb", "purpose", "reason"],
        ),
        tool(
            "agree_road",
            "Agree to a road between agents of different hamlets. \
             Both mayors must agree.",
            json!({"a": string(), "b": string(), "reason": string()}),
            &["a", "b", "reason"],
        ),
        tool(
            "agree_move",
            "Agree that an agent moves with its house. Only roads \
             both mayors name are kept.",
            json!({"agent": string(), "to": string(), "keep_roads": strings(), "reason": string()}),
            &["agent", "to", "reason"],
        ),
        tool(
            "ask_removal",
            "Ask the chief to remove a hamlet. Nothing is removed \
             until a chief confirms.",
            json!({"hamlet": string(), "reason": string()}),
            &["hamlet", "reason"],
        ),
        tool(
            "tell_agent",
            "Send a letter along a road. Its reply comes as a new \
             message.",
            json!({"name": string(), "message": string()}),
            &["name", "message"],
        ),
        tool(
            "ask_agent",
            "Ask an agent along a road and wait for its answer.",
            json!({"name": string(), "message": string()}),
            &["name", "message"],
        ),
        tool(
            "request_road",
            "Ask the mayor to build a road to another agent.",
            json!({"to": string(), "reason": string()}),
            &["to", "reason"],
        ),
        tool(
            "call_vote",
            "Call a vote to replace the mayor or found a hamlet. \
             Every agent besides the mayor votes from its own \
             house. Returns the count; the chief may overturn it.",
            json!({"kind": string(), "reason": string(), "candidate": string(), "hamlet": string(), "mayor": string(), "purpose": string()}),
            &["kind", "reason"],
        ),
        tool(
            "cast_ballot",
            "Cast your ballot in the vote you were asked about, \
             with words copied from your own notes or history.",
            json!({"choice": string(), "reason": string(), "evidence": string()}),
            &["choice", "reason", "evidence"],
        ),
        tool(
            "submit_skill",
            "Offer one of your own skills to the shared library. \
             The librarian takes it or says why not. Once taken it \
             moves out of your house; use the library's copy.",
            json!({"skill": string(), "reason": string()}),
            &["skill", "reason"],
        ),
        tool(
            "ask_librarian",
            "Ask which skill on the library's shelves does what you \
             need, and the command that runs it. Ask before writing \
             a skill of your own.",
            json!({"question": string()}),
            &["question"],
        ),
        tool(
            "object",
            "Tell the chief why the instruction you were just given \
             is a mistake. The work waits for its reply. You may \
             object once; after go ahead, do the work.",
            json!({"reason": string()}),
            &["reason"],
        ),
    ]
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
    let db = service.home.store().connection();
    if let Some(kind) = errand_of(db, binding)? {
        return Ok(ConfigureTools::Only {
            only: if kind == "ballot" {
                vec!["cast_ballot".into()]
            } else {
                Vec::new()
            },
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
    let realm =
        crate::realm::live(service.home.store().connection())?.len()
            > 1;
    let allowed = |tool: &str| {
        EVERY.contains(&tool)
            || (tool == "object" && place.chief.is_some())
            || if mayor {
                MAYOR.contains(&tool)
                    || (realm && REALM.contains(&tool))
            } else {
                tool == "request_road"
            }
    };
    Ok(ConfigureTools::Remove {
        remove: crate::gate::SOCIETY
            .iter()
            .filter(|tool| !allowed(tool))
            .map(|tool| (*tool).into())
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
    let db = service.home.store().connection();
    Ok(errand_of(db, binding)?.is_some_and(|kind| {
        !(kind == "ballot" && tool == "cast_ballot")
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
    let db = service.home.store().connection();
    let live = crate::realm::live(db)?;
    for binding in records::all(db)? {
        let db = service.home.store().connection();
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
pub(crate) fn unavailable(tool: &str) -> Value {
    json!({
        "text": "",
        "error": true,
        "diagnostics": [Diagnostic {
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
    let db = service.home.store().connection();
    let place = match records::kept(db, binding.id)? {
        Some(kept) => kept.place,
        None => records::Place {
            agent: binding.agent.clone(),
            chief: None,
            session: 0,
        },
    };
    let request = match input {
        Some(input) => {
            service.home.store().request_of(binding.id, input)?
        }
        None => None,
    };
    let db = service.home.store().connection();
    let task = match request {
        Some(request) => records::mark_of(db, binding.id, &request)?
            .and_then(|mark| {
                mark.data
                    .get("task")
                    .and_then(Value::as_str)
                    .map(String::from)
            }),
        None => None,
    };
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
    let outcome = match tool {
        "object" => {
            return objections::object(
                service, binding, key, input, &args,
            );
        }
        "cast_ballot" => votes::ballot(service, &caller, &args)?,
        "merge_agents" => match merging::operation(
            service, harnesses, &caller, &args,
        )
        .await?
        {
            Some(answer) => answer,
            None => return Ok(letters::waiting()),
        },
        "call_vote" => {
            service.home.store().connection().execute(
                "INSERT INTO society_call \
                 (runtime,key,binding,tool,input,data) VALUES \
                 (?1,?2,?3,'call_vote',?4,?5) ON CONFLICT DO NOTHING",
                params![
                    caller.id.runtime.as_str(),
                    caller.id.key,
                    binding.id.0,
                    input,
                    args.to_string()
                ],
            )?;
            match votes::operation(service, harnesses, &caller, &args)
                .await?
            {
                Some(answer) => answer,
                None => return Ok(letters::waiting()),
            }
        }
        "create_agent" => {
            agents::create(service, &caller, &args).await?
        }
        "set_model" => agents::set_model(service, &caller, &args)?,
        "submit_skill" | "ask_librarian" => {
            match library::consult(
                service, harnesses, &caller, tool, &args,
            )
            .await?
            {
                Some(answer) => answer,
                None => return Ok(letters::waiting()),
            }
        }
        "agree_road" | "agree_move" | "ask_removal" => {
            match crate::realm::dealings::operation(
                service, harnesses, &caller, tool, &args,
            )
            .await?
            {
                Some(answer) => answer,
                None => return Ok(letters::waiting()),
            }
        }
        "remove_agent" => {
            leaving::remove(service, harnesses, &caller, &args)
                .await?
        }
        "tell_agent" => {
            letters::tell(service, harnesses, &caller, &args).await?
        }
        "ask_agent" | "request_road" => {
            let answered = if tool == "ask_agent" {
                letters::ask(service, harnesses, &caller, &args)
                    .await?
            } else {
                letters::request_road(
                    service, harnesses, &caller, &args,
                )
                .await?
            };
            match answered {
                Some(outcome) => outcome,
                None => return Ok(letters::waiting()),
            }
        }
        _ => {
            outcome(&format!("There is no tool named {tool}."), true)
        }
    };
    let outcome = once(
        service.home.store(),
        &caller.id,
        &caller.request,
        |_| Ok(outcome),
    )?;
    if service.house_work.pending.contains(&(
        caller.id.runtime.as_str().into(),
        caller.id.key.clone(),
    )) {
        return Ok(letters::waiting());
    }
    Ok(result(&outcome))
}

/// A tool's outcome as markers keep it: its text and whether it
/// failed.
pub(crate) fn outcome(text: &str, error: bool) -> Value {
    json!({"text": text, "error": error})
}

/// The answer of a marked outcome.
pub(crate) fn result(outcome: &Value) -> Answer {
    Answer::Result {
        text: outcome["text"].as_str().unwrap_or_default().into(),
        error: outcome["error"] == true,
        details: outcome["details"]
            .as_object()
            .map(|details| details.clone().into_iter().collect()),
        diagnostics: serde_json::from_value(
            outcome["diagnostics"].clone(),
        )
        .unwrap_or_default(),
    }
}

/// The outcome of the call `id`: the one its marker keeps, else the
/// one `make` decides now, kept with its marker in the same
/// transaction as what it changed. A call whose identity is marked
/// for another request runs nothing.
pub(crate) fn once(
    store: &mut Store,
    id: &OperationId,
    request: &Request,
    make: impl FnOnce(&Connection) -> Result<Value>,
) -> Result<Value> {
    Ok(store.once(id, request, make)?.unwrap_or_else(|| {
        outcome(crate::harness::tools::CONFLICT, true)
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
        |v| Ok(serde_json::from_str(&v)?),
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

/// The present time in milliseconds.
pub(crate) fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

pub(crate) fn task_of(
    store: &Store,
    binding: &Binding,
    input: Option<&str>,
) -> Result<Option<String>> {
    let Some(input) = input else {
        return Ok(None);
    };
    if let Some(request) = store.request_of(binding.id, input)?
        && let Some(mark) = records::mark_of(
            store.connection(),
            binding.id,
            &request,
        )?
        && let Some(task) =
            mark.data.get("task").and_then(Value::as_str)
    {
        return Ok(Some(task.into()));
    }
    Ok(Some(format!("{}:{input}", binding.runtime.as_str())))
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
pub(crate) type Deed = std::result::Result<String, Refusal>;
