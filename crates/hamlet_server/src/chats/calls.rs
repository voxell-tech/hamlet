//! What a runtime asks of the server while a turn runs: whether a
//! tool call may run (C7, H11), the call carried out, the prompt, and
//! whether a final answer stands (C28). And a chief's answers to what
//! waits.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Result;
use hamlet_protocol::asking::{self, Pick};
use rusqlite::{OptionalExtension, params};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
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
const IDENTITY_REFUSAL: &str = "This call's identity does not match \
                                a verified request, so it did not \
                                run.";
/// The chief's recovery advice for an answer with no text or calls.
const EMPTY: &str =
    "the model answered with nothing. Send the message again";
/// The chief's recovery advice for an answer cut off for room.
const NO_ROOM: &str = "the model has no room left to answer, even \
                       with the conversation summarised. Start a \
                       new session, or choose a model that holds \
                       more";
const NO_SUMMARY: &str = "the conversation is longer than the model \
                          can hold, and the model could not \
                          summarise it. Start a new session, or \
                          choose a model that holds more";
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
    abort: AbortHandle,
}

pub(super) struct Preparing {
    binding: BindingId,
    abort: AbortHandle,
}

pub(crate) struct PreparedCall {
    id: uuid::Uuid,
    binding: BindingId,
    re: Re,
    call: Call,
    signature: Value,
    result: Preparation,
}

enum Preparation {
    Prompt(Result<Vec<Section>>),
    Preview(std::result::Result<Option<String>, String>),
}

pub(crate) async fn prepared(
    service: &mut Service,
    harnesses: &mut Harnesses,
    ready: PreparedCall,
) -> Result<()> {
    if service.chats.preparations.remove(&ready.id).is_none() {
        return Ok(());
    }
    let input = match &ready.call {
        Call::Allow { input, .. }
        | Call::Operation { input, .. }
        | Call::Prompt { input }
        | Call::Yield { input, .. } => input.as_deref(),
    };
    if service.home.store().cancelling(ready.binding)? {
        harnesses
            .answer(
                ready.re,
                Answer::Refusal(
                    crate::harness::answers::STOPPED_WORDS.into(),
                ),
            )
            .await;
        return Ok(());
    }
    if crate::society::reviews::held(service, ready.binding, input)? {
        harnesses
            .answer(
                ready.re,
                Answer::Waiting(
                    "The chief is considering an objection.".into(),
                ),
            )
            .await;
        return Ok(());
    }
    match ready.result {
        Preparation::Prompt(result) => {
            if prompt(service, ready.binding, input)?.signature()
                != ready.signature
            {
                return call(
                    service,
                    harnesses,
                    ready.binding,
                    ready.re,
                    ready.call,
                )
                .await;
            }
            let mut sections = result?;
            for section in &mut sections {
                let mut text =
                    Value::String(std::mem::take(&mut section.text));
                service.credentials.scrub(&mut text);
                section.text =
                    text.as_str().unwrap_or_default().into();
            }
            harnesses
                .answer(ready.re, Answer::Sections(sections))
                .await;
        }
        Preparation::Preview(result) => {
            let Some(bound) =
                service.home.store().binding(ready.binding)?
            else {
                return Ok(());
            };
            if service.work(&bound.agent.hamlet)? != ready.signature {
                return call(
                    service,
                    harnesses,
                    ready.binding,
                    ready.re,
                    ready.call,
                )
                .await;
            }
            call_prepared(
                service,
                harnesses,
                ready.binding,
                ready.re,
                ready.call,
                Some(result),
            )
            .await?;
        }
    }
    Ok(())
}

/// The tools the server adds to every conversation's own.
pub(crate) fn specs() -> Vec<ToolSpec> {
    let parameters = json!({
        "type": "object",
        "properties": {
            "message": {"type": "string"},
            "question": {"type": "string"},
            "questions": {"type": "array", "minItems": 1, "maxItems": asking::MOST_QUESTIONS,
                "items": {"type": "object", "properties": {
                    "question": {"type": "string"},
                    "description": {"type": "string"},
                    "options": {"type": "array", "maxItems": asking::MOST_OPTIONS,
                        "items": {"anyOf": [{"type": "string"}, {"type": "object", "properties": {
                            "title": {"type": "string"}, "description": {"type": "string"}},
                            "required": ["title"]}]}},
                    "text_input": {"type": "boolean", "description": "Allow custom words. Defaults to true."}},
                    "required": ["question"]}}
        },
        "anyOf": [{"required": ["questions"]}, {"required": ["question"]}],
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
    call_prepared(service, harnesses, binding, re, call, None).await
}

async fn call_prepared(
    service: &mut Service,
    harnesses: &mut Harnesses,
    binding: BindingId,
    re: Re,
    call: Call,
    preview: Option<std::result::Result<Option<String>, String>>,
) -> Result<()> {
    let input = match &call {
        Call::Allow { input, .. }
        | Call::Operation { input, .. }
        | Call::Prompt { input }
        | Call::Yield { input, .. } => input.clone(),
    };
    if !matches!(call, Call::Yield { .. })
        && let Some(bound) = service.home.store().binding(binding)?
        && !bound.errand
        && service.merges.held(&bound.agent)
    {
        if matches!(call, Call::Prompt { .. }) {
            harnesses.cancel(service.home.store(), binding).await?;
        }
        harnesses
            .answer(
                re,
                Answer::Refusal(
                    "This house is being merged or split. Wait for \
                     its files to finish, then ask the chief to \
                     send a new instruction."
                        .into(),
                ),
            )
            .await;
        return Ok(());
    }
    if input.is_some() {
        super::conversation(service, binding)?
            .turn
            .clone_from(&input);
    }
    if matches!(call, Call::Prompt { .. }) {
        crate::society::reviews::prepare(
            service,
            harnesses,
            binding,
            input.as_deref(),
        )
        .await?;
    }
    let held_step = match &call {
        Call::Allow { tool, .. } => tool != "object",
        Call::Prompt { .. } => true,
        Call::Operation { .. } | Call::Yield { .. } => false,
    };
    if held_step
        && crate::society::reviews::held(
            service,
            binding,
            input.as_deref(),
        )?
    {
        harnesses
            .answer(
                re,
                Answer::Waiting(
                    "The chief is considering an objection.".into(),
                ),
            )
            .await;
        return Ok(());
    }
    if let Call::Prompt { .. } = &call {
        let prepared = prompt(service, binding, input.as_deref())?;
        let signature = prepared.signature();
        let id = uuid::Uuid::new_v4();
        let abort = service.chats.preparing.spawn(async move {
            PreparedCall {
                id,
                binding,
                re,
                call,
                signature,
                result: Preparation::Prompt(prepared.render().await),
            }
        });
        service
            .chats
            .preparations
            .insert(id, Preparing { binding, abort });
        return Ok(());
    }
    if preview.is_none()
        && let Call::Allow {
            key,
            tool,
            arguments,
            ..
        }
        | Call::Operation {
            key,
            tool,
            arguments,
            ..
        } = &call
        && tool == "write"
        && let Some(bound) = service.home.store().binding(binding)?
        && same_request(
            service,
            bound.runtime.as_str(),
            key,
            binding,
            input.as_deref(),
            tool,
            arguments,
        )?
        && kept(service, bound.runtime.as_str(), key)?.is_none()
        && !service.home.store().cancelling(binding)?
        && let Some(remote) =
            service.remote_tools(&bound.agent, false)?
        && let Some(path) = arguments
            .get("path")
            .and_then(Value::as_str)
            .map(String::from)
    {
        let signature = service.work(&bound.agent.hamlet)?;
        let id = uuid::Uuid::new_v4();
        let abort = service.chats.preparing.spawn(async move {
            let result = async {
                let full = remote
                    .footing(&path)
                    .await
                    .map_err(|e| e.message)?;
                Ok(match remote.read(&full, false).await {
                    Ok(bytes) => Some(
                        String::from_utf8_lossy(&bytes).into_owned(),
                    ),
                    Err(error) if error.code == "not_found" => None,
                    Err(_) => Some(
                        "[a file that could not be read as text]"
                            .into(),
                    ),
                })
            }
            .await;
            PreparedCall {
                id,
                binding,
                re,
                call,
                signature,
                result: Preparation::Preview(result),
            }
        });
        service
            .chats
            .preparations
            .insert(id, Preparing { binding, abort });
        return Ok(());
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
                preview.as_ref(),
            )
            .await?
            {
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
                preview.as_ref(),
            )
            .await?
            {
                Step::Answer(answer) => answer,
                Step::Running => return Ok(()),
                Step::Society(bound) => {
                    crate::society::operation(
                        service,
                        harnesses,
                        &bound,
                        &key,
                        input.as_deref(),
                        &tool,
                        &arguments,
                    )
                    .await?
                }
            }
        }
        Call::Prompt { .. } => unreachable!("prompt prepared above"),
        Call::Yield {
            answer,
            history,
            retried,
            summary_failure,
            ..
        } => verdict(
            service,
            binding,
            input.as_deref(),
            &answer,
            history,
            retried,
            summary_failure.as_deref(),
        )?,
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
async fn decide(
    service: &mut Service,
    binding: BindingId,
    key: &str,
    input: Option<&str>,
    tool: &str,
    arguments: &BTreeMap<String, Value>,
    preview: Option<&std::result::Result<Option<String>, String>>,
) -> Result<Decision> {
    let Some(bound) = service.home.store().binding(binding)? else {
        return Ok(Decision::Block(NOBODY.into()));
    };
    let runtime = bound.runtime.as_str().to_owned();
    if !same_request(
        service, &runtime, key, binding, input, tool, arguments,
    )? {
        return Ok(Decision::Block(IDENTITY_REFUSAL.into()));
    }
    if crate::society::withheld(service, binding, tool)? {
        return Ok(Decision::Run { net: false });
    }
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
    if let Some(manifest) =
        crate::hand::load(service.home.layout().root())
            .0
            .into_iter()
            .find(|manifest| manifest.spec.name == tool)
    {
        call = gate::Call::Hand {
            name: tool.into(),
            args: args.clone(),
            net: manifest.net(),
        };
    }
    let at = places(service, &place.agent)?;
    let remote = service.remote_tools(&place.agent, false)?;
    if let gate::Call::Edit {
        write: true,
        path,
        old,
        created,
        ..
    } = &mut call
    {
        let previous = if remote.is_some() {
            match preview {
                Some(Ok(before)) => before.clone(),
                Some(Err(words)) => {
                    return Ok(Decision::Block(words.clone()));
                }
                None => {
                    return Ok(Decision::Block(
                        "The remote edit preview is not available."
                            .into(),
                    ));
                }
            }
        } else {
            tools::footing(path, &at)
                .ok()
                .and_then(|full| before(service, &at, &full))
        };
        if let Some(previous) = previous {
            *old = previous;
            *created = false;
        }
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
        && remote.is_none()
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
            questions: None,
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
            "SELECT net, block, wait, how, text, EXISTS (SELECT 1 \
             FROM marker WHERE marker.runtime = gate_call.runtime \
             AND marker.key = gate_call.key) FROM gate_call WHERE \
             runtime = ?1 AND key = ?2",
            params![runtime, key],
            |row| {
                Ok((
                    row.get::<_, Option<bool>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, bool>(5)?,
                ))
            },
        )
        .optional()?;
    let Some((net, block, wait, how, text, marked)) = row else {
        return Ok(None);
    };
    if marked {
        return Ok(Some(Decision::Run {
            net: net.unwrap_or(false),
        }));
    }
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
    // Automatic allowances depend on the current grants and session
    // trust. Only a chief's ruling above stands until execution.
    Ok(None)
}

/// Reserves an identity before the early allowance or the effect.
/// A legacy call or orphaned marker has no verifiable request.
fn same_request(
    service: &mut Service,
    runtime: &str,
    key: &str,
    binding: BindingId,
    input: Option<&str>,
    tool: &str,
    arguments: &BTreeMap<String, Value>,
) -> Result<bool> {
    let mut canonical = String::new();
    crate::operation::canonical(
        &json!([input, tool, arguments]),
        &mut canonical,
    );
    let digest = Sha256::digest(canonical).to_vec();
    let db = service.home.store().connection();
    db.execute(
        "INSERT INTO gate_call (runtime, key, binding, digest) \
         SELECT ?1, ?2, ?3, ?4 WHERE NOT EXISTS (SELECT 1 FROM \
         marker WHERE runtime = ?1 AND key = ?2) ON CONFLICT DO \
         NOTHING",
        params![runtime, key, binding.0, digest],
    )?;
    Ok(db
        .query_row(
            "SELECT binding, digest FROM gate_call WHERE runtime = \
             ?1 AND key = ?2",
            params![runtime, key],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<Vec<u8>>>(1)?,
                ))
            },
        )
        .optional()?
        .is_some_and(|(kept_binding, kept_digest)| {
            kept_binding == binding.0
                && kept_digest.as_ref() == Some(&digest)
        }))
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
        "UPDATE gate_call SET input = ?4, net = ?5, block = ?6, \
         change = ?7 WHERE runtime = ?1 AND key = ?2 AND binding = \
         ?3",
        params![runtime, key, binding.0, input, net, block, change],
    )?;
    Ok(())
}

/// The text of the file at `path` as a write's preview shows it, or
/// `None` when there is none. It is read as the agent at `at` may,
/// so the preview shows the chief nothing the agent could not read
/// itself.
fn before(
    service: &Service,
    at: &tools::Place,
    path: &std::path::Path,
) -> Option<String> {
    let path = crate::sandbox::path::text(path);
    let Ok(env) =
        tools::environment(&service.chats.sandbox, at, false)
    else {
        return Some(
            "[a file that could not be read as text]".into(),
        );
    };
    match env.read_text(&path) {
        Ok(text) => {
            let mut text = Value::String(text);
            service.credentials.scrub(&mut text);
            text.as_str().map(String::from)
        }
        Err(error) if error.code == "not_found" => None,
        Err(error) => {
            let size = env
                .file_info(&path)
                .map(|info| format!(", {} bytes", info.size))
                .unwrap_or_default();
            let reason = if error.code == "invalid" {
                "is not UTF-8 text"
            } else {
                "could not be read as text"
            };
            Some(format!("[a file that {reason}{size}]"))
        }
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

/// What carrying out a call comes to.
enum Step {
    Answer(Answer),
    /// The call runs, and is answered when it ends.
    Running,
    /// The gate let a society call run, which the society carries
    /// out.
    Society(crate::harness::Binding),
}

/// Carries out the call `key`: its marked result when it ran, else as
/// the gate decides now.
async fn operation(
    service: &mut Service,
    binding: BindingId,
    re: &Re,
    key: &str,
    input: Option<&str>,
    tool: &str,
    arguments: &BTreeMap<String, Value>,
    preview: Option<&std::result::Result<Option<String>, String>>,
) -> Result<Step> {
    let Some(bound) = service.home.store().binding(binding)? else {
        return Ok(Step::Answer(Answer::result(NOBODY, true)));
    };
    let runtime = bound.runtime.as_str().to_owned();
    if !same_request(
        service, &runtime, key, binding, input, tool, arguments,
    )? {
        return Ok(Step::Answer(Answer::Refusal(
            IDENTITY_REFUSAL.into(),
        )));
    }
    let id = (runtime.clone(), key.to_owned());
    if let Some(running) = service.chats.running.get_mut(&id) {
        running.res.push(re.clone());
        return Ok(Step::Running);
    }
    if service.house_work.pending.contains(&id) {
        return Ok(Step::Answer(Answer::Waiting(
            "The house work is still running.".into(),
        )));
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
        return Ok(Step::Answer(if outcome.is_empty() {
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
    if tool != "object"
        && crate::society::reviews::held(service, binding, input)?
    {
        return Ok(Step::Answer(Answer::Waiting(
            "The chief is considering an objection.".into(),
        )));
    }
    if crate::society::withheld(service, binding, tool)? {
        let outcome = crate::society::unavailable(tool);
        service.home.store().connection().execute(
            "INSERT INTO marker (runtime, key, kind, digest, \
             outcome) VALUES (?1, ?2, 'tool', ?3, ?4) ON CONFLICT \
             DO NOTHING",
            params![
                runtime,
                key,
                tool.as_bytes(),
                outcome.to_string()
            ],
        )?;
        return Ok(Step::Answer(result(outcome)));
    }
    if tool == "object" {
        return Ok(Step::Society(bound));
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
        .map(Step::Answer);
    }
    let net = match decide(
        service, binding, key, input, tool, arguments, preview,
    )
    .await?
    {
        Decision::Run { net } => net,
        Decision::Block(words) => {
            return Ok(Step::Answer(Answer::Refusal(words)));
        }
        Decision::Wait => {
            return Ok(Step::Answer(Answer::Waiting(WAITING.into())));
        }
    };
    if tool != "build_road" && crate::gate::SOCIETY.contains(&tool) {
        return Ok(Step::Society(bound));
    }
    if tool == "build_road" {
        let call = match crate::harness::tools::parse(tool, arguments)
        {
            Ok(call) => call,
            Err(words) => {
                return Ok(Step::Answer(Answer::result(words, true)));
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
        return Ok(Step::Answer(Answer::result(text, error)));
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
    if UNSAFE.contains(&tool)
        || crate::hand::load(service.home.layout().root())
            .0
            .iter()
            .any(|manifest| manifest.spec.name == tool)
    {
        service.home.store().connection().execute(
            "INSERT INTO marker (runtime, key, kind, digest, \
             outcome) VALUES (?1, ?2, 'tool', ?3, '') ON CONFLICT \
             DO NOTHING",
            params![runtime, key, tool.as_bytes()],
        )?;
    }
    let place = places(service, &bound.agent)?;
    let remote = service.remote_tools(&bound.agent, net)?;
    let sandbox = service.chats.sandbox;
    let secrets = service.credentials.secrets();
    let arguments =
        arguments.clone().into_iter().collect::<Map<_, _>>();
    let name = tool.to_owned();
    let (task_runtime, task_key) = id.clone();
    let abort = service.chats.tools.spawn(async move {
        let mut outcome = if let Some(remote) = &remote {
            tools::run_remote(
                remote, &sandbox, &place, net, &secrets, &name,
                &arguments,
            )
            .await
        } else {
            tools::run(
                &sandbox, &place, net, &secrets, &name, &arguments,
            )
            .await
        };
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
            abort,
        },
    );
    Ok(Step::Running)
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
    service.home.store().connection().execute(
        "UPDATE marker SET outcome = ?3 WHERE runtime = ?1 AND key \
         = ?2 AND kind = 'tool'",
        params![ran.runtime, ran.key, outcome.to_string()],
    )?;
    let _ = running.binding;
    for re in running.res {
        harnesses.answer(re, result(outcome.clone())).await;
    }
    Ok(())
}

/// Stops the tool calls running for `binding`.
pub(crate) fn stop_tools(service: &mut Service, binding: BindingId) {
    service.chats.preparations.retain(|_, preparing| {
        if preparing.binding == binding {
            preparing.abort.abort();
            false
        } else {
            true
        }
    });
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
    if service.home.store().cancelling(binding)? {
        return Ok(Answer::Refusal(
            crate::harness::answers::STOPPED_WORDS.into(),
        ));
    }
    let answered = |text: &str| Ok(Answer::result(text, false));
    let db = service.home.store().connection();
    let row = db
        .query_row(
            "SELECT wait, how, text, block, (SELECT questions IS \
             NOT NULL FROM wait WHERE id = gate_call.wait) FROM \
             gate_call WHERE runtime = ?1 AND key = ?2",
            params![runtime.as_str(), key],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<bool>>(4)?.unwrap_or(false),
                ))
            },
        )
        .optional()?;
    if let Some((_, _, _, Some(words), _)) = &row {
        return Ok(Answer::Refusal(words.clone()));
    }
    // A row with no wait is the gate's allowing, before the question.
    if let Some((Some(_), how, text, _, structured)) = &row {
        match how.as_deref() {
            Some("reply") => {
                if *structured {
                    return answered(
                        text.as_deref().unwrap_or_default(),
                    );
                }
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
    let questions =
        match asking::read(&serde_json::to_string(arguments)?) {
            Ok(questions) => questions,
            Err(why) => return answered(&why),
        };
    let mut questions = serde_json::to_value(questions)?;
    service.credentials.scrub(&mut questions);
    let questions =
        serde_json::from_value::<asking::Questions>(questions)?;
    let limits = service.limits_of(&place.agent.hamlet)?;
    let most = limits["asking"]["questions"].as_i64().unwrap_or(3);
    let db = service.home.store().connection();
    let mut asked = Asked::of(db, binding, input)?;
    if asked.questions >= most {
        let times = if most == 1 {
            "once".into()
        } else {
            format!("{most} times")
        };
        return answered(&format!(
            "You have asked the chief {times} this turn; go on with \
             what you know."
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
            about: questions.about(),
            change: None,
            questions: Some(questions),
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
) -> Result<crate::prompt::Prepared> {
    let Some(bound) = service.home.store().binding(binding)? else {
        return Ok(crate::prompt::Prepared::fixed(Vec::new()));
    };
    if bound.errand {
        let text = service
            .home
            .store()
            .connection()
            .query_row(
                "SELECT instructions FROM society_binding WHERE \
                 binding=?1",
                [binding.0],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        return Ok(crate::prompt::Prepared::fixed(
            text.map(|text| {
                vec![Section {
                    name: "errand".into(),
                    text,
                }]
            })
            .unwrap_or_default(),
        ));
    }
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
        return Ok(crate::prompt::Prepared::fixed(Vec::new()));
    };
    let Some(member) = record
        .agents
        .iter()
        .find(|a| a.number == agent.number)
        .cloned()
    else {
        return Ok(crate::prompt::Prepared::fixed(Vec::new()));
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
        conversation: Some(binding),
    };
    let mut prepared = crate::prompt::prepare(service, &subject)?;
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
    prepared.sections.push(Section {
        name: "budget".into(),
        text: budget,
    });
    Ok(prepared)
}

/// Whether a final answer stands: one with no words and no call
/// fails, and its turn ends (C28).
fn verdict(
    service: &mut Service,
    binding: BindingId,
    input: Option<&str>,
    answer: &Item,
    history: bool,
    retried: bool,
    summary_failure: Option<&str>,
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
            compact: false,
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
    if cutoff && history && !retried && summary_failure.is_none() {
        return Ok(Answer::Verdict {
            words: None,
            notice: None,
            compact: true,
        });
    }
    let words = if let Some(error) = summary_failure {
        if error.starts_with("Summarization failed:") {
            format!(
                "provider error: {}",
                error.trim_start_matches("Summarization failed: ")
            )
        } else {
            NO_SUMMARY.into()
        }
    } else if cutoff && !history && !retried {
        "the message is longer than the model can hold".into()
    } else if cutoff {
        NO_ROOM.into()
    } else {
        EMPTY.into()
    };
    if let Some(input) = input {
        follow::fail_answer(db, binding, input, &words)?;
    }
    Ok(Answer::Verdict {
        notice: Some(super::notice(&words)),
        words: Some(words),
        compact: false,
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
    picks: Option<&[hamlet_protocol::v0::Pick]>,
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
    let text = if let Some(picks) = picks {
        if choice != "reply" || !text.is_empty() {
            return Ok(Err("Use picks only for a reply, without \
                           text."
                .into()));
        }
        let Some(questions) = &wait.questions else {
            return Ok(Err("This wait does not take structured \
                           answers."
                .into()));
        };
        let picks = match picks
            .iter()
            .cloned()
            .map(Pick::try_from)
            .collect::<std::result::Result<Vec<_>, _>>()
        {
            Ok(picks) => picks,
            Err(why) => return Ok(Err(why)),
        };
        match questions.answer(&picks) {
            Ok(answer) => answer,
            Err(why) => return Ok(Err(why)),
        }
    } else if choice == "reply" {
        if text.trim().is_empty() {
            return Ok(Err("Say what to reply.".into()));
        }
        if let Some(questions) = &wait.questions {
            match questions.answer(&[Pick::Words(text.into())]) {
                Ok(_) => {
                    format!("{} {}", asking::ANSWERS, text.trim())
                }
                Err(why) => return Ok(Err(why)),
            }
        } else {
            text.to_owned()
        }
    } else {
        text.to_owned()
    };
    let mut words = Value::String(text);
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
    let tx = service.home.store().db.transaction()?;
    let Some((settled, mut woken)) =
        oversight::settle(&tx, id, choice, &text, Some(&chief), now)?
    else {
        return Ok(Ok("Nothing waits for you.".into()));
    };
    if settled.kind == "objection" {
        woken.extend(crate::society::reviews::settle(
            &tx, id, choice, &text, &chief,
        )?);
    }
    tx.commit()?;
    if settled.kind == "objection" && choice == "withdraw" {
        crate::fault::point("after_instruction_withdrawal");
    }
    crate::society::reviews::sweep(service, harnesses).await?;
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

#[cfg(test)]
mod tests {
    use hamlet_testing::Scratch;

    use super::*;
    use crate::harness::link::ModelRef;

    fn decide(
        service: &mut Service,
        binding: BindingId,
        key: &str,
        input: Option<&str>,
        tool: &str,
        arguments: &BTreeMap<String, Value>,
    ) -> Result<Decision> {
        futures::executor::block_on(super::decide(
            service, binding, key, input, tool, arguments, None,
        ))
    }

    /// A service on a scratch home, with the founder's conversation
    /// bound to a runtime.
    fn served(scratch: &Scratch) -> (Service, BindingId, String) {
        let home = crate::Home::open(&scratch.join("home")).unwrap();
        let mut service = Service::new(home, true).unwrap();
        let agent = AgentRef {
            hamlet: "home".into(),
            number: 0,
        };
        let store = service.home.store();
        let runtime = store.runtime_for("pi").unwrap();
        let model = ModelRef {
            provider: "faux".into(),
            id: "faux-1".into(),
            base_url: None,
        };
        let binding = store
            .add_binding(&agent, false, &model, &runtime)
            .unwrap();
        let place = Place {
            agent,
            chief: Some(service.chief.clone()),
            session: 1,
        };
        records::keep(store.connection(), binding, &place, None, 0)
            .unwrap();
        (service, binding, runtime.as_str().to_owned())
    }

    #[test]
    fn a_write_s_preview_shows_only_what_the_agent_may_read() {
        let scratch = Scratch::new("calls-preview");
        let (mut service, _, _) = served(&scratch);
        let root = service.home.layout().root().to_owned();
        std::fs::write(root.join("private.txt"), "the chief's own\n")
            .unwrap();
        let agent = AgentRef {
            hamlet: "home".into(),
            number: 0,
        };
        let at = places(&mut service, &agent).unwrap();
        std::fs::write(at.house.join("NOTES.md"), "# Notes\n")
            .unwrap();

        assert_eq!(
            before(&service, &at, &root.join("private.txt"))
                .as_deref(),
            Some("[a file that could not be read as text]")
        );
        assert_eq!(
            before(&service, &at, &at.house.join("NOTES.md"))
                .as_deref(),
            Some("# Notes\n")
        );
        assert_eq!(
            before(&service, &at, &at.house.join("new.md")),
            None
        );
    }

    #[test]
    fn a_gate_identity_cannot_authorize_a_changed_request() {
        let scratch = Scratch::new("calls-identity");
        let (mut service, binding, _) = served(&scratch);
        let arguments = BTreeMap::from([(
            "command".to_owned(),
            Value::String("echo harmless".into()),
        )]);
        assert!(matches!(
            decide(
                &mut service,
                binding,
                "same-key",
                None,
                "bash",
                &arguments
            )
            .unwrap(),
            Decision::Run { .. }
        ));
        let changed = BTreeMap::from([(
            "command".to_owned(),
            Value::String("rm -rf important".into()),
        )]);
        assert!(
            matches!(
                decide(
                    &mut service,
                    binding,
                    "same-key",
                    None,
                    "bash",
                    &changed
                )
                .unwrap(),
                Decision::Block(_)
            ),
            "a different command reused an earlier allowance"
        );
        assert!(matches!(
            decide(
                &mut service,
                binding,
                "same-key",
                None,
                "write",
                &arguments
            )
            .unwrap(),
            Decision::Block(_)
        ));
        let bound =
            service.home.store().binding(binding).unwrap().unwrap();
        let other = service
            .home
            .store()
            .add_binding(
                &bound.agent,
                false,
                &bound.model,
                &bound.runtime,
            )
            .unwrap();
        assert!(matches!(
            decide(
                &mut service,
                other,
                "same-key",
                None,
                "bash",
                &arguments
            )
            .unwrap(),
            Decision::Block(_)
        ));
        service.home.close().unwrap();
        let mut service = Service::new(
            crate::Home::open(&scratch.join("home")).unwrap(),
            true,
        )
        .unwrap();
        service
            .home
            .store()
            .connection()
            .execute(
                "INSERT INTO marker \
                 (runtime,key,kind,digest,outcome) VALUES \
                 (?1,'same-key','tool',?2,?3)",
                params![
                    bound.runtime.as_str(),
                    b"bash".as_slice(),
                    r#"{"text":"done","error":false}"#
                ],
            )
            .unwrap();
        service.home.store().begin_cancel(binding).unwrap();
        assert!(matches!(
            decide(
                &mut service,
                binding,
                "same-key",
                None,
                "bash",
                &arguments
            )
            .unwrap(),
            Decision::Run { .. }
        ));
        assert!(matches!(
            decide(
                &mut service,
                binding,
                "same-key",
                Some("different-input"),
                "bash",
                &arguments
            )
            .unwrap(),
            Decision::Block(_)
        ));
        assert!(matches!(
            decide(
                &mut service,
                binding,
                "same-key",
                None,
                "bash",
                &changed
            )
            .unwrap(),
            Decision::Block(_)
        ));
    }

    #[test]
    fn a_grant_taken_back_before_execution_does_not_authorize_the_call()
     {
        let scratch = Scratch::new("calls-revoked");
        let (mut service, binding, _) = served(&scratch);
        let agent = AgentRef {
            hamlet: "home".into(),
            number: 0,
        };
        gate::give(
            service.home.store().connection(),
            &agent,
            "mayor",
            "network",
            "chief",
            0,
        )
        .unwrap();
        let args = BTreeMap::from([
            ("command".to_owned(), json!("echo harmless")),
            ("network".to_owned(), json!(true)),
        ]);
        assert!(matches!(
            decide(&mut service, binding, "k", None, "bash", &args)
                .unwrap(),
            Decision::Run { net: true }
        ));
        gate::revoke(
            service.home.store().connection(),
            &agent,
            "network",
            "chief",
            1,
        )
        .unwrap();
        assert!(
            matches!(
                decide(
                    &mut service,
                    binding,
                    "k",
                    None,
                    "bash",
                    &args
                )
                .unwrap(),
                Decision::Wait
            ),
            "a revoked grant still allowed an unexecuted call"
        );
    }

    #[test]
    fn upgrading_old_gate_rows_never_invents_a_request_or_replays_an_effect()
     {
        let scratch = Scratch::new("calls-upgrade");
        let (mut service, binding, runtime) = served(&scratch);
        let args = BTreeMap::from([(
            "command".to_owned(),
            json!("echo harmless"),
        )]);
        for key in ["allowed", "finished"] {
            assert!(matches!(
                decide(
                    &mut service,
                    binding,
                    key,
                    None,
                    "bash",
                    &args
                )
                .unwrap(),
                Decision::Run { .. }
            ));
        }
        let db = service.home.store().connection();
        db.execute(
            "INSERT INTO marker (runtime,key,kind,digest,outcome) \
             VALUES (?1,'finished','tool',?2,?3)",
            params![runtime, b"bash".as_slice(), "kept outcome"],
        )
        .unwrap();
        // Reconstruct the version-8 schema on disposable data.
        db.execute_batch(
            "ALTER TABLE wait DROP COLUMN questions; ALTER TABLE \
             gate_call DROP COLUMN digest; PRAGMA user_version = 8;",
        )
        .unwrap();
        service.home.close().unwrap();
        let mut service = Service::new(
            crate::Home::open(&scratch.join("home")).unwrap(),
            true,
        )
        .unwrap();
        for key in ["allowed", "finished"] {
            assert!(matches!(
                decide(
                    &mut service,
                    binding,
                    key,
                    None,
                    "bash",
                    &args
                )
                .unwrap(),
                Decision::Block(_)
            ));
        }
        let db = service.home.store().connection();
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            10
        );
        assert_eq!(
            db.query_row(
                "SELECT outcome FROM marker WHERE runtime = ?1 AND \
                 key = 'finished'",
                [&runtime],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
            "kept outcome"
        );
        assert!(matches!(
            decide(&mut service, binding, "new", None, "bash", &args)
                .unwrap(),
            Decision::Run { .. }
        ));
    }

    #[test]
    fn a_call_allowed_before_a_stop_runs_nothing_after_it() {
        let scratch = Scratch::new("calls-stopped");
        let (mut service, binding, runtime) = served(&scratch);
        let arguments = BTreeMap::from([(
            "command".to_owned(),
            Value::String("echo hi".into()),
        )]);
        let allowed = decide(
            &mut service,
            binding,
            "call-1",
            Some("input-1"),
            "bash",
            &arguments,
        )
        .unwrap();
        assert!(matches!(allowed, Decision::Run { .. }));

        let question =
            BTreeMap::from([("question".into(), json!("Continue?"))]);
        assert!(matches!(
            decide(
                &mut service,
                binding,
                "question-1",
                Some("input-1"),
                "ask_chief",
                &question
            )
            .unwrap(),
            Decision::Run { .. }
        ));
        service.home.store().begin_cancel(binding).unwrap();
        let refused = decide(
            &mut service,
            binding,
            "call-1",
            Some("input-1"),
            "bash",
            &arguments,
        )
        .unwrap();
        assert!(
            matches!(&refused, Decision::Block(words) if words == crate::harness::answers::STOPPED_WORDS),
            "a call allowed before the stop still runs after it"
        );

        // The refusal stands once the runtime has acknowledged the
        // cancel, as the result of a call that ran would.
        service.home.store().cancelled(binding).unwrap();
        let still = kept(&mut service, &runtime, "call-1").unwrap();
        assert!(matches!(still, Some(Decision::Block(_))));
        let runtime = crate::RuntimeInstance::new(runtime).unwrap();
        assert!(
            matches!(
                ask(
                    &mut service,
                    binding,
                    &runtime,
                    "question-1",
                    Some("input-1"),
                    &question
                )
                .unwrap(),
                Answer::Refusal(_)
            ),
            "a stopped question bypassed the gate's refusal"
        );
    }
}
