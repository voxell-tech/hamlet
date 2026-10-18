//! What a runtime asks of the server while a turn runs: whether a
//! tool call may run (C7, H11), the call carried out, the prompt, and
//! whether a final answer stands (C28). And a chief's answers to what
//! waits.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Result;
use rusqlite::{OptionalExtension, params};
use serde_json::{Map, Value, json};
use tokio::task::AbortHandle;

use super::follow;
use super::records::{self, Place};
use crate::gate::{self, Asked, Standing, TrustKey, Verdict};
use crate::harness::link::{
    Block, Item, Section, ToolSpec, ToolSpecReplay,
};
use crate::harness::{Answer, BindingId, Call, Harnesses, Re};
use crate::oversight::{self, ChatPlace, Raising};
use crate::record::{AgentRef, Hamlet};
use crate::service::Service;
use crate::tools;

/// The words of a call that waits for a chief.
const WAITING: &str = "This call waits for a chief's answer.";
/// The words of a call that nobody may approve.
const NOBODY: &str = "nobody is here to approve it; it did not run";
/// The chief's recovery advice for an answer with no text or calls.
const EMPTY: &str =
    "the model answered with nothing. Send the message again";
/// The chief's recovery advice for an answer cut off for room.
const NO_ROOM: &str = "the model has no room left to answer, even \
                       with the conversation summarised. Start a \
                       new session, or choose a model that holds \
                       more";
/// The tools whose call may change the world, run once.
const UNSAFE: &[&str] = &["bash", "write", "edit"];

/// A tool call that finished running.
pub(crate) struct Ran {
    runtime: String,
    key: String,
    outcome: tools::Outcome,
}

/// A tool call running, and the calls of runtimes it answers.
pub(super) struct Running {
    res: Vec<Re>,
    binding: BindingId,
    tool: String,
    abort: AbortHandle,
}

/// The tools the server adds to every conversation's own.
pub(crate) fn specs() -> Vec<ToolSpec> {
    let parameters = json!({
        "type": "object",
        "properties": {"question": {"type": "string"}},
        "required": ["question"],
    });
    vec![ToolSpec {
        name: "ask_chief".into(),
        description: "Ask the chief a question you cannot go on \
                      without. Waits for the answer, so ask only \
                      when no agent and no file can tell you. Use \
                      object to argue against an instruction."
            .into(),
        parameters: match parameters {
            Value::Object(map) => map.into_iter().collect(),
            _ => BTreeMap::new(),
        },
        replay: ToolSpecReplay::Safe,
    }]
}

/// Answers what the runtime of `binding` asked.
pub(super) async fn call(
    service: &mut Service,
    harnesses: &mut Harnesses,
    binding: BindingId,
    re: Re,
    call: Call,
) -> Result<()> {
    let input = match &call {
        Call::Allow { input, .. }
        | Call::Operation { input, .. }
        | Call::Prompt { input }
        | Call::Yield { input, .. } => input.clone(),
    };
    if input.is_some() {
        super::conversation(service, binding)?
            .turn
            .clone_from(&input);
    }
    let answer = match call {
        Call::Allow {
            key,
            tool,
            arguments,
            ..
        } => {
            match decide(
                service,
                binding,
                &key,
                input.as_deref(),
                &tool,
                &arguments,
            )? {
                Decision::Run { .. } => Answer::Allowed,
                Decision::Block(words) => Answer::Refusal(words),
                Decision::Wait => Answer::Waiting(WAITING.into()),
            }
        }
        Call::Operation {
            key,
            tool,
            arguments,
            ..
        } => {
            match operation(
                service,
                binding,
                &re,
                &key,
                input.as_deref(),
                &tool,
                &arguments,
            )? {
                Some(answer) => answer,
                None => return Ok(()),
            }
        }
        Call::Prompt { .. } => Answer::Sections(prompt(
            service,
            binding,
            input.as_deref(),
        )?),
        Call::Yield { answer, .. } => {
            verdict(service, binding, input.as_deref(), &answer)?
        }
    };
    harnesses.answer(re, answer).await;
    Ok(())
}

/// What the gate decided of a call.
enum Decision {
    Run { net: bool },
    Block(String),
    Wait,
}

/// The gate's decision of the call `key`: the one kept, else one
/// made now and kept, raising a wait when it asks a chief.
fn decide(
    service: &mut Service,
    binding: BindingId,
    key: &str,
    input: Option<&str>,
    tool: &str,
    arguments: &BTreeMap<String, Value>,
) -> Result<Decision> {
    let Some(bound) = service.home.store().binding(binding)? else {
        return Ok(Decision::Block(NOBODY.into()));
    };
    let runtime = bound.runtime.as_str().to_owned();
    if let Some(decision) = kept(service, &runtime, key)? {
        return Ok(decision);
    }
    let db = service.home.store().connection();
    let Some(conversation) = records::kept(db, binding)? else {
        keep_call(
            service,
            &runtime,
            key,
            binding,
            input,
            Some(false),
            None,
            None,
        )?;
        return Ok(Decision::Run { net: false });
    };
    if service.home.store().cancelling(binding)? {
        let words = crate::harness::answers::STOPPED_WORDS;
        keep_call(
            service,
            &runtime,
            key,
            binding,
            input,
            None,
            Some(words),
            None,
        )?;
        return Ok(Decision::Block(words.into()));
    }
    let place = conversation.place;
    let args = Value::Object(arguments.clone().into_iter().collect());
    let mut call = gate::call_of(tool, &args);
    let at = places(service, &place.agent)?;
    if let gate::Call::Edit {
        write: true,
        path,
        old,
        created,
        ..
    } = &mut call
        && let Ok(full) = tools::footing(path, &at)
        && let Some(before) = before(service, &full)
    {
        *old = before;
        *created = false;
    }
    let change = match &call {
        gate::Call::Edit {
            write,
            path,
            old,
            now,
            created,
        } => {
            let mut change =
                serde_json::to_value(crate::change::change_between(
                    path,
                    old,
                    now,
                    *write && *created,
                ))?;
            service.credentials.scrub(&mut change);
            Some(change)
        }
        _ => None,
    };
    let Some(record) =
        service.home.store().record(&place.agent.hamlet)?
    else {
        return Ok(Decision::Block(NOBODY.into()));
    };
    let by = wait_by(&record, &place.agent);
    let mut standing = standing(service, &place, &record)?;
    if let gate::Call::Edit { path, .. } = &call
        && let Ok(full) = tools::footing(path, &at)
        && crate::sandbox::within(&full, &at.house)
    {
        standing.house_edit = true;
    }
    let limits = service.limits_of(&place.agent.hamlet)?;
    let db = service.home.store().connection();
    let asked = Asked::of(db, binding, input)?;
    let mut about = Value::String(gate::call_text(&call));
    service.credentials.scrub(&mut about);
    let about = about.as_str().unwrap_or_default().to_owned();
    let verdict = gate::rule(
        &call,
        &standing,
        &asked,
        &by,
        &about,
        &limits,
        record.mayor == place.agent.number,
    );
    let change_text = change.as_ref().map(Value::to_string);
    let danger = match verdict {
        Verdict::Run { net } => {
            keep_call(
                service,
                &runtime,
                key,
                binding,
                input,
                Some(net),
                None,
                change_text,
            )?;
            return Ok(Decision::Run { net });
        }
        Verdict::Block(words) => {
            asked.keep(service.home.store().connection(), binding)?;
            keep_call(
                service,
                &runtime,
                key,
                binding,
                input,
                None,
                Some(&words),
                change_text,
            )?;
            return Ok(Decision::Block(words));
        }
        Verdict::Ask(danger) => danger,
    };
    asked.keep(service.home.store().connection(), binding)?;
    let reflecting = request_of(service, binding, input)?
        .is_some_and(|request| request.starts_with("reflect:"));
    if service.chats.closing && reflecting {
        keep_call(
            service,
            &runtime,
            key,
            binding,
            input,
            None,
            Some(NOBODY),
            change_text,
        )?;
        return Ok(Decision::Block(NOBODY.into()));
    }
    keep_call(
        service,
        &runtime,
        key,
        binding,
        input,
        None,
        None,
        change_text,
    )?;
    oversight::raise(
        service.home.store().connection(),
        Raising {
            kind: "approval",
            danger: Some(danger.into()),
            by,
            agent: Some(place.agent.clone()),
            about,
            change,
            answers: if danger == "destructive"
                || danger == "reserved"
            {
                vec!["allow".into(), "deny".into()]
            } else {
                vec!["allow".into(), "always".into(), "deny".into()]
            },
            chat: chat_of(&place),
            call: Some((bound.runtime.clone(), key.into())),
        },
        super::now(),
    )?;
    Ok(Decision::Wait)
}

/// The decision kept for the call `key` of `runtime`.
fn kept(
    service: &mut Service,
    runtime: &str,
    key: &str,
) -> Result<Option<Decision>> {
    let db = service.home.store().connection();
    let row = db
        .query_row(
            "SELECT net, block, wait, how, text FROM gate_call \
             WHERE runtime = ?1 AND key = ?2",
            params![runtime, key],
            |row| {
                Ok((
                    row.get::<_, Option<bool>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            },
        )
        .optional()?;
    let Some((net, block, wait, how, text)) = row else {
        return Ok(None);
    };
    if let Some(block) = block {
        return Ok(Some(Decision::Block(block)));
    }
    if let Some(how) = how {
        return Ok(Some(match how.as_str() {
            "allow" | "always" => Decision::Run {
                net: net.unwrap_or(false),
            },
            "deny" => Decision::Block(format!(
                "the chief refused it: {}",
                text.filter(|t| !t.is_empty())
                    .unwrap_or_else(|| "no reason given".into())
            )),
            _ => Decision::Block(NOBODY.into()),
        }));
    }
    if wait.is_some() {
        return Ok(Some(Decision::Wait));
    }
    Ok(net.map(|net| Decision::Run { net }))
}

#[allow(clippy::too_many_arguments)]
fn keep_call(
    service: &mut Service,
    runtime: &str,
    key: &str,
    binding: BindingId,
    input: Option<&str>,
    net: Option<bool>,
    block: Option<&str>,
    change: Option<String>,
) -> Result<()> {
    service.home.store().connection().execute(
        "INSERT INTO gate_call (runtime, key, binding, input, net, \
         block, change) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) ON \
         CONFLICT DO NOTHING",
        params![runtime, key, binding.0, input, net, block, change],
    )?;
    Ok(())
}

/// The text of the file at `path` as a write's preview shows it, or
/// `None` when there is none.
fn before(
    service: &Service,
    path: &std::path::Path,
) -> Option<String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound =>
        {
            return None;
        }
        Err(_) => {
            let size = std::fs::metadata(path)
                .map(|m| format!(", {} bytes", m.len()))
                .unwrap_or_default();
            return Some(format!(
                "[a file that could not be read as text{size}]"
            ));
        }
    };
    match String::from_utf8(bytes) {
        Ok(text) => {
            let mut text = Value::String(text);
            service.credentials.scrub(&mut text);
            text.as_str().map(String::from)
        }
        Err(error) => Some(format!(
            "[a file that is not UTF-8 text, {} bytes]",
            error.as_bytes().len()
        )),
    }
}

/// The grants, limits, mode and session trust in force for `place`.
fn standing(
    service: &mut Service,
    place: &Place,
    record: &Hamlet,
) -> Result<Standing> {
    let hamlet = &place.agent.hamlet;
    let work = service.work(hamlet)?;
    let limits = service.limits_of(hamlet)?;
    let allow_net = service.settings()?["allowNet"] == true;
    let db = service.home.store().connection();
    let grants = gate::grants(db)?;
    let by = record.address_of(place.agent.number);
    let chief = place.chief.clone().unwrap_or_default();
    let (edit, command) = gate::trusted(
        db,
        &TrustKey {
            agent: &place.agent,
            chief: &chief,
            session: place.session,
        },
    )?;
    let manual =
        !work["folder"].is_null() && work["mode"] == "manual";
    Ok(Standing {
        house_edit: false,
        net: allow_net
            || gate::holds(
                &limits,
                &grants,
                by,
                &place.agent,
                "network",
            ),
        asks_edits: manual && !edit,
        asks_commands: manual && !command,
    })
}

/// The name a wait shows for `agent`.
fn wait_by(record: &Hamlet, agent: &AgentRef) -> String {
    let name = record.address_of(agent.number);
    if name == "mayor" && agent.hamlet != "home" {
        format!("mayor@{}", agent.hamlet)
    } else {
        name.into()
    }
}

fn chat_of(place: &Place) -> Option<ChatPlace> {
    place.chief.as_ref().map(|chief| ChatPlace {
        hamlet: place.agent.hamlet.clone(),
        number: place.agent.number,
        chief: chief.clone(),
        session: place.session,
    })
}

/// Where the tools of `agent` work.
fn places(
    service: &mut Service,
    agent: &AgentRef,
) -> Result<tools::Place> {
    let root = service.home.layout().root().to_owned();
    let work = service.work(&agent.hamlet)?;
    Ok(tools::Place {
        house: root
            .join("houses")
            .join(&agent.hamlet)
            .join(agent.number.to_string()),
        folder: work["folder"].as_str().map(PathBuf::from),
        home: root.clone(),
        shared: vec![root.join("tools"), root.join("library")],
    })
}

/// Carries out the call `key`: its marked result when it ran, else as
/// the gate decides now. `None` while it runs.
fn operation(
    service: &mut Service,
    binding: BindingId,
    re: &Re,
    key: &str,
    input: Option<&str>,
    tool: &str,
    arguments: &BTreeMap<String, Value>,
) -> Result<Option<Answer>> {
    let Some(bound) = service.home.store().binding(binding)? else {
        return Ok(Some(Answer::result(NOBODY, true)));
    };
    let runtime = bound.runtime.as_str().to_owned();
    let id = (runtime.clone(), key.to_owned());
    if let Some(running) = service.chats.running.get_mut(&id) {
        running.res.push(re.clone());
        return Ok(None);
    }
    let db = service.home.store().connection();
    let marked = db
        .query_row(
            "SELECT outcome FROM marker WHERE runtime = ?1 AND key \
             = ?2 AND kind = 'tool'",
            params![runtime, key],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if let Some(outcome) = marked {
        return Ok(Some(if outcome.is_empty() {
            Answer::result(
                format!(
                    "Tool {tool} was interrupted by a restart and \
                     may have partially run"
                ),
                true,
            )
        } else {
            result(serde_json::from_str(&outcome)?)
        }));
    }
    if tool == "ask_chief" {
        return ask(
            service,
            binding,
            &bound.runtime,
            key,
            input,
            arguments,
        )
        .map(Some);
    }
    let net = match decide(
        service, binding, key, input, tool, arguments,
    )? {
        Decision::Run { net } => net,
        Decision::Block(words) => {
            return Ok(Some(Answer::Refusal(words)));
        }
        Decision::Wait => {
            return Ok(Some(Answer::Waiting(WAITING.into())));
        }
    };
    if tool == "build_road" {
        let call = match crate::harness::tools::parse(tool, arguments)
        {
            Ok(call) => call,
            Err(words) => {
                return Ok(Some(Answer::result(words, true)));
            }
        };
        let (text, error) = crate::harness::tools::run(
            service.home.store(),
            &crate::OperationId {
                runtime: bound.runtime.clone(),
                key: key.into(),
            },
            &bound,
            &call,
            super::now(),
        )?;
        return Ok(Some(Answer::result(text, error)));
    }
    let change = service
        .home
        .store()
        .connection()
        .query_row(
            "SELECT change FROM gate_call WHERE runtime = ?1 AND \
             key = ?2",
            params![runtime, key],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten();
    if UNSAFE.contains(&tool) {
        service.home.store().connection().execute(
            "INSERT INTO marker (runtime, key, kind, digest, \
             outcome) VALUES (?1, ?2, 'tool', ?3, '') ON CONFLICT \
             DO NOTHING",
            params![runtime, key, tool.as_bytes()],
        )?;
    }
    let place = places(service, &bound.agent)?;
    let sandbox = service.chats.sandbox;
    let secrets = service.credentials.secrets();
    let arguments =
        arguments.clone().into_iter().collect::<Map<_, _>>();
    let name = tool.to_owned();
    let (task_runtime, task_key) = id.clone();
    let abort = service.chats.tools.spawn(async move {
        let mut outcome = tools::run(
            &sandbox, &place, net, &secrets, &name, &arguments,
        )
        .await;
        if let Some(change) = change
            .and_then(|c| serde_json::from_str::<Value>(&c).ok())
        {
            outcome
                .details
                .get_or_insert_with(Map::new)
                .insert("change".into(), change);
        }
        Ran {
            runtime: task_runtime,
            key: task_key,
            outcome,
        }
    });
    service.chats.running.insert(
        id,
        Running {
            res: vec![re.clone()],
            binding,
            tool: tool.into(),
            abort,
        },
    );
    Ok(None)
}

/// Answers the calls a finished tool call was asked by, keeping its
/// result when it may not run again.
pub(crate) async fn ran(
    service: &mut Service,
    harnesses: &mut Harnesses,
    ran: Ran,
) -> Result<()> {
    let Some(running) = service
        .chats
        .running
        .remove(&(ran.runtime.clone(), ran.key.clone()))
    else {
        return Ok(());
    };
    let outcome = json!({
        "text": ran.outcome.text,
        "error": ran.outcome.error,
        "details": ran.outcome.details,
        "diagnostics": ran.outcome.diagnostics,
    });
    if UNSAFE.contains(&running.tool.as_str()) {
        service.home.store().connection().execute(
            "UPDATE marker SET outcome = ?3 WHERE runtime = ?1 AND \
             key = ?2 AND kind = 'tool'",
            params![ran.runtime, ran.key, outcome.to_string()],
        )?;
    }
    let _ = running.binding;
    for re in running.res {
        harnesses.answer(re, result(outcome.clone())).await;
    }
    Ok(())
}

/// Stops the tool calls running for `binding`.
pub(super) fn stop_tools(service: &mut Service, binding: BindingId) {
    service.chats.running.retain(|_, running| {
        if running.binding == binding {
            running.abort.abort();
            false
        } else {
            true
        }
    });
}

fn result(outcome: Value) -> Answer {
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

/// The agent's question to the chief, which waits for the answer.
fn ask(
    service: &mut Service,
    binding: BindingId,
    runtime: &crate::RuntimeInstance,
    key: &str,
    input: Option<&str>,
    arguments: &BTreeMap<String, Value>,
) -> Result<Answer> {
    let answered = |text: &str| Ok(Answer::result(text, false));
    let db = service.home.store().connection();
    let row = db
        .query_row(
            "SELECT wait, how, text FROM gate_call WHERE runtime = \
             ?1 AND key = ?2",
            params![runtime.as_str(), key],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()?;
    // A row with no wait is the gate's allowing, before the question.
    if let Some((Some(_), how, text)) = &row {
        match how.as_deref() {
            Some("reply") => {
                return answered(&format!(
                    "The chief answers: {}",
                    text.clone().unwrap_or_default()
                ));
            }
            Some(_) => {
                return answered(
                    "The chief did not answer; go on with what you \
                     know.",
                );
            }
            None => return Ok(Answer::Waiting(WAITING.into())),
        }
    }
    let Some(conversation) = records::kept(db, binding)? else {
        return answered(
            "No chief oversees this turn; go on with what you know.",
        );
    };
    let place = conversation.place;
    let request = request_of(service, binding, input)?;
    let db = service.home.store().connection();
    let mark = request
        .as_deref()
        .map(|request| records::mark_of(db, binding, request))
        .transpose()?
        .flatten();
    let chief = match mark.as_ref().map(|mark| mark.kind.as_str()) {
        Some("hamlet.chief") => mark
            .as_ref()
            .and_then(|mark| mark.data.get("chief"))
            .and_then(Value::as_str)
            .map(String::from),
        Some("hamlet.retry") => place.chief.clone(),
        _ => None,
    };
    if chief.is_none() {
        return answered(
            "No chief oversees this turn; go on with what you know.",
        );
    }
    let question = arguments
        .get("question")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned();
    if question.is_empty() {
        return answered("Say what in `question`.");
    }
    let limits = service.limits_of(&place.agent.hamlet)?;
    let most = limits["asking"]["questions"].as_i64().unwrap_or(3);
    let db = service.home.store().connection();
    let mut asked = Asked::of(db, binding, input)?;
    if asked.questions >= most {
        return answered(&format!(
            "You have asked the chief {most} questions this turn; \
             go on with what you know."
        ));
    }
    asked.questions += 1;
    asked.keep(db, binding)?;
    let Some(record) =
        service.home.store().record(&place.agent.hamlet)?
    else {
        return answered(
            "No chief oversees this turn; go on with what you know.",
        );
    };
    keep_call(
        service,
        runtime.as_str(),
        key,
        binding,
        input,
        None,
        None,
        None,
    )?;
    oversight::raise(
        service.home.store().connection(),
        Raising {
            kind: "question",
            danger: None,
            by: wait_by(&record, &place.agent),
            agent: Some(place.agent.clone()),
            about: question,
            change: None,
            answers: vec!["reply".into(), "dismiss".into()],
            chat: chat_of(&place),
            call: Some((runtime.clone(), key.into())),
        },
        super::now(),
    )?;
    Ok(Answer::Waiting(WAITING.into()))
}

/// The prompt sections of the next model call of `binding`, with the
/// budget words held through its turn.
fn prompt(
    service: &mut Service,
    binding: BindingId,
    input: Option<&str>,
) -> Result<Vec<Section>> {
    let Some(bound) = service.home.store().binding(binding)? else {
        return Ok(Vec::new());
    };
    let agent = bound.agent.clone();
    let request = request_of(service, binding, input)?;
    let db = service.home.store().connection();
    if let (Some(input), Some(request)) = (input, &request)
        && request.starts_with("reflect:")
    {
        records::enter(db, binding, input)?;
    }
    let task = request
        .as_deref()
        .map(|request| records::mark_of(db, binding, request))
        .transpose()?
        .flatten()
        .and_then(|mark| {
            mark.data
                .get("task")
                .and_then(Value::as_str)
                .map(String::from)
        });
    let Some(record) = service.home.store().record(&agent.hamlet)?
    else {
        return Ok(Vec::new());
    };
    let Some(member) = record
        .agents
        .iter()
        .find(|a| a.number == agent.number)
        .cloned()
    else {
        return Ok(Vec::new());
    };
    let settings = service.settings()?;
    let limits = service.limits_of(&agent.hamlet)?;
    let model = crate::state::model(
        &member,
        &limits,
        &settings,
        record.mayor == agent.number,
    );
    let instructions = records::instructions(
        service.home.store().connection(),
        &agent.hamlet,
    )?;
    let sandbox = service.chats.sandbox.name().to_owned();
    let subject = crate::prompt::Subject {
        hamlet: &agent.hamlet,
        number: i64::try_from(agent.number)?,
        model: (!model.is_empty()).then_some(model.as_str()),
        sandbox: &sandbox,
        instructions: &instructions,
        task: task.as_deref(),
    };
    let mut sections = crate::prompt::sections(service, &subject)?;
    let held = input
        .map(|input| {
            records::budget_words(
                service.home.store().connection(),
                binding,
                input,
            )
        })
        .transpose()?
        .flatten();
    let budget = match held {
        Some(words) => words,
        None => {
            let words = crate::prompt::budget(service, &subject)?;
            if let Some(input) = input {
                records::keep_budget_words(
                    service.home.store().connection(),
                    binding,
                    input,
                    &words,
                )?;
            }
            words
        }
    };
    sections.push(Section {
        name: "budget".into(),
        text: budget,
    });
    Ok(sections)
}

/// Whether a final answer stands: one with no words and no call
/// fails, and its turn ends (C28).
fn verdict(
    service: &mut Service,
    binding: BindingId,
    input: Option<&str>,
    answer: &Item,
) -> Result<Answer> {
    let stands =
        answer.blocks.iter().flatten().any(|block| match block {
            Block::Call { .. } => true,
            Block::Text { text } => !text.trim().is_empty(),
            Block::Thinking { .. } => false,
        });
    if stands {
        return Ok(Answer::Verdict {
            words: None,
            notice: None,
        });
    }
    let request = request_of(service, binding, input)?;
    let db = service.home.store().connection();
    let mark = request
        .as_deref()
        .map(|request| records::mark_of(db, binding, request))
        .transpose()?
        .flatten();
    let reflection = mark
        .as_ref()
        .is_some_and(|mark| mark.kind == "hamlet.reflection");
    let task = mark.as_ref().and_then(|mark| mark.data.get("task"));
    let cutoff = answer.stop.as_deref() == Some("length")
        && answer
            .usage
            .as_ref()
            .is_some_and(|usage| usage.output < 1024)
        && !reflection
        && task.is_some_and(|task| !task.is_null());
    // Without compaction on the link, an answer cut off for room
    // fails at once.
    let words = if cutoff { NO_ROOM } else { EMPTY };
    if let Some(input) = input {
        follow::fail_answer(db, binding, input, words)?;
    }
    Ok(Answer::Verdict {
        words: Some(words.into()),
        notice: Some(super::notice(words)),
    })
}

/// A chief's answer to the wait `id`, in the words a reply gives; a
/// refusal in words.
pub(super) async fn answer_words(
    service: &mut Service,
    harnesses: &mut Harnesses,
    id: &str,
    choice: &str,
    text: &str,
) -> Result<std::result::Result<String, String>> {
    let db = service.home.store().connection();
    let Some(wait) = oversight::find(db, id)? else {
        return Ok(Ok("Nothing waits for you.".into()));
    };
    let hamlet = wait
        .agent
        .as_ref()
        .map(|agent| agent.hamlet.clone())
        .or_else(|| {
            wait.chat.as_ref().map(|chat| chat.hamlet.clone())
        });
    if let Some(hamlet) = hamlet
        && let Err(refusal) =
            service.access(&hamlet, "answer wait")?
    {
        return Ok(Err(refusal.text()));
    }
    if !wait.answers.iter().any(|answer| answer == choice) {
        return Ok(Err(format!(
            "That cannot be answered that way:\n{}",
            oversight::strip(&wait)
        )));
    }
    let mut words = Value::String(text.into());
    service.credentials.scrub(&mut words);
    let text = words.as_str().unwrap_or_default().trim().to_owned();
    let chief = service.chief.clone();
    let now = super::now();
    let danger = wait.danger.as_deref();
    if choice == "always"
        && matches!(danger, Some("network" | "unconfined"))
        && let Some(agent) = &wait.agent
    {
        gate::give(
            service.home.store().connection(),
            agent,
            &wait.by,
            danger.unwrap_or_default(),
            &chief,
            now,
        )?;
    }
    let held = {
        let db = service.home.store().connection();
        let mut query = db.prepare(
            "SELECT runtime, key, binding, input FROM gate_call \
             WHERE wait = ?1 AND how IS NULL",
        )?;
        query
            .query_map([id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    BindingId(row.get(2)?),
                    row.get::<_, Option<String>>(3)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    let mut counted = Vec::new();
    for (runtime, key, binding, input) in held {
        let db = service.home.store().connection();
        let Some(conversation) = records::kept(db, binding)? else {
            continue;
        };
        let place = conversation.place;
        if choice == "allow" || choice == "always" {
            let Some(record) =
                service.home.store().record(&place.agent.hamlet)?
            else {
                continue;
            };
            let standing = standing(service, &place, &record)?;
            let net = danger == Some("network")
                || (matches!(danger, Some("edit" | "command"))
                    && standing.net);
            let db = service.home.store().connection();
            db.execute(
                "UPDATE gate_call SET net = ?3 WHERE runtime = ?1 \
                 AND key = ?2",
                params![runtime, key, net],
            )?;
            if choice == "always"
                && let Some(danger @ ("edit" | "command")) = danger
            {
                let chief = place.chief.clone().unwrap_or_default();
                gate::trust(
                    db,
                    &TrustKey {
                        agent: &place.agent,
                        chief: &chief,
                        session: place.session,
                    },
                    danger,
                )?;
            }
        }
        if choice == "deny"
            && let Some(danger) = danger
            && !counted.contains(&binding)
        {
            let db = service.home.store().connection();
            let mut asked = Asked::of(db, binding, input.as_deref())?;
            asked.refuse(
                &wait.by,
                danger,
                &wait.about,
                if text.is_empty() {
                    "no reason given"
                } else {
                    &text
                },
            );
            asked.keep(db, binding)?;
            counted.push(binding);
        }
    }
    let Some((settled, woken)) = oversight::settle(
        service.home.store().connection(),
        id,
        choice,
        &text,
        Some(&chief),
        now,
    )?
    else {
        return Ok(Ok("Nothing waits for you.".into()));
    };
    for (runtime, key) in woken {
        harnesses.wake(&runtime, &key).await;
    }
    Ok(Ok(format!(
        "Answered /{choice}: {}",
        oversight::line(&settled, 80)
    )))
}

/// The request of the input `input` of `binding`.
fn request_of(
    service: &mut Service,
    binding: BindingId,
    input: Option<&str>,
) -> Result<Option<String>> {
    Ok(match input {
        Some(input) => {
            service.home.store().request_of(binding, input)?
        }
        None => None,
    })
}
