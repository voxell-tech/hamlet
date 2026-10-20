//! Reads and edits of the chief's documents, committed before
//! replying.

use std::path::PathBuf;

use anyhow::{Context, Result};
use hamlet_protocol::v0::{
    Address, Edit, EditLimits, Limits, Read, ServerMessage,
};
use serde_json::{Value, json};

use crate::Outcome;
use crate::limits::record_limits;
use crate::papers::{CHARTER, cut};
use crate::protocol::{Refusal, reply};
use crate::record::{Actor, Audience, Change, RecordOperation, trim};
use crate::service::{Service, document, save};

impl Service {
    pub(crate) fn prepare_notes(
        &mut self,
        agent: &Address,
        text: &str,
    ) -> Result<
        std::result::Result<
            (crate::papers::files::Files, String),
            Refusal,
        >,
    > {
        let member = match self.agent(agent, "read or edit papers")? {
            Ok(agent) => agent,
            Err(error) => return Ok(Err(error)),
        };
        let mut text = Value::String(text.into());
        self.credentials.scrub(&mut text);
        Ok(Ok((
            self.houses.files(&agent.hamlet, member.number)?,
            text.as_str().context("paper text")?.into(),
        )))
    }

    pub(crate) async fn edit(
        &mut self,
        edit: Edit,
    ) -> Result<std::result::Result<String, Refusal>> {
        match edit {
            Edit::Charter { text, .. } => {
                let mut own = text
                    .filter(|t| {
                        !trim(t).is_empty()
                            && trim(t) != trim(CHARTER)
                    })
                    .map(Value::String)
                    .unwrap_or(Value::Null);
                self.credentials.scrub(&mut own);
                save(
                    self.home.store().connection(),
                    "charter",
                    "home",
                    &own,
                )?;
            }
            Edit::Character { agent, text, .. } => {
                return self.edit_papers(&agent, &text, true).await;
            }
            Edit::Notes { agent, text, .. } => {
                return self.edit_papers(&agent, &text, false).await;
            }
            Edit::Limits(edit) => {
                let (hamlet, reason) = match &edit {
                    EditLimits::Set { hamlet, reason, .. }
                    | EditLimits::Whole { hamlet, reason, .. } => {
                        (hamlet, reason)
                    }
                };
                let record =
                    match self.access(hamlet, "edit limits")? {
                        Ok(record) => record,
                        Err(error) => return Ok(Err(error)),
                    };
                let current = self.limits_of(hamlet)?;
                let mut value =
                    match self.limits.edit(current.clone(), &edit) {
                        Ok(value) => value,
                        Err(error) => return Ok(Err(error)),
                    };
                self.credentials.scrub(&mut value);
                let Ok(limits) =
                    serde_json::from_value::<Limits>(value.clone())
                else {
                    return Ok(Err(Refusal::Invalid(
                        "A limit is outside protocol v0's supported \
                         number range."
                            .into(),
                    )));
                };
                if limits.budget.cost.is_some_and(|cost| {
                    cost * 1_000_000.0 >= i64::MAX as f64
                }) || limits.budget.minutes.is_some_and(
                    |minutes| minutes.checked_mul(60_000).is_none(),
                ) {
                    return Ok(Err(Refusal::Invalid(
                        "The budget is outside protocol v0's \
                         supported number range."
                            .into(),
                    )));
                }
                let operation = self.operation(
                    hamlet,
                    Change::SetLimits(record_limits(
                        &limits,
                        &record.limits,
                    )),
                    reason.as_deref().unwrap_or_default(),
                );
                let settings = self.settings()?;
                let tx = self.home.store().transaction()?;
                if let Outcome::Refused(error) =
                    crate::operation::change(&tx, &operation)?
                {
                    return Ok(Err(Refusal::Invalid(
                        error.text(Audience::Chief),
                    )));
                }
                let mut changed = crate::record::read(&tx, hamlet)?
                    .expect("edited record");
                for agent in &record.agents {
                    let mayor = agent.number == record.mayor;
                    let before = crate::state::model(
                        agent, &current, &settings, mayor,
                    );
                    let after = crate::state::model(
                        agent, &value, &settings, mayor,
                    );
                    if before != after {
                        changed
                            .decisions
                            .push(crate::record::Decision {
                            at: operation.at,
                            by: format!("chief:{}", self.chief),
                            reason: reason
                                .clone()
                                .unwrap_or_default(),
                            kind:
                                crate::record::DecisionKind::Model {
                                    agent: agent.number,
                                    model: after,
                                },
                        });
                    }
                }
                crate::record::write(&tx, &changed)?;
                save(&tx, "limits", hamlet, &value)?;
                tx.commit()?;
            }
            Edit::Settings { set, .. } => {
                let previous = self.settings()?;
                let workspace = match crate::settings::validate(&set)
                {
                    Ok(workspace) => workspace,
                    Err(error) => return Ok(Err(error)),
                };
                let mut fields = serde_json::to_value(&set)?;
                fields
                    .as_object_mut()
                    .context("settings edit")?
                    .remove("workspace");
                self.credentials.scrub(&mut fields);
                if !fields
                    .as_object()
                    .context("settings edit")?
                    .is_empty()
                    || workspace.is_none()
                {
                    let mut settings = document(
                        self.home.store().connection(),
                        "settings",
                        "home",
                    )?
                    .unwrap_or_else(crate::settings::defaults);
                    for (key, value) in
                        fields.as_object().context("settings edit")?
                    {
                        settings[key] = value.clone();
                    }
                    settings["saved"] = true.into();
                    save(
                        self.home.store().connection(),
                        "settings",
                        "home",
                        &settings,
                    )?;
                }
                if let Some(workspace) = workspace {
                    self.credentials.choose_workspace(workspace)?;
                }
                if let Some(endpoints) = set.endpoints {
                    for old in previous["endpoints"]
                        .as_array()
                        .into_iter()
                        .flatten()
                    {
                        if let Some(name) = old["name"].as_str()
                            && !endpoints
                                .iter()
                                .any(|e| e.name == name)
                        {
                            self.credentials
                                .save_model(name, None)?;
                        }
                    }
                }
            }
            Edit::Catalogue { model, set, .. } => {
                return self.edit_catalogue(&model, set);
            }
        }
        Ok(Ok("Saved.".into()))
    }

    pub(crate) async fn edit_papers(
        &mut self,
        agent: &Address,
        text: &str,
        character: bool,
    ) -> Result<std::result::Result<String, Refusal>> {
        let member = match self.agent(agent, "read or edit papers")? {
            Ok(agent) => agent,
            Err(error) => return Ok(Err(error)),
        };
        let mut text = Value::String(text.into());
        self.credentials.scrub(&mut text);
        let text = text.as_str().context("paper text")?;
        if character {
            let operation = self.operation(
                &agent.hamlet,
                Change::SetCharacter {
                    number: member.number,
                    text: text.into(),
                },
                "",
            );
            let tx = self.home.store().transaction()?;
            if let Outcome::Refused(error) =
                crate::operation::change(&tx, &operation)?
            {
                return Ok(Err(Refusal::Invalid(
                    error.text(Audience::Chief),
                )));
            }
            tx.commit()?;
        } else if self
            .houses
            .write_notes(&agent.hamlet, member.number, text)
            .await
            .is_err()
        {
            return Ok(Err(Refusal::Invalid(
                "The agent's NOTES.md could not be written safely. \
                 Inspect its house before trying again."
                    .into(),
            )));
        }
        Ok(Ok("Saved.".into()))
    }

    fn operation(
        &self,
        hamlet: &str,
        change: Change,
        reason: &str,
    ) -> RecordOperation {
        RecordOperation {
            hamlet: hamlet.into(),
            actor: Actor::Chief(self.chief.clone()),
            at: chrono::Utc::now().timestamp_millis(),
            reason: reason.into(),
            task: None,
            change,
        }
    }

    pub(crate) fn charter(&mut self) -> Result<(String, bool)> {
        let own = document(
            self.home.store().connection(),
            "charter",
            "home",
        )?;
        Ok(match own.and_then(|v| v.as_str().map(String::from)) {
            Some(text) => (text, true),
            None => (CHARTER.into(), false),
        })
    }

    /// Sets how the hamlet's agents ask before they change its
    /// folder.
    pub(crate) fn set_mode(
        &mut self,
        hamlet: &str,
        mode: &str,
    ) -> Result<String> {
        let mut work = self.work(hamlet)?;
        work["mode"] = mode.into();
        save(self.home.store().connection(), "work", hamlet, &work)?;
        let suffix = if work["folder"].is_null() {
            " It counts once this hamlet works on a folder."
        } else {
            ""
        };
        Ok(format!("{}{suffix}", mode_words(mode)))
    }

    /// Points the hamlet at the folder `tail` names, at none, or
    /// tells which it works on.
    pub(crate) fn folder(
        &mut self,
        hamlet: &str,
        tail: &str,
    ) -> Result<std::result::Result<String, Refusal>> {
        let mut work = self.work(hamlet)?;
        if tail.is_empty() {
            return Ok(Ok(match work["folder"].as_str() {
                Some(folder) => format!(
                    "This hamlet works on `{folder}`. {} `/folder \
                     none` stops that.",
                    mode_words(
                        work["mode"].as_str().unwrap_or("manual")
                    )
                ),
                None => "This hamlet works on no folder. `/folder \
                         <folder>` points it at one."
                    .into(),
            }));
        }
        let folder = if tail == "none" {
            Value::Null
        } else if self.houses.workspace.is_some() {
            if tail != "/project" {
                return Ok(Err(Refusal::Invalid(
                    "Remote workspaces accept only `/folder \
                     /project` or `/folder none`. Local folders are \
                     not uploaded or synchronized."
                        .into(),
                )));
            }
            Value::String("/project".into())
        } else {
            let expanded = if tail == "~" || tail.starts_with("~/") {
                PathBuf::from(
                    std::env::var_os("HOME")
                        .context("no home directory")?,
                )
                .join(tail.strip_prefix("~/").unwrap_or(""))
            } else {
                PathBuf::from(tail)
            };
            if !expanded.is_absolute() {
                return Ok(Err(Refusal::Invalid(
                    "give the folder's full path, such as \
                     /Users/you/code/project"
                        .into(),
                )));
            }
            if !expanded.is_dir() {
                return Ok(Err(Refusal::Invalid(format!(
                    "`{}` is not a folder",
                    expanded.display()
                ))));
            }
            let next = expanded.canonicalize()?;
            let root = self.home.layout().root().canonicalize()?;
            if next.starts_with(&root) || root.starts_with(&next) {
                return Ok(Err(Refusal::Invalid(format!(
                    "a hamlet cannot work on Hamlet's own folder \
                     ({}), or on one that holds it",
                    root.display()
                ))));
            }
            next.to_string_lossy().into_owned().into()
        };
        let previous = work["folder"].clone();
        let before = work["before"]
            .as_array_mut()
            .context("earlier folders")?;
        before.retain(|old| *old != folder);
        if !previous.is_null() && previous != folder {
            before.insert(0, previous);
        }
        work["folder"] = folder.clone();
        save(self.home.store().connection(), "work", hamlet, &work)?;
        Ok(Ok(match folder.as_str() {
            Some(folder) => format!(
                "This hamlet works on `{folder}` now. {}",
                mode_words(work["mode"].as_str().unwrap_or("manual"))
            ),
            None => "This hamlet works on no folder now. Its agents \
                     are back in their own workspaces."
                .into(),
        }))
    }
}

pub(crate) fn mode_words(mode: &str) -> &'static str {
    if mode == "auto" {
        "Agents edit and run commands in the folder without asking."
    } else {
        "Agents ask you before each edit and command."
    }
}

pub(crate) async fn read(
    service: &mut Service,
    read: Read,
) -> Result<ServerMessage> {
    prepare_read(service, read)?.render().await
}

pub(crate) enum Reading {
    Ready(ServerMessage),
    Papers {
        id: String,
        papers: Value,
        remote: Option<crate::workspace::Remote>,
        full: bool,
    },
    Prompt {
        id: String,
        prompt: crate::prompt::Prepared,
    },
}

impl Reading {
    pub async fn render(self) -> Result<ServerMessage> {
        match self {
            Self::Ready(message) => Ok(message),
            Self::Papers {
                id,
                mut papers,
                remote,
                full,
            } => {
                if let Some(remote) = remote {
                    let bytes = remote
                        .read("/house/NOTES.md", true)
                        .await
                        .unwrap_or_default();
                    let notes =
                        String::from_utf8(bytes).unwrap_or_default();
                    papers["notes"] = if full {
                        notes
                    } else {
                        cut(trim(&notes), 6000, "")
                    }
                    .into();
                }
                Ok(serde_json::from_value(
                    json!({"type":"read","to":id,"papers":papers}),
                )?)
            }
            Self::Prompt { id, prompt } => {
                let sections = prompt.render().await?;
                Ok(serde_json::from_value(
                    json!({"type":"read","to":id,"prompt":crate::prompt::text(&sections)}),
                )?)
            }
        }
    }
}

pub(crate) fn prepare_read(
    service: &mut Service,
    read: Read,
) -> Result<Reading> {
    let (address, id, full, prompt) = match read {
        Read::Charter { id } => {
            let (text, own) = service.charter()?;
            return Ok(Reading::Ready(serde_json::from_value(
                json!({"type":"read", "to":id, "charter":{"text":text,"own":own}}),
            )?));
        }
        Read::Models { id, .. } => {
            return service.catalogue_read(&id).map(Reading::Ready);
        }
        Read::History { agent, id } => {
            let error = service
                .agent(&agent, "read or edit papers")?
                .err()
                .unwrap_or(Refusal::Unsupported("conversations"));
            return Ok(Reading::Ready(reply(&id, Err(error))));
        }
        Read::Papers { agent, id, full } => {
            (agent, id, full.unwrap_or(false), false)
        }
        Read::Prompt { agent, id } => (agent, id, true, true),
    };
    let agent =
        match service.agent(&address, "read or edit papers")? {
            Ok(agent) => agent,
            Err(error) => {
                return Ok(Reading::Ready(reply(&id, Err(error))));
            }
        };
    if !prompt {
        let remote = service.houses.remote(
            &address.hamlet,
            agent.number,
            true,
        )?;
        let notes = if remote.is_some() {
            String::new()
        } else {
            service
                .houses
                .read(&address.hamlet, agent.number)
                .unwrap_or_default()
        };
        let character = if full {
            agent.character
        } else {
            cut(trim(&agent.character), 6000, "")
        };
        let notes = if full {
            notes
        } else {
            cut(trim(&notes), 6000, "")
        };
        return Ok(Reading::Papers {
            id,
            papers: json!({"character":character,"notes":notes,"skills":[]}),
            remote,
            full,
        });
    }
    let settings = service.settings()?;
    let limits = service.limits_of(&address.hamlet)?;
    let mayor = service
        .home
        .store()
        .record(&address.hamlet)?
        .context("the prompt's hamlet")?
        .mayor
        == agent.number;
    let model =
        crate::state::model(&agent, &limits, &settings, mayor);
    let instructions = crate::chats::records::instructions(
        service.home.store().connection(),
        &address.hamlet,
    )?;
    let sandbox = service.chats.sandbox.name().to_owned();
    let subject = crate::prompt::Subject {
        hamlet: &address.hamlet,
        number: i64::try_from(agent.number)?,
        model: (!model.is_empty()).then_some(model.as_str()),
        sandbox: &sandbox,
        instructions: &instructions,
        task: None,
        conversation: None,
    };
    let mut prompt = crate::prompt::prepare(service, &subject)?;
    prompt.sections.push(crate::harness::link::Section {
        name: "budget".into(),
        text: crate::prompt::budget(service, &subject)?,
    });
    Ok(Reading::Prompt { id, prompt })
}
