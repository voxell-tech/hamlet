//! The society's tools (H35): what an agent does in the society, each
//! checked against the caller's standing as a chief's command is.

mod letters;
pub(crate) mod library;
mod scholars;
pub(crate) mod welcome;

use anyhow::Result;
use hamlet_protocol::runtime::{ToolSpec, ToolSpecReplay};
use hamlet_store::ids::{
    Actor, AgentId, ChiefId, HamletId, ScholarId, TaskId,
};
use serde_json::{Value, json};

use super::{Call, said, spec};
use crate::change::{Change, Done};
use crate::core::Core;
use crate::society::addresses::{self, Reach};
use crate::society::agents::{self, AgentKind, EndHow};
use crate::society::hamlets::{self, Founding};
use crate::society::hires::{self, Hiring, Spec};
use crate::society::refusal::Refusal;
use crate::society::{authority, city, limits, record, roads};
use crate::spend::micros;
use crate::talk::Env;
use crate::work::tools::Outcome;

/// What a society call comes to.
pub(super) enum Step {
    Done(Outcome),
    /// It waits for another agent, in these words.
    Wait(String),
}

impl Step {
    fn of(result: Result<String, Refusal>) -> Self {
        match result {
            Ok(words) => Self::Done(said(words, false)),
            Err(refusal) => {
                Self::Done(said(refusal.to_string(), true))
            }
        }
    }
}

fn string(description: &str) -> Value {
    json!({"type": "string", "description": description})
}

fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type": "object", "properties": properties, "required": required})
}

/// The society's tools as a model is offered them.
pub(super) fn specs() -> Vec<ToolSpec> {
    let address = "An agent's address: `hand`, `mayor`, \
                   `mayor@<hamlet>`, `<hamlet>#<n>` or \
                   `worker#<id>`.";
    let reason = "Why, in a sentence; the record keeps it.";
    // A call cut off by a crash may be asked again: its change is
    // kept with its answer, or what it began is found by its call.
    // A write of a scholar's repository is kept by no transaction.
    let unsafe_ = ToolSpecReplay::Unsafe;
    let safe = ToolSpecReplay::Safe;
    vec![
        spec(
            "tell",
            "Send a letter to an agent you have a road to. It is \
             queued for them; you do not wait for an answer.",
            object(
                json!({"to": string(address), "text": string("The letter.")}),
                &["to", "text"],
            ),
            safe.clone(),
        ),
        spec(
            "ask",
            "Ask an agent you have a road to a question and wait \
             for its answer. Ask only what you need; a question to \
             someone waiting on you is refused.",
            object(
                json!({"to": string(address), "question": string("The question.")}),
                &["to", "question"],
            ),
            safe.clone(),
        ),
        spec(
            "start_worker",
            "Start a worker on a task: a helper with your tools \
             that works in a scratch folder and where you work, \
             then answers once. With `wait`, you wait for its \
             answer; without, the answer comes to you as a reply.",
            object(
                json!({
                    "task": string("What the worker is to do, and what to answer."),
                    "tools": {"type": "array", "items": {"type": "string"},
                        "description": "Your built-in tools it may use; all of them when left out."},
                    "wait": {"type": "boolean", "description": "Wait for its answer."}
                }),
                &["task"],
            ),
            safe.clone(),
        ),
        spec(
            "submit_skill",
            "Offer a skill from your skills/ folder to the library, \
             where every agent can read it. The librarian judges it.",
            object(
                json!({"skill": string("The skill's folder name under skills/."), "reason": string(reason)}),
                &["skill", "reason"],
            ),
            safe.clone(),
        ),
        spec(
            "ask_librarian",
            "Ask the librarian which skills in the library help \
             with something.",
            object(
                json!({"question": string("What you need a skill for.")}),
                &["question"],
            ),
            safe.clone(),
        ),
        spec(
            "object",
            "Object to the chief's instruction you are working on, \
             with your reason. The chief rules: go on, or withdraw \
             it.",
            object(
                json!({"reason": string("Why the instruction should not be done.")}),
                &["reason"],
            ),
            safe.clone(),
        ),
        spec(
            "request_road",
            "Ask your lead for a road to an agent of your hamlet \
             you need to reach.",
            object(
                json!({"to": string(address), "reason": string(reason)}),
                &["to", "reason"],
            ),
            safe.clone(),
        ),
        spec(
            "create_scholar",
            "Make a new scholar of the city, to hire next. Without \
             a character it starts from the realm's own. A scholar \
             you make may wait at the harbor until it is let into \
             the city; its hires work all the same.",
            object(
                json!({"name": string("A short name, one line with no colon. It is a label and may repeat."), "character": string("Who the scholar is and how it works.")}),
                &["name"],
            ),
            safe.clone(),
        ),
        spec(
            "hire",
            "Hire a scholar into your hamlet under you, or under a \
             hire of your tree. When no scholar fits, give `name` \
             instead to hire a new blank one, which starts from the \
             realm's character and the library's skills.",
            object(
                json!({
                    "scholar": string("The scholar's address, `scholar#<id>`, with `@<version>` to pin a version."),
                    "name": string("A new blank scholar's name, in place of `scholar`."),
                    "purpose": string("What the hire is for."),
                    "under": string("The hire it answers to; you when left out."),
                    "hamlet": string("The hamlet, for the Hand.")
                }),
                &["purpose"],
            ),
            safe.clone(),
        ),
        spec(
            "end_hire",
            "End a hire under you. Its work stays in its scholar.",
            object(
                json!({"hire": string(address), "reason": string(reason)}),
                &["hire", "reason"],
            ),
            safe.clone(),
        ),
        spec(
            "merge_hires",
            "Merge one hire under you into another: the kept hire \
             takes what the other learned, its roads and its hires.",
            object(
                json!({"absorbed": string(address), "into": string(address), "reason": string(reason)}),
                &["absorbed", "into", "reason"],
            ),
            safe.clone(),
        ),
        spec(
            "set_model",
            "Choose the model a hire under you runs; leave out \
             `model` for its hamlet's.",
            object(
                json!({"hire": string(address), "model": string("The model, as `provider::id`."), "reason": string(reason)}),
                &["hire", "reason"],
            ),
            safe.clone(),
        ),
        spec(
            "set_tools",
            "Turn built-in tools off or back on for a hire under \
             you.",
            object(
                json!({
                    "hire": string(address),
                    "off": {"type": "array", "items": {"type": "string"}},
                    "on": {"type": "array", "items": {"type": "string"}},
                    "reason": string(reason)
                }),
                &["hire", "reason"],
            ),
            safe.clone(),
        ),
        spec(
            "set_allowance",
            "Give a hire under you an allowance in dollars for the \
             budget's period, or none.",
            object(
                json!({"hire": string(address), "dollars": {"type": "number"}, "reason": string(reason)}),
                &["hire", "reason"],
            ),
            safe.clone(),
        ),
        spec(
            "nickname",
            "Give a hire under you a short nickname it is shown by \
             in the world; leave out `nickname` for its own name.",
            object(
                json!({"hire": string(address), "nickname": string("At most 24 letters."), "reason": string(reason)}),
                &["hire", "reason"],
            ),
            safe.clone(),
        ),
        spec(
            "lay_road",
            "Lay a road between two hires under you, so they can \
             write and ask each other.",
            object(
                json!({"a": string(address), "b": string(address), "reason": string(reason)}),
                &["a", "b", "reason"],
            ),
            safe.clone(),
        ),
        spec(
            "take_up_road",
            "Take up a road you laid between two hires under you.",
            object(
                json!({"a": string(address), "b": string(address), "reason": string(reason)}),
                &["a", "b", "reason"],
            ),
            safe.clone(),
        ),
        spec(
            "agree_road",
            "As a mayor, agree a road between a hire of your hamlet \
             and one of another. It opens once both mayors agree.",
            object(
                json!({"mine": string(address), "theirs": string(address), "reason": string(reason)}),
                &["mine", "theirs", "reason"],
            ),
            safe.clone(),
        ),
        spec(
            "found_hamlet",
            "Found a hamlet with a scholar as its mayor.",
            object(
                json!({
                    "hamlet": string("Its id: lowercase letters, digits and hyphens."),
                    "scholar": string("The address, `scholar#<id>`, of the scholar its mayor is a hire of."),
                    "purpose": string("What the hamlet is for.")
                }),
                &["hamlet", "scholar", "purpose"],
            ),
            safe.clone(),
        ),
        spec(
            "seat_mayor",
            "Seat a hire as its hamlet's mayor.",
            object(
                json!({"hire": string(address), "reason": string(reason)}),
                &["hire", "reason"],
            ),
            safe.clone(),
        ),
        spec(
            "remove_hamlet",
            "Remove a hamlet; a chief approves it first.",
            object(
                json!({"hamlet": string("The hamlet's id."), "reason": string(reason)}),
                &["hamlet", "reason"],
            ),
            safe.clone(),
        ),
        spec(
            "read_record",
            "Read a hamlet's record: every decision with its reason.",
            object(
                json!({"hamlet": string("The hamlet's id.")}),
                &["hamlet"],
            ),
            safe.clone(),
        ),
        spec(
            "read_scholar",
            "List a scholar's files, or read one, at its `main` or \
             a version.",
            object(
                json!({"scholar": string("The scholar's address, `scholar#<id>`."), "path": string("A file to read."), "version": string("A version or commit.")}),
                &["scholar"],
            ),
            safe,
        ),
        spec(
            "edit_scholar",
            "Write a file of a scholar's `main`; a chief approves \
             it first.",
            object(
                json!({
                    "scholar": string("The scholar's address, `scholar#<id>`."),
                    "path": string("The file."),
                    "content": string("Its new text; empty removes it."),
                    "message": string("What the change does.")
                }),
                &["scholar", "path", "content", "message"],
            ),
            unsafe_.clone(),
        ),
        spec(
            "tag",
            "Name the scholar's `main` as a version; a chief \
             approves it first.",
            object(
                json!({"scholar": string("The scholar's address, `scholar#<id>`."), "tag": string("The version's name."), "note": string("What it holds.")}),
                &["scholar", "tag"],
            ),
            unsafe_.clone(),
        ),
        spec(
            "revert",
            "Undo a commit of a scholar's `main`; a chief approves \
             it first.",
            object(
                json!({"scholar": string("The scholar's address, `scholar#<id>`."), "commit": string("The commit or version to undo."), "reason": string(reason)}),
                &["scholar", "commit", "reason"],
            ),
            unsafe_,
        ),
    ]
}

/// The string argument `key` of `call`, trimmed.
fn text(call: &Call<'_>, key: &str) -> String {
    call.arguments
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned()
}

fn list(call: &Call<'_>, key: &str) -> Vec<String> {
    call.arguments
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(String::from)
        .collect()
}

impl Core {
    /// Carries out the society call `call`, which the gate let run.
    pub(super) async fn society(
        &mut self,
        call: &Call<'_>,
    ) -> Result<Step> {
        let agent = call.conversation.agent;
        let here = self.hamlet_of(agent)?;
        let reason = text(call, "reason");
        let result = match call.tool {
            "tell" => return self.tell(call).await,
            "ask" => return self.ask(call).await,
            "request_road" => return self.request_road(call).await,
            "start_worker" => return self.start_worker(call).await,
            "submit_skill" => return self.submit_skill(call).await,
            "ask_librarian" => return self.ask_librarian(call).await,
            "read_scholar" => self.read_scholar(call)?,
            "edit_scholar" => self.edit_scholar(call).await?,
            "tag" => self.tag_scholar(call)?,
            "revert" => self.revert_scholar(call).await?,
            "create_scholar" => {
                let name = text(call, "name");
                let character = text(call, "character");
                let task = self.task_of(call)?;
                let db = self.realm.store().connection();
                let chief = task_chief(db, task)?;
                let owner = authority::owner_for(
                    db,
                    Actor::Agent(agent),
                    here.as_ref(),
                    chief,
                )?;
                self.carry(call, |cx, env| {
                    let character = if character.is_empty() {
                        env.settings.character.clone()
                    } else {
                        character
                    };
                    let made = match city::make(
                        cx, &name, owner, &character, None,
                    )? {
                        Ok(made) => made,
                        Err(refusal) => return Ok(Err(refusal)),
                    };
                    Ok(Ok(arrived(cx, env, made, &character)?))
                })
                .await?
            }
            "hire" => self.hire_tool(call, here.as_ref()).await?,
            "end_hire" => {
                let hire =
                    match self.led(call, "hire", here.as_ref())? {
                        Ok(hire) => hire,
                        Err(refusal) => {
                            return Ok(Step::of(Err(refusal)));
                        }
                    };
                let name = self.name(hire)?;
                self.carry(call, |cx, _| {
                    Ok(hires::end(cx, hire, EndHow::Ended, &reason)?
                        .map(|()| format!("Ended {name}.")))
                })
                .await?
            }
            "set_model" => {
                let hire =
                    match self.led(call, "hire", here.as_ref())? {
                        Ok(hire) => hire,
                        Err(refusal) => {
                            return Ok(Step::of(Err(refusal)));
                        }
                    };
                let model = text(call, "model");
                let model = (!model.is_empty()).then_some(model);
                let name = self.name(hire)?;
                let words = format!(
                    "{name} runs {} now.",
                    model.as_deref().unwrap_or("its hamlet's model")
                );
                self.carry(call, |cx, _| {
                    Ok(hires::set_model(
                        cx,
                        hire,
                        model.as_deref(),
                        &reason,
                    )?
                    .map(|()| words))
                })
                .await?
            }
            "set_tools" => {
                let hire =
                    match self.led(call, "hire", here.as_ref())? {
                        Ok(hire) => hire,
                        Err(refusal) => {
                            return Ok(Step::of(Err(refusal)));
                        }
                    };
                let (off, on) = (list(call, "off"), list(call, "on"));
                let name = self.name(hire)?;
                self.carry(call, |cx, _| {
                    Ok(hires::set_tools(
                        cx, hire, &off, &on, &reason,
                    )?
                    .map(|()| {
                        format!("Changed the tools of {name}.")
                    }))
                })
                .await?
            }
            "set_allowance" => {
                let hire =
                    match self.led(call, "hire", here.as_ref())? {
                        Ok(hire) => hire,
                        Err(refusal) => {
                            return Ok(Step::of(Err(refusal)));
                        }
                    };
                let allowance = call
                    .arguments
                    .get("dollars")
                    .and_then(Value::as_f64)
                    .map(micros);
                let name = self.name(hire)?;
                self.carry(call, |cx, _| {
                    Ok(hires::set_allowance(
                        cx, hire, allowance, &reason,
                    )?
                    .map(|()| {
                        format!("Set the allowance of {name}.")
                    }))
                })
                .await?
            }
            "nickname" => {
                let hire =
                    match self.led(call, "hire", here.as_ref())? {
                        Ok(hire) => hire,
                        Err(refusal) => {
                            return Ok(Step::of(Err(refusal)));
                        }
                    };
                let nickname = text(call, "nickname");
                let given =
                    (!nickname.trim().is_empty()).then_some(nickname);
                let name = self.name(hire)?;
                let words = match &given {
                    Some(nickname) => {
                        format!("{name} is called {nickname} now.")
                    }
                    None => format!("{name} goes by its name again."),
                };
                self.carry(call, |cx, _| {
                    Ok(hires::set_nickname(
                        cx,
                        hire,
                        given.as_deref(),
                        &reason,
                    )?
                    .map(|()| words))
                })
                .await?
            }
            "lay_road" | "take_up_road" => {
                let a = match self.led(call, "a", here.as_ref())? {
                    Ok(a) => a,
                    Err(refusal) => {
                        return Ok(Step::of(Err(refusal)));
                    }
                };
                let b = match self.led(call, "b", here.as_ref())? {
                    Ok(b) => b,
                    Err(refusal) => {
                        return Ok(Step::of(Err(refusal)));
                    }
                };
                let (one, other) = (self.name(a)?, self.name(b)?);
                let task = self.task_of(call)?;
                if call.tool == "lay_road" {
                    self.carry(call, |cx, _| {
                        Ok(roads::lay(cx, a, b, &reason, task)?.map(
                            |_| {
                                format!(
                                    "Laid a road between {one} and \
                                     {other}."
                                )
                            },
                        ))
                    })
                    .await?
                } else {
                    self.carry(call, |cx, _| {
                        Ok(roads::take_up(cx, a, b, &reason)?.map(
                            |()| {
                                format!(
                                    "Took up the road between {one} \
                                     and {other}."
                                )
                            },
                        ))
                    })
                    .await?
                }
            }
            "merge_hires" => {
                let absorbed = match self.led(
                    call,
                    "absorbed",
                    here.as_ref(),
                )? {
                    Ok(hire) => hire,
                    Err(refusal) => {
                        return Ok(Step::of(Err(refusal)));
                    }
                };
                let kept =
                    match self.led(call, "into", here.as_ref())? {
                        Ok(hire) => hire,
                        Err(refusal) => {
                            return Ok(Step::of(Err(refusal)));
                        }
                    };
                let (one, other) =
                    (self.name(absorbed)?, self.name(kept)?);
                self.carry(call, |cx, _| {
                    Ok(crate::society::merges::merge(
                        cx, kept, absorbed, &reason,
                    )?
                    .map(|_| format!("Merged {one} into {other}.")))
                })
                .await?
            }
            "agree_road" => {
                self.agree_road(call, here.as_ref()).await?
            }
            "found_hamlet" => {
                let id = HamletId(text(call, "hamlet"));
                let purpose = text(call, "purpose");
                let task = self.task_of(call)?;
                let db = self.realm.store().connection();
                let overseer = task_chief(db, task)?;
                let chief = authority::owner_for(
                    db,
                    Actor::Agent(agent),
                    None,
                    overseer,
                )?;
                let mayor = match self
                    .scholar_hiring(&text(call, "scholar"))?
                {
                    Ok(hiring) => hiring,
                    Err(refusal) => {
                        return Ok(Step::of(Err(refusal)));
                    }
                };
                let words = format!("Founded {id}.");
                self.carry(call, |cx, _| {
                    Ok(hamlets::found(
                        cx,
                        Founding {
                            id,
                            chief,
                            mayor,
                            purpose,
                        },
                    )?
                    .map(|_| words))
                })
                .await?
            }
            "seat_mayor" => {
                let hire =
                    match self.named(call, "hire", here.as_ref())? {
                        Ok(hire) => hire,
                        Err(refusal) => {
                            return Ok(Step::of(Err(refusal)));
                        }
                    };
                let db = self.realm.store().connection();
                let hamlet = match hires::find(db, hire)? {
                    Some(found) => found.hamlet,
                    None => {
                        return Ok(Step::of(Err(Refusal::RoadEnds)));
                    }
                };
                let name = self.name(hire)?;
                self.carry(call, |cx, _| {
                    Ok(hires::seat(cx, &hamlet, hire, &reason)?.map(
                        |()| {
                            format!(
                                "{name} is the mayor of {hamlet} \
                                 now."
                            )
                        },
                    ))
                })
                .await?
            }
            "remove_hamlet" => {
                let id = HamletId(text(call, "hamlet"));
                self.carry(call, |cx, _| {
                    Ok(hamlets::remove(cx, &id, &reason)?
                        .map(|()| format!("Removed {id}.")))
                })
                .await?
            }
            "read_record" => {
                let id = HamletId(text(call, "hamlet"));
                let db = self.realm.store().connection();
                match hamlets::find(db, &id)? {
                    Some(_) => Ok(record::lines(db, &id, 0)?),
                    None => {
                        Err(Refusal::NoHamlet { id: id.to_string() })
                    }
                }
            }
            other => Err(Refusal::Unsupported {
                what: format!("the tool {other}"),
            }),
        };
        Ok(Step::of(result))
    }

    /// Makes the change of `call` by its agent, and keeps the call's
    /// answer in the same transaction: a call cut off by a crash
    /// after its change is answered the same when it is asked again,
    /// and never changes anything twice.
    async fn carry(
        &mut self,
        call: &Call<'_>,
        make: impl FnOnce(&mut Change<'_>, &Env) -> Done<String>,
    ) -> Result<Result<String, Refusal>> {
        let identity = call.call.identity();
        let request = &call.request;
        self.attempt(
            Actor::Agent(call.conversation.agent),
            |cx, env| {
                let made = make(cx, env)?;
                if let Ok(words) = &made
                    && !crate::fault::is_marker_split()
                {
                    hamlet_store::marker::settle(
                        cx.db,
                        identity,
                        request,
                        &said(words.clone(), false),
                        cx.at,
                    )?;
                }
                Ok(made)
            },
        )
        .await
    }

    /// Marks `call` begun before an effect outside the store, which
    /// no transaction keeps with its answer: cut off by a crash, it
    /// is answered as interrupted when it is asked again, never run
    /// twice.
    fn begin(&mut self, call: &Call<'_>) -> Result<()> {
        let db = self.realm.store().connection();
        hamlet_store::marker::begin(
            db,
            call.call.identity(),
            &call.request,
            crate::clock::now(),
        )
    }

    /// The hamlet `agent` speaks in: a hire's own.
    pub(super) fn hamlet_of(
        &mut self,
        agent: AgentId,
    ) -> Result<Option<HamletId>> {
        let db = self.realm.store().connection();
        Ok(hires::find(db, agent)?.map(|hire| hire.hamlet))
    }

    pub(super) fn name(&mut self, agent: AgentId) -> Result<String> {
        let db = self.realm.store().connection();
        addresses::name_of(db, agent)
    }

    /// The living agent the argument `key` of `call` names.
    pub(super) fn named(
        &mut self,
        call: &Call<'_>,
        key: &str,
        here: Option<&HamletId>,
    ) -> Result<Result<AgentId, Refusal>> {
        let address = match addresses::parse(&text(call, key)) {
            Ok(address) => address,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let db = self.realm.store().connection();
        addresses::resolve(db, &address, here, Reach::Living)
    }

    /// The hire the argument `key` of `call` names, which the caller
    /// leads.
    fn led(
        &mut self,
        call: &Call<'_>,
        key: &str,
        here: Option<&HamletId>,
    ) -> Result<Result<AgentId, Refusal>> {
        let hire = match self.named(call, key, here)? {
            Ok(hire) => hire,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let caller = call.conversation.agent;
        let db = self.realm.store().connection();
        if agents::get(db, hire)?.kind != AgentKind::Hire {
            return Ok(Err(Refusal::RoadEnds));
        }
        if hire == caller
            || !authority::may_lead(db, Actor::Agent(caller), hire)?
        {
            let name = addresses::name_of(db, hire)?;
            return Ok(Err(Refusal::NotLead { name }));
        }
        Ok(Ok(hire))
    }

    /// The scholar `text` addresses, as a hire takes it: at `main`,
    /// or pinned to `@<version>`.
    fn scholar_hiring(
        &mut self,
        text: &str,
    ) -> Result<Result<Hiring, Refusal>> {
        let (address, version) = match text.split_once('@') {
            Some((address, version)) => {
                (address, Some(version.to_owned()))
            }
            None => (text, None),
        };
        let id = match addresses::parse_scholar(address) {
            Ok(id) => id,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let layout = self.realm.layout().clone();
        let db = self.realm.store().connection();
        let Some(scholar) = city::living(db, id)? else {
            return Ok(Err(Refusal::NoScholar {
                address: address.into(),
            }));
        };
        if let Some(version) = &version {
            let found = hamlet_git::Repository::open(
                &layout.repository(scholar.id),
            )
            .ok()
            .and_then(|repo| repo.resolve(version).ok().flatten());
            if found.is_none() || version == hamlet_git::MAIN {
                return Ok(Err(Refusal::NoVersion {
                    name: scholar.label(),
                    version: version.clone(),
                }));
            }
        }
        Ok(Ok(Hiring::Existing {
            scholar: scholar.id,
            pin: version,
        }))
    }

    async fn hire_tool(
        &mut self,
        call: &Call<'_>,
        here: Option<&HamletId>,
    ) -> Result<Result<String, Refusal>> {
        let agent = call.conversation.agent;
        let named_hamlet = text(call, "hamlet");
        let hamlet = match (here, named_hamlet.is_empty()) {
            (Some(here), _) => here.clone(),
            (None, false) => HamletId(named_hamlet),
            (None, true) => {
                return Ok(Err(Refusal::Usage {
                    words: "Say which hamlet to hire into.".into(),
                }));
            }
        };
        let under = text(call, "under");
        let parent = if under.is_empty() {
            let db = self.realm.store().connection();
            if agents::get(db, agent)?.kind == AgentKind::Hand {
                if hamlets::find(db, &hamlet)?.is_none() {
                    return Ok(Err(Refusal::NoHamlet {
                        id: hamlet.to_string(),
                    }));
                }
                crate::society::tree::mayor(db, &hamlet)?
            } else {
                agent
            }
        } else {
            let found =
                match self.named(call, "under", Some(&hamlet))? {
                    Ok(found) => found,
                    Err(refusal) => return Ok(Err(refusal)),
                };
            let db = self.realm.store().connection();
            if found != agent
                && !authority::may_lead(
                    db,
                    Actor::Agent(agent),
                    found,
                )?
            {
                let name = addresses::name_of(db, found)?;
                return Ok(Err(Refusal::NotLead { name }));
            }
            found
        };
        // A scholar of the city, or a new blank one when none fits.
        let (named, blank) =
            (text(call, "scholar"), text(call, "name"));
        let hiring = match (named.is_empty(), blank.is_empty()) {
            (false, true) => match self.scholar_hiring(&named)? {
                Ok(hiring) => hiring,
                Err(refusal) => return Ok(Err(refusal)),
            },
            (true, false) => Hiring::New {
                name: blank,
                character: self.env.settings.character.clone(),
            },
            _ => {
                return Ok(Err(Refusal::Usage {
                    words: "Give `scholar` to hire one of the city, \
                            or `name` to hire a new blank scholar."
                        .into(),
                }));
            }
        };
        let task = self.task_of(call)?;
        let db = self.realm.store().connection();
        if let Hiring::Existing { scholar, .. } = &hiring
            && !authority::may_hire(
                db,
                Actor::Agent(agent),
                *scholar,
                &hamlet,
            )?
        {
            let name = city::get(db, *scholar)?.label();
            return Ok(Err(Refusal::ScholarNotOwn { name }));
        }
        let made = match &hiring {
            Hiring::New { character, .. } => Some(character.clone()),
            Hiring::Existing { .. } => None,
        };
        let overseer = task_chief(db, task)?;
        let owner = authority::owner_for(
            db,
            Actor::Agent(agent),
            Some(&hamlet),
            overseer,
        )?;
        let purpose = text(call, "purpose");
        self.carry(call, |cx, env| {
            let hired = match hires::hire(
                cx,
                Spec {
                    hamlet,
                    parent: Some(parent),
                    hiring,
                    purpose,
                    reason: String::new(),
                    owner,
                },
            )? {
                Ok(hired) => hired,
                Err(refusal) => return Ok(Err(refusal)),
            };
            let name = addresses::name_of(cx.db, hired.agent)?;
            let Some(character) = made else {
                return Ok(Ok(format!("Hired {name}.")));
            };
            let arrival =
                arrived(cx, env, hired.scholar, &character)?;
            Ok(Ok(format!("Hired {name}. {arrival}")))
        })
        .await
    }

    async fn agree_road(
        &mut self,
        call: &Call<'_>,
        here: Option<&HamletId>,
    ) -> Result<Result<String, Refusal>> {
        let agent = call.conversation.agent;
        let db = self.realm.store().connection();
        if !hires::is_mayor(db, agent)? {
            return Ok(Err(Refusal::NotMayor));
        }
        let mine = match self.named(call, "mine", here)? {
            Ok(found) => found,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let theirs = match self.named(call, "theirs", None)? {
            Ok(found) => found,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let reason = text(call, "reason");
        let task = self.task_of(call)?;
        let (one, other) = (self.name(mine)?, self.name(theirs)?);
        self.carry(call, |cx, _| {
            Ok(roads::agree(cx, agent, mine, theirs, &reason, task)?
                .map(|agreed| match agreed {
                    roads::Agreed::Laid(_) => format!(
                        "The road between {one} and {other} is open."
                    ),
                    roads::Agreed::Waiting => format!(
                        "You agree a road between {one} and \
                         {other}; it opens once the other mayor \
                         agrees."
                    ),
                }))
        })
        .await
    }
}

/// What a new scholar `name` comes to, in words for the agent that
/// made it: in the city, or at the harbor, with the Hand asked to
/// judge it when the realm leaves newcomers to the Hand.
fn arrived(
    cx: &mut Change<'_>,
    env: &Env,
    id: ScholarId,
    character: &str,
) -> Result<String> {
    let made = city::get(cx.db, id)?;
    let name = made.label();
    if made.arrival == city::Arrival::Housed {
        return Ok(format!(
            "The scholar {name} lives in the city; hire it next."
        ));
    }
    if city::awaits_hand(cx.db, id)? {
        welcome::judge(cx, env, id, character)?;
    }
    Ok(format!(
        "The scholar {name} is made and waits at the harbor until \
         it is let into the city; you can hire it now."
    ))
}

/// The chief overseeing task `task`.
pub(super) fn task_chief(
    db: &mut hamlet_store::Db,
    task: Option<TaskId>,
) -> Result<Option<ChiefId>> {
    Ok(match task {
        Some(task) => crate::talk::tasks::get(db, task)?.chief,
        None => None,
    })
}

/// The limits of the hamlet `agent` works in.
pub(super) fn limits_of(
    db: &mut hamlet_store::Db,
    hamlet: Option<&HamletId>,
) -> Result<limits::Limits> {
    Ok(match hamlet {
        Some(hamlet) => limits::of(db, hamlet)?,
        None => limits::Limits::default(),
    })
}
