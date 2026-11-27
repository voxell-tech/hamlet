//! The realm actor's commands that drive the agent as itself, through
//! the agent's demo-only `forward`.

use std::io::Write;

use anyhow::{Context, Result, bail, ensure};
use hamlet_host::Host;
use hamlet_messages::address::{self};
use hamlet_messages::realm::budget::{BudgetRead, ReadBudget};
use hamlet_messages::realm::letters::{
    Ask, Enclosed, Reply, Sent, Tell, Waiting,
};
use hamlet_messages::realm::models::{self, KeepCredential};
use hamlet_messages::realm::oversight::{AskChief, Object, Question};
use hamlet_messages::realm::roads::{AgreeRoad, Agreed};
use hamlet_messages::realm::workers::{
    Ended, ErrandDone, StartWorker, Started, WorkerDone,
};
use hamlet_messages::realm::{Done, LetterId, Tool};
use hamlet_messages::workspace::{
    AgentNumber, CallId, ConversationId, Program, Run, RunAnswer,
    RunEnd,
};

use crate::agent_host::{forward, taken};
use crate::command::number;
use crate::realm_actor::{
    CONVERSATION, charge, copy_parts, done, exit_word, may_spend,
    named, read_copies, refused, task_number, task_or_standing,
    task_word, tell, tool, until,
};

pub async fn as_agent(
    host: &Host,
    me: u32,
    words: &[String],
) -> Result<()> {
    let mut out = std::io::stdout();
    match words {
        [verb, call, to, words @ ..] if verb == "tell" => {
            let mut tell = Tell {
                to: named(to)?,
                text: String::new(),
                paths: Vec::new(),
            };
            let mut task = None;
            let mut text = words;
            while let [word, rest @ ..] = text {
                match word.split_once('=') {
                    Some(("task", word)) => {
                        task = Some(task_word(host, word).await?);
                    }
                    Some(("paths", paths)) => {
                        tell.paths = paths
                            .split(',')
                            .map(|path| Enclosed {
                                path: path.into(),
                            })
                            .collect();
                    }
                    _ => break,
                }
                text = rest;
            }
            tell.text = text.join(" ");
            let tool = Tool {
                task,
                ..tool(call, tell)
            };
            match forward(
                host,
                AgentNumber(me),
                &address::Realm,
                &tool,
            )
            .await?
            {
                Sent::Sent { .. } => writeln!(out, "sent/{call} 1")?,
                Sent::Refused { why } => refused(call, &why)?,
            }
        }
        [verb, call, task, question @ ..]
            if verb == "ask-chief" && !question.is_empty() =>
        {
            let ask = AskChief {
                task: task_word(host, task).await?,
                conversation: ConversationId(CONVERSATION.to_owned()),
                message: None,
                questions: vec![Question {
                    question: question.join(" "),
                    description: None,
                    options: Vec::new(),
                    text_input: true,
                }],
            };
            done(
                "asked",
                call,
                forward(
                    host,
                    AgentNumber(me),
                    &address::Realm,
                    &tool(call, ask),
                )
                .await?,
            )?;
        }
        [verb, call, task, reason @ ..] if verb == "object" => {
            let object = Object {
                task: task_word(host, task).await?,
                conversation: ConversationId(CONVERSATION.to_owned()),
                reason: reason.join(" "),
            };
            done(
                "objected",
                call,
                forward(
                    host,
                    AgentNumber(me),
                    &address::Realm,
                    &tool(call, object),
                )
                .await?,
            )?;
        }
        [verb, call, brief @ ..] if verb == "start-worker" => {
            let start = StartWorker {
                brief: brief.join(" "),
                tools: Vec::new(),
                task: None,
                hamlet: None,
            };
            match forward(
                host,
                AgentNumber(me),
                &address::Realm,
                &tool(call, start),
            )
            .await?
            {
                Started::Started { worker } => {
                    writeln!(
                        out,
                        "started/{call} {}",
                        worker.agent.0
                    )?;
                }
                Started::Refused { why } => refused(call, &why)?,
            }
        }
        [verb, text @ ..]
            if verb == "worker-done" || verb == "errand-done" =>
        {
            let ended = Ended::Answered {
                text: text.join(" "),
            };
            let me = AgentNumber(me);
            if verb == "worker-done" {
                forward(
                    host,
                    me,
                    &address::Realm,
                    &WorkerDone { ended },
                )
                .await?;
            } else {
                forward(
                    host,
                    me,
                    &address::Realm,
                    &ErrandDone { ended },
                )
                .await?;
            }
            writeln!(out, "{verb}/{} 1", me.0)?;
        }
        [verb, call, tokens_in, tokens_out, task @ ..]
            if verb == "call" =>
        {
            let price = read_copies(host, me)
                .await?
                .model
                .and_then(|model| model.value.price)
                .context("the agent's model copy holds no price")?;
            let tokens_in = u32::try_from(number(Some(tokens_in))?)?;
            let tokens_out =
                u32::try_from(number(Some(tokens_out))?)?;
            let micros = f64::from(tokens_in) * price.input
                + f64::from(tokens_out) * price.output;
            ensure!(
                (0.0..=f64::from(u32::MAX)).contains(&micros),
                "a call costs {micros} millionths of a dollar"
            );
            let micros = micros.round() as u32;
            let task =
                task_or_standing(host, me, task.first()).await?;
            if may_spend(host, me, task.clone()).await? {
                let tokens = Some(tokens_in + tokens_out);
                charge(host, me, call, task, micros, tokens).await?;
                writeln!(out, "called/{call} {micros}")?;
            } else {
                writeln!(out, "refused/{call} 1")?;
            }
        }
        [verb, call, task @ ..] if verb == "read-budget" => {
            let task = match task.first() {
                Some(task) => Some(task_word(host, task).await?),
                None => None,
            };
            let read = ReadBudget { task };
            match forward(
                host,
                AgentNumber(me),
                &address::Realm,
                &tool(call, read),
            )
            .await?
            {
                BudgetRead::Budget {
                    spend,
                    own_micros,
                    allowance_micros,
                    ..
                } => {
                    tracing::info!(
                        ?spend,
                        own_micros,
                        ?allowance_micros,
                        "agent {me} read its budget"
                    );
                    writeln!(out, "own/{me} {own_micros}")?;
                    writeln!(
                        out,
                        "allowance/{me} {}",
                        allowance_micros
                            .map_or(Ok(-1), i64::try_from)?
                    )?;
                }
                BudgetRead::Refused { why } => refused(call, &why)?,
            }
        }
        [verb] if verb == "copies" => {
            for (part, version, value) in
                copy_parts(&read_copies(host, me).await?)
            {
                tracing::info!(
                    version,
                    value,
                    "agent {me} read the copy of its {part}"
                );
                writeln!(out, "copy/{me}/{part} {version}")?;
            }
        }
        [verb, provider, key] if verb == "keep-credential" => {
            let keep = KeepCredential {
                provider: provider.clone(),
                value: models::Kept::Key {
                    key: key.clone(),
                    env: Default::default(),
                },
            };
            forward(host, AgentNumber(me), &address::Realm, &keep)
                .await?;
            writeln!(out, "kept/{provider} 1")?;
        }
        [verb, call, mine, theirs] if verb == "agree" => {
            let agree = AgreeRoad {
                mine: named(mine)?,
                theirs: named(theirs)?,
                why: None,
            };
            match forward(
                host,
                AgentNumber(me),
                &address::Realm,
                &tool(call, agree),
            )
            .await?
            {
                Agreed::Waiting { .. } => {
                    writeln!(out, "waiting/{call} 1")?;
                }
                Agreed::Laid { .. } => {
                    writeln!(out, "laid/{call} 1")?;
                }
                Agreed::Refused { why } => refused(call, &why)?,
            }
        }
        [verb, kind, count, text @ ..] if verb == "await" => {
            let count = usize::try_from(number(Some(count))?)?;
            let text = text.join(" ");
            let took = || async {
                let holding = taken(host, AgentNumber(me)).await?;
                anyhow::Ok(
                    holding
                        .inputs
                        .iter()
                        .filter(|input| {
                            input.kind == *kind
                                && input.text.contains(&text)
                        })
                        .count(),
                )
            };
            until(|| async {
                Ok((took().await? >= count).then_some(()))
            })
            .await?
            .with_context(|| {
                format!("agent {me} took fewer than {count} {kind}")
            })?;
            writeln!(out, "took/{me}/{kind} {}", took().await?)?;
        }
        [verb, call, to, text @ ..] if verb == "ask" => {
            let ask = Ask {
                to: named(to)?,
                text: text.join(" "),
                conversation: ConversationId(CONVERSATION.to_owned()),
                paths: Vec::new(),
            };
            match forward(
                host,
                AgentNumber(me),
                &address::Realm,
                &tool(call, ask),
            )
            .await?
            {
                Waiting::Waiting { .. } => {
                    writeln!(out, "sent/{call} 1")?;
                }
                Waiting::Refused { why } => refused(call, &why)?,
            }
        }
        [verb, question, text @ ..] if verb == "reply" => {
            let reply = Reply {
                question: LetterId(u32::try_from(number(Some(
                    question,
                ))?)?),
                text: text.join(" "),
            };
            match forward(
                host,
                AgentNumber(me),
                &address::Realm,
                &reply,
            )
            .await?
            {
                Done::Done => writeln!(out, "replied/{question} 1")?,
                Done::Refused { why } => refused(question, &why)?,
            }
        }
        [verb, task @ ..] if verb == "may-spend" => {
            let task =
                task_or_standing(host, me, task.first()).await?;
            let may = may_spend(host, me, task).await?;
            writeln!(out, "may/{me} {}", i64::from(may))?;
        }
        [verb, call, micros, task @ ..] if verb == "charge" => {
            let task =
                task_or_standing(host, me, task.first()).await?;
            let micros = u32::try_from(number(Some(micros))?)?;
            charge(host, me, call, task, micros, None).await?;
            writeln!(out, "charged/{call} {micros}")?;
        }
        [verb, micros, times] if verb == "spend" => {
            let task = taken(host, AgentNumber(me)).await?.standing;
            let micros = u32::try_from(number(Some(micros))?)?;
            let mut spent = 0;
            for time in 1..=number(Some(times))? {
                if !may_spend(host, me, task.clone()).await? {
                    break;
                }
                let call = format!("spend-{time}");
                charge(host, me, &call, task.clone(), micros, None)
                    .await?;
                spent += u64::from(micros);
            }
            writeln!(out, "spent/{me} {spent}")?;
        }
        [verb, count, to] if verb == "letters" => {
            let count = number(Some(count))?;
            for letter in 1..=count {
                let call = format!("letter-{letter}");
                let text = format!("letter {letter}");
                if let Sent::Refused { why } =
                    tell(host, me, &call, named(to)?, text).await?
                {
                    bail!("{call} was refused: {why}");
                }
            }
            writeln!(out, "told/{me} {count}")?;
        }
        [verb, call, script @ ..] if verb == "run" => {
            let (network, script) = match script {
                [net, script @ ..] if net == "net" => (true, script),
                _ => (false, script),
            };
            ensure!(!script.is_empty(), "run takes a script");
            let run = Run {
                call: CallId(call.clone()),
                conversation: ConversationId(CONVERSATION.to_owned()),
                realm_task: None,
                program: Program::Script {
                    script: script.join(" "),
                    network,
                    timeout_ms: None,
                },
            };
            match forward(
                host,
                AgentNumber(me),
                &address::Workspace,
                &run,
            )
            .await?
            {
                RunAnswer::Ended {
                    end: RunEnd::Ran(end),
                } => {
                    let (exit, code) = exit_word(&end.exit);
                    writeln!(out, "done/{call}/{exit} {code}")?;
                }
                RunAnswer::Ended {
                    end: RunEnd::NotRun { why },
                } => refused(call, &why)?,
                RunAnswer::Running { command } => {
                    writeln!(out, "running/{call} {}", command.0)?;
                }
                RunAnswer::WaitsForChief { needs } => {
                    writeln!(out, "waits/{call} {}", needs.len())?;
                }
                RunAnswer::Refused { why } => refused(call, &why)?,
            }
        }
        [verb] if verb == "inputs" => {
            let holding = taken(host, AgentNumber(me)).await?;
            writeln!(
                out,
                "standing/{me} {}",
                task_number(&holding.standing)
            )?;
            for input in holding.inputs {
                let task =
                    input.task.as_ref().map_or(-1, task_number);
                writeln!(
                    out,
                    "input/{me}/{}/{} {task}",
                    input.taken, input.kind
                )?;
                if let Some(letter) = input.question {
                    writeln!(
                        out,
                        "question/{me}/{} {letter}",
                        input.taken
                    )?;
                }
            }
        }
        _ => bail!(
            "usage: demo agent <n> tell|ask|reply|ask-chief|object|\
             start-worker|worker-done|errand-done|agree|may-spend|\
             charge|call|read-budget|spend|letters|run|copies|\
             keep-credential|inputs|await ..."
        ),
    }
    Ok(())
}
