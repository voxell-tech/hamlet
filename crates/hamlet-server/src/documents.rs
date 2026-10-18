//! Reads and edits of the chief's documents, committed before
//! replying.

use std::path::PathBuf;

use anyhow::{Context, Result};
use hamlet_protocol::v0::{
    Address, Edit, EditLimits, Limits, Read, ServerMessage,
};
use serde_json::{Value, json};

use crate::limits::record_limits;
use crate::papers::{CHARTER, cut};
use crate::protocol::{Refusal, reply};
use crate::record::{Actor, Audience, Change, RecordOperation, trim};
use crate::service::{Service, document, save};
use crate::{Operation, Outcome};

impl Service {
    pub(crate) fn edit(
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
                return self.edit_papers(&agent, &text, true);
            }
            Edit::Notes { agent, text, .. } => {
                return self.edit_papers(&agent, &text, false);
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
                let mut value = match self.limits.edit(current, &edit)
                {
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
                let tx = self.home.store().db.transaction()?;
                if let Some(error) = operation.authorize(&tx)? {
                    return Ok(Err(Refusal::Invalid(
                        error.text(Audience::Chief),
                    )));
                }
                if let Outcome::Refused(error) =
                    operation.apply(&tx)?
                {
                    return Ok(Err(Refusal::Invalid(
                        error.text(Audience::Chief),
                    )));
                }
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

    pub(crate) fn edit_papers(
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
            let tx = self.home.store().db.transaction()?;
            if let Some(error) = operation.authorize(&tx)? {
                return Ok(Err(Refusal::Invalid(
                    error.text(Audience::Chief),
                )));
            }
            if let Outcome::Refused(error) = operation.apply(&tx)? {
                return Ok(Err(Refusal::Invalid(
                    error.text(Audience::Chief),
                )));
            }
            tx.commit()?;
        } else if self
            .houses
            .write(&agent.hamlet, member.number, text)
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

    pub(crate) fn say(
        &mut self,
        address: &Address,
        text: &str,
    ) -> Result<std::result::Result<String, Refusal>> {
        if let Err(error) = self.agent(address, "say")? {
            return Ok(Err(error));
        }
        let text = trim(text);
        let (command, tail) = text
            .split_once(char::is_whitespace)
            .unwrap_or((text, ""));
        let tail = trim(tail);
        let mut work = self.work(&address.hamlet)?;
        match command {
            "/budget" => {
                let report = self.budget(&address.hamlet)?;
                let mut lines = vec![
                    report["summary"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                ];
                lines.extend(
                    report["lines"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|v| v.as_str().map(String::from)),
                );
                return Ok(Ok(lines.join("\n")));
            }
            "/reset-budget" => {
                return Ok(Ok(self.reset_budget(&address.hamlet)?));
            }
            "/queue" => {
                return Ok(Ok("Nothing waits for you.".into()));
            }
            "/mode" => {
                if !["manual", "auto"].contains(&tail) {
                    return Ok(Err(Refusal::Invalid(
                        "usage: /mode <manual|auto>".into(),
                    )));
                }
                work["mode"] = tail.into();
                save(
                    self.home.store().connection(),
                    "work",
                    &address.hamlet,
                    &work,
                )?;
                let suffix = if work["folder"].is_null() {
                    " It counts once this hamlet works on a folder."
                } else {
                    ""
                };
                return Ok(Ok(format!(
                    "{}{suffix}",
                    mode_words(tail)
                )));
            }
            "/folder" => {}
            _ => {
                return Ok(Err(Refusal::Unsupported(
                    if text.starts_with('/') {
                        "that command"
                    } else {
                        "conversations"
                    },
                )));
            }
        }
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
        save(
            self.home.store().connection(),
            "work",
            &address.hamlet,
            &work,
        )?;
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

pub(crate) fn read(
    service: &mut Service,
    read: Read,
) -> Result<ServerMessage> {
    let (address, id, full, prompt) = match read {
        Read::Charter { id } => {
            let (text, own) = service.charter()?;
            return Ok(serde_json::from_value(
                json!({"type":"read", "to":id, "charter":{"text":text,"own":own}}),
            )?);
        }
        Read::Models { id, .. } => {
            return service.catalogue_read(&id);
        }
        Read::History { agent, id } => {
            let error = service
                .agent(&agent, "read or edit papers")?
                .err()
                .unwrap_or(Refusal::Unsupported("conversations"));
            return Ok(reply(&id, Err(error)));
        }
        Read::Papers { agent, id, full } => {
            (agent, id, full.unwrap_or(false), false)
        }
        Read::Prompt { agent, id } => (agent, id, true, true),
    };
    let agent =
        match service.agent(&address, "read or edit papers")? {
            Ok(agent) => agent,
            Err(error) => return Ok(reply(&id, Err(error))),
        };
    let notes = service
        .houses
        .read(&address.hamlet, agent.number)
        .unwrap_or_default();
    if !prompt {
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
        return Ok(serde_json::from_value(
            json!({"type":"read", "to":id, "papers":{"character":character,"notes":notes,"skills":[]}}),
        )?);
    }
    let root = service.home.layout().root().to_owned();
    let house = root
        .join("houses")
        .join(&address.hamlet)
        .join(agent.number.to_string());
    let settings = service.settings()?;
    let work = service.work(&address.hamlet)?;
    let limits = service.limits_of(&address.hamlet)?;
    let (charter, _) = service.charter()?;
    let mayor = service
        .home
        .store()
        .record(&address.hamlet)?
        .context("the prompt's hamlet")?
        .mayor
        == agent.number;
    let model =
        crate::state::model(&agent, &limits, &settings, mayor);
    let mut sections = vec![
        charter.trim_end().to_string(),
        format!(
            "# Environment\nSandbox: unavailable. Network for \
             tools: {}. Model: {model}.\nSystem: {}.\nWorkspace: {}",
            if settings["allowNet"] == true {
                "on"
            } else {
                "off unless a chief allows it; a command that needs \
                 it is held until the chief answers, so set \
                 `network` to true on it and carry on"
            },
            if cfg!(target_os = "macos") {
                "macOS, whose shell tools are the BSD ones"
            } else {
                "Linux"
            },
            house.display()
        ),
    ];
    if let Some(folder) = work["folder"].as_str() {
        sections.push(format!(
            "# The project\nYour hamlet works on the folder \
             {folder}. The work a chief asks for is done there."
        ));
    }
    if !trim(&agent.character).is_empty() {
        sections.push(format!(
            "# Your character\n{}",
            cut(
                trim(&agent.character),
                4000,
                "\n[The character is longer than 4000 characters \
                 and is cut here.]"
            )
        ));
    }
    sections.push(
        format!(
            "# NOTES.md\n{}",
            cut(
                &notes,
                8000,
                "\n[NOTES.md is longer than 8000 characters and is \
                 cut here. Shorten it so that all of it is shown.]"
            )
        )
        .trim_end()
        .into(),
    );
    sections.push("# Skills\n(none yet)".into());
    sections.push(format!(
        "# Library\nSkills every agent shares, kept in {}.",
        root.join("library").display()
    ));
    sections
        .push("# Budget\nNo model work has been admitted.".into());
    Ok(serde_json::from_value(
        json!({"type":"read", "to":id, "prompt":sections.join("\n\n")}),
    )?)
}
