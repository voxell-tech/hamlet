//! Reads and edits of the chief's documents, committed before
//! replying.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use hamlet_protocol::client::{
    Address, Charter, Edit, EditLimits, Papers, Read, ReadResult,
    RequestId, ServerMessage,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Outcome;
use crate::limits::Limits;
use crate::papers::{CHARTER, cut};
use crate::protocol::{Refusable, Refusal, reply};
use crate::record::{Actor, Change, RecordOperation};
use crate::service::{Service, document, save};

impl Service {
    pub(crate) fn prepare_notes(
        &mut self,
        agent: &Address,
        text: &str,
    ) -> Result<Refusable<(crate::papers::files::Files, String)>>
    {
        let member = match self.agent(agent, "read or edit papers")? {
            Ok(agent) => agent,
            Err(error) => return Ok(Err(error)),
        };
        Ok(Ok((
            self.houses.house(&agent.hamlet, member.number)?,
            self.credentials.scrub_text(text),
        )))
    }

    pub(crate) async fn edit(
        &mut self,
        edit: Edit,
    ) -> Result<Refusable<String>> {
        match edit {
            Edit::Charter { text, .. } => {
                let mut own = text
                    .filter(|t| {
                        !t.trim().is_empty()
                            && t.trim() != CHARTER.trim()
                    })
                    .map(Value::String)
                    .unwrap_or(Value::Null);
                self.credentials.scrub(&mut own);
                save(self.db(), "charter", "home", &own)?;
            }
            Edit::Character { agent, text, .. } => {
                return self.edit_papers(&agent, &text, true).await;
            }
            Edit::Notes { agent, text, .. } => {
                return self.edit_papers(&agent, &text, false).await;
            }
            Edit::Limits(edit) => return self.edit_limits(&edit),
            Edit::Settings { set, .. } => {
                return self.edit_settings(set);
            }
            Edit::Catalogue { model, set, .. } => {
                return self.edit_catalogue(&model, set);
            }
        }
        Ok(Ok("Saved.".into()))
    }

    /// The chief's edit of a hamlet's limits, with the model each
    /// agent runs under them kept in the record.
    fn edit_limits(
        &mut self,
        edit: &EditLimits,
    ) -> Result<Refusable<String>> {
        let (hamlet, reason) = match edit {
            EditLimits::Set { hamlet, reason, .. }
            | EditLimits::Whole { hamlet, reason, .. } => {
                (hamlet, reason)
            }
        };
        let record = match self.access(hamlet, "edit limits")? {
            Ok(record) => record,
            Err(error) => return Ok(Err(error)),
        };
        let current = record.limits.chief.clone();
        let mut value = match self.limits.edit(&current, edit) {
            Ok(value) => value,
            Err(error) => return Ok(Err(error)),
        };
        self.credentials.scrub(&mut value);
        let Ok(limits) = serde_json::from_value::<Limits>(value)
        else {
            return Ok(Err(Refusal::OutOfRange("A limit")));
        };
        // The schema keeps both at zero or more; a budget too large
        // for micros or milliseconds reaches the largest `i64`.
        if limits.budget.cost.is_some_and(|cost| {
            crate::spend::micros(cost) == i64::MAX
        }) || limits.budget.minutes.is_some_and(|minutes| {
            crate::clock::minutes(minutes) == i64::MAX
        }) {
            return Ok(Err(Refusal::OutOfRange("The budget")));
        }
        let home = self.settings()?.model;
        let runs = record
            .agents
            .iter()
            .filter_map(|agent| {
                let mayor = agent.number == record.mayor;
                let before = current.model_of(agent, mayor, &home);
                let after = limits.model_of(agent, mayor, &home);
                (before != after).then_some((agent.number, after))
            })
            .collect();
        let operation = self.operation(
            hamlet,
            Change::SetLimits {
                limits: crate::record::Limits {
                    chief: Box::new(limits),
                    road_budget: record.limits.road_budget.clone(),
                },
                runs,
            },
            reason.as_deref().unwrap_or_default(),
        );
        let tx = self.home.store().transaction()?;
        if let Outcome::Refused(error) =
            crate::operation::change(&tx, &operation)?
        {
            return Ok(Err(Refusal::Record(Box::new(error))));
        }
        tx.commit()?;
        Ok(Ok("Saved.".into()))
    }

    /// The chief's edit of the home's settings and its workspace.
    fn edit_settings(
        &mut self,
        set: hamlet_protocol::client::EditSettingsSet,
    ) -> Result<Refusable<String>> {
        let previous = self.settings()?;
        let workspace = match crate::settings::validate(&set) {
            Ok(workspace) => workspace,
            Err(error) => return Ok(Err(error)),
        };
        let edits = set.model.is_some()
            || set.reflect_every.is_some()
            || set.allow_net.is_some()
            || set.endpoints.is_some();
        if edits || workspace.is_none() {
            let db = self.db();
            let mut settings = crate::settings::stored(db)?;
            if let Some(model) = &set.model {
                settings.model = self.credentials.scrub_text(model);
            }
            if let Some(every) = set.reflect_every {
                settings.reflect_every = u64::try_from(every)?;
            }
            if let Some(net) = set.allow_net {
                settings.allow_net = net;
            }
            if let Some(endpoints) = &set.endpoints {
                settings.endpoints = endpoints
                    .iter()
                    .map(|endpoint| {
                        hamlet_protocol::client::Endpoint {
                            name: self
                                .credentials
                                .scrub_text(&endpoint.name),
                            base_url: self
                                .credentials
                                .scrub_text(&endpoint.base_url),
                        }
                    })
                    .collect();
            }
            settings.saved = true;
            if let Err(error) = self.overrides.apply(settings.clone())
            {
                return Ok(Err(Refusal::Invalid(error.to_string())));
            }
            crate::settings::keep(self.db(), &settings)?;
        }
        if let Some(workspace) = workspace {
            self.credentials.choose_workspace(workspace)?;
        }
        if let Some(endpoints) = set.endpoints {
            for old in &previous.endpoints {
                if !endpoints.iter().any(|e| e.name == old.name) {
                    self.credentials.save_model(&old.name, None)?;
                }
            }
        }
        Ok(Ok("Saved.".into()))
    }

    pub(crate) async fn edit_papers(
        &mut self,
        agent: &Address,
        text: &str,
        character: bool,
    ) -> Result<Refusable<String>> {
        let member = match self.agent(agent, "read or edit papers")? {
            Ok(agent) => agent,
            Err(error) => return Ok(Err(error)),
        };
        let text = &self.credentials.scrub_text(text);
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
                return Ok(Err(Refusal::Record(Box::new(error))));
            }
            tx.commit()?;
        } else if self
            .houses
            .write_notes(&agent.hamlet, member.number, text)
            .await
            .is_err()
        {
            return Ok(Err(Refusal::NotesUnwritable));
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
            at: crate::clock::now(),
            reason: reason.into(),
            task: None,
            change,
        }
    }

    pub(crate) fn charter(&mut self) -> Result<(String, bool)> {
        let own = document(self.db(), "charter", "home")?;
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
        mode: Mode,
    ) -> Result<String> {
        let mut work = self.work(hamlet)?;
        work.mode = mode;
        self.keep_work(hamlet, &work)?;
        let suffix = if work.folder.is_none() {
            " It counts once this hamlet works on a folder."
        } else {
            ""
        };
        Ok(format!("{}{suffix}", mode.words()))
    }

    fn keep_work(&mut self, hamlet: &str, work: &Work) -> Result<()> {
        save(self.db(), "work", hamlet, &serde_json::to_value(work)?)
    }

    /// Points the hamlet at the folder `tail` names, at none, or
    /// tells which it works on.
    pub(crate) fn folder(
        &mut self,
        hamlet: &str,
        tail: &str,
    ) -> Result<Refusable<String>> {
        let mut work = self.work(hamlet)?;
        if tail.is_empty() {
            return Ok(Ok(match &work.folder {
                Some(folder) => format!(
                    "This hamlet works on `{folder}`. {} `/folder \
                     none` stops that.",
                    work.mode.words()
                ),
                None => "This hamlet works on no folder. `/folder \
                         <folder>` points it at one."
                    .into(),
            }));
        }
        let folder = if tail == "none" {
            None
        } else if let Some(fixed) = self.houses.fixed_folder() {
            if tail != fixed {
                return Ok(Err(Refusal::Invalid(format!(
                    "Remote workspaces accept only `/folder \
                     {fixed}` or `/folder none`. Local folders are \
                     not uploaded or synchronized."
                ))));
            }
            Some(fixed.into())
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
            Some(next.to_string_lossy().into_owned())
        };
        work.before.retain(|old| Some(old) != folder.as_ref());
        if let Some(previous) = work.folder.take()
            && Some(&previous) != folder.as_ref()
        {
            work.before.insert(0, previous);
        }
        work.folder.clone_from(&folder);
        self.keep_work(hamlet, &work)?;
        Ok(Ok(match folder {
            Some(folder) => format!(
                "This hamlet works on `{folder}` now. {}",
                work.mode.words()
            ),
            None => "This hamlet works on no folder now. Its agents \
                     are back in their own workspaces."
                .into(),
        }))
    }
}

/// What a hamlet works on, and how its agents ask before they change
/// it.
#[derive(
    Clone, Debug, Default, PartialEq, Serialize, Deserialize,
)]
pub(crate) struct Work {
    pub folder: Option<String>,
    pub mode: Mode,
    /// The folders it worked on before, latest first.
    pub before: Vec<String>,
}

impl Work {
    /// Whether its agents ask before each edit and command.
    pub fn is_asking(&self) -> bool {
        self.folder.is_some() && self.mode == Mode::Manual
    }
}

/// Whether agents ask before they change a hamlet's folder.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Mode {
    #[default]
    Manual,
    Auto,
}

impl From<Mode> for hamlet_protocol::client::HamletMode {
    fn from(mode: Mode) -> Self {
        match mode {
            Mode::Manual => Self::Manual,
            Mode::Auto => Self::Auto,
        }
    }
}

impl Mode {
    pub fn words(self) -> &'static str {
        match self {
            Self::Auto => {
                "Agents edit and run commands in the folder without \
                 asking."
            }
            Self::Manual => {
                "Agents ask you before each edit and command."
            }
        }
    }
}

pub(crate) async fn read(
    service: &mut Service,
    read: Read,
) -> Result<ServerMessage> {
    prepare_read(service, read)?.render().await
}

pub(crate) enum Reading {
    Ready(Box<ServerMessage>),
    Papers {
        id: RequestId,
        character: String,
        reader: crate::papers::Reader,
        full: bool,
    },
    Prompt {
        id: RequestId,
        prompt: crate::prompt::Prepared,
    },
}

impl Reading {
    pub async fn render(self) -> Result<ServerMessage> {
        match self {
            Self::Ready(message) => Ok(*message),
            Self::Papers {
                id,
                character,
                reader,
                full,
            } => {
                let notes = reader.notes().await.unwrap_or_default();
                let notes = if full {
                    notes
                } else {
                    cut(notes.trim(), 6000, "")
                };
                Ok(ServerMessage::Read(ReadResult::Papers {
                    to: id,
                    papers: Papers {
                        character,
                        notes,
                        skills: Vec::new(),
                    },
                }))
            }
            Self::Prompt { id, prompt } => {
                let sections = prompt.render().await?;
                Ok(ServerMessage::Read(ReadResult::Prompt {
                    to: id,
                    prompt: crate::prompt::text(&sections),
                }))
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
            return Ok(Reading::Ready(Box::new(
                ServerMessage::Read(ReadResult::Charter {
                    to: id,
                    charter: Charter { text, own },
                }),
            )));
        }
        Read::Models { .. } | Read::History { .. } => {
            bail!("the models and the chats answer this read")
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
                return Ok(Reading::Ready(Box::new(reply(
                    &id,
                    Err(error),
                ))));
            }
        };
    if !prompt {
        let reader = service.houses.reader(
            &address.hamlet,
            agent.number,
            false,
        )?;
        let character = if full {
            agent.character
        } else {
            cut(agent.character.trim(), 6000, "")
        };
        return Ok(Reading::Papers {
            id,
            character,
            reader,
            full,
        });
    }
    let home = service.settings()?.model;
    let record = service
        .home
        .store()
        .record(&address.hamlet)?
        .context("the prompt's hamlet")?;
    let model = record.limits.chief.model_of(
        &agent,
        record.mayor == agent.number,
        &home,
    );
    let instructions = crate::chats::records::instructions(
        service.db(),
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
    prompt.sections.push(hamlet_protocol::runtime::Section {
        name: "budget".into(),
        text: crate::prompt::budget(service, &subject)?,
    });
    Ok(Reading::Prompt { id, prompt })
}
