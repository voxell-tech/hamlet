//! The prompt, built before every model call (C11).

use anyhow::Result;
use diesel::prelude::*;
use hamlet_git::{MAIN, Repository};
use hamlet_protocol::runtime::Section;
use hamlet_store::Db;
use hamlet_store::schema::document;

use super::conversations::Conversation;
use crate::knowledge::{self, CHARACTER};
use crate::layout::Layout;
use crate::society::agents::{self, AgentKind};
use crate::society::{addresses, hires};

/// The charter every agent is given unless a chief wrote its own
/// (C12).
const CHARTER: &str = include_str!("../../assets/charter.md");

/// The sections of `conversation`'s next call.
pub(crate) fn sections(
    db: &mut Db,
    layout: &Layout,
    conversation: &Conversation,
) -> Result<Vec<Section>> {
    if let Some(instructions) = &conversation.instructions {
        return Ok(vec![Section {
            name: "errand".into(),
            text: instructions.clone(),
        }]);
    }
    let mut sections = vec![Section {
        name: "charter".into(),
        text: charter(db)?,
    }];
    let agent = agents::get(db, conversation.agent)?;
    if agent.kind != AgentKind::Worker {
        let lodging = knowledge::lodging(db, layout, agent.id)?;
        let repository =
            Repository::open(&layout.repository(lodging.scholar));
        if let Ok(repository) = repository
            && let Some(tip) = repository.tip(MAIN)?
            && let Some(text) = repository.read(tip, CHARACTER)?
        {
            sections.push(Section {
                name: "character".into(),
                text: String::from_utf8_lossy(&text)
                    .chars()
                    .take(4000)
                    .collect(),
            });
        }
    }
    let place = match agent.kind {
        AgentKind::Hand => {
            "You are the Hand of this realm.".to_owned()
        }
        AgentKind::Hire => {
            let hire = hires::get(db, agent.id)?;
            let name = addresses::name_of(db, agent.id)?;
            format!(
                "You are `{name}`, a scholar of the city, hired \
                 into `{}` to {}.",
                hire.hamlet, hire.purpose
            )
        }
        AgentKind::Worker => {
            "You are a worker started for one task.".into()
        }
    };
    sections.push(Section {
        name: "place".into(),
        text: place,
    });
    Ok(sections)
}

/// The realm's charter: the chief's own, else the default.
pub(crate) fn charter(db: &mut Db) -> Result<String> {
    Ok(document::table
        .find("charter")
        .select(document::value)
        .first::<String>(db)
        .optional()?
        .unwrap_or_else(|| CHARTER.to_owned()))
}
