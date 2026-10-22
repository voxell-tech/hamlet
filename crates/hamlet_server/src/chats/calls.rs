//! What a runtime asks of the server while a turn runs: whether a
//! tool call may run (C7, H11), the call carried out, the prompt, and
//! whether a final answer stands (C28). And a chief's answers to what
//! waits.

mod answer;
mod ask;
mod decide;
mod tasks;

use std::collections::BTreeMap;

use anyhow::Result;
use hamlet_protocol::runtime::{
    Block, Item, Section, ToolSpec, ToolSpecReplay,
};
use serde_json::{Map, Value, json};

pub(super) use self::answer::answer_words;
use self::decide::{Decision, Gate, Reading, Unadmitted};
use self::tasks::Prepared;
pub(crate) use self::tasks::{Finished, Tasks};
use super::{asking, follow, records};
use crate::harness::tools::CONFLICT;
use crate::harness::{
    Answer, Binding, BindingId, Call, Harnesses, InputMark, Re,
};
use crate::operation::{self, Marked, Request};
use crate::service::Service;
use crate::tools;

/// The words of a call that waits for a chief.
const WAITING: &str = "This call waits for a chief's answer.";
/// The words of a call held while the chief considers an objection.
const OBJECTING: &str = "The chief is considering an objection.";
/// The words of a call that nobody may approve.
const NOBODY: &str = "nobody is here to approve it; it did not run";
const IDENTITY_REFUSAL: &str = "This call's identity does not match \
                                a verified request, so it did not \
                                run.";
/// The words of a call whose preparation failed.
const UNPREPARED: &str =
    "The server could not prepare this call, so it did not run.";
/// What a preview shows of a file it cannot show as text.
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

/// What a call's preparation read.
pub(crate) enum Preparation {
    Prompt(Result<Vec<Section>>),
    Preview(std::result::Result<Option<String>, String>),
}

/// Answers the calls of a task that ended.
pub(crate) async fn finished(
    service: &mut Service,
    harnesses: &mut Harnesses,
    finished: Finished,
) -> Result<()> {
    match finished {
        Finished::Ran { id, res, outcome } => {
            // Kept before any answer, as the call may not run again.
            operation::settle(
                service.home.store().connection(),
                &id,
                &outcome,
            )?;
            for re in res {
                harnesses.answer(re, outcome.clone().into()).await;
            }
        }
        Finished::Prepared(prepared) => {
            self::prepared(service, harnesses, prepared).await?;
        }
        Finished::Unprepared { re } => {
            harnesses
                .answer(re, Answer::Refusal(UNPREPARED.into()))
                .await;
        }
    }
    Ok(())
}

/// Answers a prepared call, unless what it read changed meanwhile.
async fn prepared(
    service: &mut Service,
    harnesses: &mut Harnesses,
    ready: Prepared,
) -> Result<()> {
    let Prepared {
        binding,
        re,
        call,
        signature,
        result,
    } = ready;
    match result {
        Preparation::Prompt(result) => {
            let input = call.input();
            if service.home.store().cancelling(binding)? {
                harnesses
                    .answer(
                        re,
                        Answer::Refusal(
                            crate::harness::answers::STOPPED_WORDS
                                .into(),
                        ),
                    )
                    .await;
                return Ok(());
            }
            if objecting(service, binding, input)? {
                harnesses
                    .answer(re, Answer::Waiting(OBJECTING.into()))
                    .await;
                return Ok(());
            }
            if prompt(service, binding, input)?.signature()
                != signature
            {
                return self::call(
                    service, harnesses, binding, re, call,
                )
                .await;
            }
            let mut sections = result?;
            for section in &mut sections {
                section.text =
                    service.credentials.scrub_text(&section.text);
            }
            harnesses.answer(re, Answer::Sections(sections)).await;
        }
        Preparation::Preview(result) => {
            let Some(bound) =
                service.home.store().binding(binding)?
            else {
                return Ok(());
            };
            if serde_json::to_value(
                service.work(&bound.agent.hamlet)?,
            )? != signature
            {
                return self::call(
                    service, harnesses, binding, re, call,
                )
                .await;
            }
            call_prepared(
                service,
                harnesses,
                binding,
                re,
                call,
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
        "required": ["questions"],
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
    if merging(service, binding, &call)? {
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
    let input = call.input().map(String::from);
    if input.is_some() {
        super::conversation(service, binding)?
            .turn
            .clone_from(&input);
    }
    let input = input.as_deref();
    if let Call::Prompt { .. } = call {
        return prompt_call(service, harnesses, binding, re, call)
            .await;
    }
    let answer = match &call {
        Call::Allow {
            key,
            tool,
            arguments,
            ..
        } => {
            match decide::allow(
                service,
                binding,
                key,
                input,
                tool,
                arguments,
                preview.as_ref(),
            )? {
                Gate::Decided(Decision::Run { .. }) => {
                    Answer::Allowed
                }
                Gate::Decided(Decision::Block(words)) => {
                    Answer::Refusal(words)
                }
                Gate::Decided(Decision::Wait(words)) => {
                    Answer::Waiting(words.into())
                }
                Gate::Preview(reading) => {
                    preview_call(service, binding, re, call, reading);
                    return Ok(());
                }
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
                key,
                input,
                tool,
                arguments,
                preview.as_ref(),
            )? {
                Step::Answer(answer) => answer,
                Step::Running => return Ok(()),
                Step::Preview(reading) => {
                    preview_call(service, binding, re, call, reading);
                    return Ok(());
                }
                Step::Society(bound) => {
                    crate::society::operation(
                        service, harnesses, &bound, key, input, tool,
                        arguments,
                    )
                    .await?
                }
            }
        }
        Call::Prompt { .. } => unreachable!("prompts are prepared"),
        Call::Yield {
            answer,
            history,
            retried,
            summary_failure,
            ..
        } => verdict(
            service,
            binding,
            input,
            answer,
            *history,
            *retried,
            summary_failure.as_deref(),
        )?,
    };
    harnesses.answer(re, answer).await;
    Ok(())
}

/// Whether the house of `binding`'s agent is being merged or split,
/// which holds every call of its own work but a final answer.
fn merging(
    service: &mut Service,
    binding: BindingId,
    call: &Call,
) -> Result<bool> {
    Ok(!matches!(call, Call::Yield { .. })
        && service.home.store().binding(binding)?.is_some_and(
            |bound| {
                !bound.errand && service.merges.held(&bound.agent)
            },
        ))
}

/// Whether a chief considers an objection to the instruction whose
/// turn made a call.
fn objecting(
    service: &mut Service,
    binding: BindingId,
    input: Option<&str>,
) -> Result<bool> {
    crate::society::reviews::held(service, binding, input)
}

/// Prepares the prompt of the next model call, after the mayor's
/// review of a chief's instruction is asked for.
async fn prompt_call(
    service: &mut Service,
    harnesses: &mut Harnesses,
    binding: BindingId,
    re: Re,
    call: Call,
) -> Result<()> {
    let input = call.input();
    crate::society::reviews::prepare(
        service, harnesses, binding, input,
    )
    .await?;
    if objecting(service, binding, input)? {
        harnesses
            .answer(re, Answer::Waiting(OBJECTING.into()))
            .await;
        return Ok(());
    }
    let prepared = prompt(service, binding, input)?;
    let signature = prepared.signature();
    let answers = re.clone();
    service.chats.calls.prepare(binding, re, async move {
        Prepared {
            binding,
            re: answers,
            call,
            signature,
            result: Preparation::Prompt(prepared.render().await),
        }
    });
    Ok(())
}

/// Reads the file a write replaces, for its approval to show.
fn preview_call(
    service: &mut Service,
    binding: BindingId,
    re: Re,
    call: Call,
    reading: Reading,
) {
    let answers = re.clone();
    service.chats.calls.prepare(binding, re, async move {
        let Reading {
            tools,
            path,
            signature,
        } = reading;
        let result = tools.preview(&path).await;
        Prepared {
            binding,
            re: answers,
            call,
            signature,
            result: Preparation::Preview(result),
        }
    });
}

/// What carrying out a call comes to.
enum Step {
    Answer(Answer),
    /// The call runs, and is answered when it ends.
    Running,
    /// The call waits for the file it writes to be read.
    Preview(Reading),
    /// The gate let a society call run, which the society carries
    /// out.
    Society(Binding),
}

/// Carries out the call `key`: its marked result when it ran, else as
/// the gate decides now.
#[allow(clippy::too_many_arguments)]
fn operation(
    service: &mut Service,
    binding: BindingId,
    re: &Re,
    key: &str,
    input: Option<&str>,
    tool: &str,
    arguments: &BTreeMap<String, Value>,
    preview: Option<&std::result::Result<Option<String>, String>>,
) -> Result<Step> {
    let call = match decide::admit(
        service, binding, key, input, tool, arguments,
    )? {
        Ok(call) => call,
        Err(Unadmitted::Unbound) => {
            return Ok(Step::Answer(Answer::result(NOBODY, true)));
        }
        Err(Unadmitted::Changed) => {
            return Ok(Step::Answer(Answer::Refusal(
                IDENTITY_REFUSAL.into(),
            )));
        }
    };
    if service.chats.calls.join(&call.id, re) {
        return Ok(Step::Running);
    }
    if service.house_work.pending(&call.id) {
        return Ok(Step::Answer(Answer::Waiting(
            "The house work is still running.".into(),
        )));
    }
    let request = Request::tool(tool, arguments)?;
    let db = service.home.store().connection();
    match operation::marked::<tools::Outcome>(db, &call.id, &request)?
    {
        Some(Marked::Begun) => {
            return Ok(Step::Answer(Answer::result(
                format!(
                    "Tool {tool} was interrupted by a restart and \
                     may have partially run"
                ),
                true,
            )));
        }
        Some(Marked::Outcome(outcome)) => {
            return Ok(Step::Answer(outcome.into()));
        }
        Some(Marked::Conflict) => {
            return Ok(Step::Answer(Answer::Refusal(
                CONFLICT.into(),
            )));
        }
        None => {}
    }
    if tool != "object" && objecting(service, binding, input)? {
        return Ok(Step::Answer(Answer::Waiting(OBJECTING.into())));
    }
    if crate::society::withheld(service, binding, tool)? {
        let outcome =
            service.home.store().once(&call.id, &request, |_| {
                Ok(crate::society::unavailable(tool))
            })?;
        return Ok(Step::Answer(match outcome {
            Some(outcome) => crate::society::result(&outcome)?,
            None => Answer::Refusal(CONFLICT.into()),
        }));
    }
    if tool == "object" {
        return Ok(Step::Society(call.bound));
    }
    if tool == "ask_chief" {
        return ask::ask(service, &call).map(Step::Answer);
    }
    let net = match decide::decide(service, &call, preview)? {
        Gate::Decided(Decision::Run { net }) => net,
        Gate::Decided(Decision::Block(words)) => {
            return Ok(Step::Answer(Answer::Refusal(words)));
        }
        Gate::Decided(Decision::Wait(words)) => {
            return Ok(Step::Answer(Answer::Waiting(words.into())));
        }
        Gate::Preview(reading) => return Ok(Step::Preview(reading)),
    };
    if tool == "build_road" {
        let call_of =
            match crate::harness::tools::parse(tool, arguments) {
                Ok(call) => call,
                Err(words) => {
                    return Ok(Step::Answer(Answer::result(
                        words, true,
                    )));
                }
            };
        let (text, error) = crate::harness::tools::run(
            service.home.store(),
            &call.id,
            &call.bound,
            &call_of,
            crate::clock::now(),
        )?;
        return Ok(Step::Answer(Answer::result(text, error)));
    }
    if crate::society::SocietyTool::named(tool).is_some() {
        return Ok(Step::Society(call.bound));
    }
    run(service, call, re, &request, net)?;
    Ok(Step::Running)
}

/// Runs the tool call `call`, after marking it begun when it may not
/// run again.
fn run(
    service: &mut Service,
    call: decide::Admitted<'_>,
    re: &Re,
    request: &Request,
    net: bool,
) -> Result<()> {
    let db = service.home.store().connection();
    let change = crate::oversight::held::decision(db, &call.id)?
        .and_then(|kept| kept.change);
    if UNSAFE.contains(&call.tool) || call.hand.is_some() {
        operation::begin(db, &call.id, request)?;
    }
    let place = decide::places(service, &call.bound.agent)?;
    let tools = service.tools(&call.bound.agent, place, net)?;
    let secrets = service.credentials.secrets();
    let arguments =
        call.arguments.clone().into_iter().collect::<Map<_, _>>();
    let name = call.tool.to_owned();
    service.chats.calls.run(
        call.id,
        call.bound.id,
        re.clone(),
        async move {
            let mut outcome =
                tools.run(&secrets, &name, &arguments).await;
            if let Some(change) = change {
                outcome
                    .details
                    .get_or_insert_with(Map::new)
                    .insert("change".into(), change);
            }
            outcome
        },
    );
    Ok(())
}

/// Stops the tool calls running for `binding`.
pub(crate) fn stop_tools(service: &mut Service, binding: BindingId) {
    service.chats.calls.stop(binding);
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
        let text = crate::society::errands::instructions(
            service.home.store().connection(),
            binding,
        )?;
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
    let mark = service.home.store().input_mark(binding, input)?;
    if let (Some(input), Some(InputMark::Reflection { .. })) =
        (input, &mark)
    {
        records::enter(
            service.home.store().connection(),
            binding,
            input,
        )?;
    }
    let task =
        mark.as_ref().and_then(InputMark::task).map(String::from);
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
    let model = record.limits.chief.model_of(
        &member,
        record.mayor == agent.number,
        &service.settings()?.model,
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
    let mark = service.home.store().input_mark(binding, input)?;
    let reflection =
        matches!(mark, Some(InputMark::Reflection { .. }));
    let tasked = mark.as_ref().and_then(InputMark::task).is_some();
    let cutoff = answer.stop.as_deref() == Some("length")
        && answer
            .usage
            .as_ref()
            .is_some_and(|usage| usage.output < 1024)
        && !reflection
        && tasked;
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
        follow::fail_answer(
            service.home.store().connection(),
            binding,
            input,
            &words,
        )?;
    }
    Ok(Answer::Verdict {
        notice: Some((&InputMark::notice(&words)).into()),
        words: Some(words),
        compact: false,
    })
}

/// Whether the turn of `input` is a reflection.
fn reflecting(
    service: &mut Service,
    binding: BindingId,
    input: Option<&str>,
) -> Result<bool> {
    Ok(matches!(
        service.home.store().input_mark(binding, input)?,
        Some(InputMark::Reflection { .. })
    ))
}

/// Whether a chief oversees the turn of `input` at `place`: a chief's
/// line, or a retry in a chief's chat.
fn chief_oversees(
    service: &mut Service,
    binding: BindingId,
    input: Option<&str>,
    place: &records::Place,
) -> Result<bool> {
    Ok(match service.home.store().input_mark(binding, input)? {
        Some(InputMark::Chief { .. }) => true,
        Some(InputMark::Retry { .. }) => place.chief.is_some(),
        _ => false,
    })
}
