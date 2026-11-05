//! The model each agent runs (H29) and how hard it thinks (H29a).

use anyhow::Result;
use hamlet_protocol::runtime::ModelRef;
use hamlet_store::Db;
use hamlet_store::ids::AgentId;

use crate::settings::{self, Overrides, Settings};
use crate::society::agents::{self, AgentKind};
use crate::society::hires::Effort;
use crate::society::{hires, limits, workers};

/// The model an agent's next call runs, and its effort.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Choice {
    pub model: ModelRef,
    pub effort: Option<Effort>,
}

/// What `agent` runs now: its own model when one was chosen for it,
/// else its hamlet's, else the realm's; a worker its starter's unless
/// it was given one. A faux realm runs the faux model alone.
pub(crate) fn choose(
    db: &mut Db,
    settings: &Settings,
    overrides: &Overrides,
    agent: AgentId,
) -> Result<Choice> {
    let mut effort = settings.effort;
    let named = named(db, settings, agent, &mut effort)?;
    let model = if overrides.faux {
        settings::faux_model()
    } else {
        settings::model_ref(settings, &named)
    };
    Ok(Choice { model, effort })
}

fn named(
    db: &mut Db,
    settings: &Settings,
    agent: AgentId,
    effort: &mut Option<Effort>,
) -> Result<String> {
    Ok(match agents::get(db, agent)?.kind {
        AgentKind::Hand => settings.model.clone(),
        AgentKind::Hire => {
            let hire = hires::get(db, agent)?;
            if hire.effort.is_some() {
                *effort = hire.effort;
            }
            match hire.model {
                Some(model) => model,
                None => limits::of(db, &hire.hamlet)?
                    .model
                    .unwrap_or_else(|| settings.model.clone()),
            }
        }
        AgentKind::Worker => {
            let worker = workers::get(db, agent)?;
            match (worker.model, worker.starter) {
                (Some(model), _) => model,
                (None, Some(starter)) => {
                    named(db, settings, starter, effort)?
                }
                (None, None) => match &worker.hamlet {
                    Some(hamlet) => limits::of(db, hamlet)?
                        .model
                        .unwrap_or_else(|| settings.model.clone()),
                    None => settings.model.clone(),
                },
            }
        }
    })
}
