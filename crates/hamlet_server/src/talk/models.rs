//! The model each agent runs (H29), how hard it thinks (H29a) and
//! whether it asks for the provider's fast tier.

use anyhow::Result;
use hamlet_protocol::runtime::ModelRef;
use hamlet_store::Db;
use hamlet_store::ids::AgentId;

use crate::settings::{self, Overrides, Settings};
use crate::society::agents::{self, AgentKind, Effort};
use crate::society::{hand, hires, limits, workers};

/// The model an agent's next call runs, its effort and its tier.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Choice {
    pub model: ModelRef,
    pub effort: Option<Effort>,
    pub fast: bool,
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
    let named = named(db, settings, agent)?;
    let model = if overrides.faux {
        settings::faux_model()
    } else {
        settings::model_ref(settings, &named)
    };
    Ok(Choice {
        model,
        effort: effort_of(db, settings, agent)?,
        fast: agents::get(db, agent)?.fast,
    })
}

/// Whether `agent` runs a model at all: none is named before the
/// chief sets one up, and a conversation begun with none would never
/// start.
pub(crate) fn has_model(
    db: &mut Db,
    settings: &Settings,
    overrides: &Overrides,
    agent: AgentId,
) -> Result<bool> {
    if overrides.faux {
        return Ok(true);
    }
    let model =
        settings::model_ref(settings, &named(db, settings, agent)?);
    Ok(!model.id.is_empty() && !model.provider.is_empty())
}

/// How hard `agent` thinks: its own effort when one was chosen for
/// it, else its starter's for a worker, else the realm's.
pub(crate) fn effort_of(
    db: &mut Db,
    settings: &Settings,
    agent: AgentId,
) -> Result<Option<Effort>> {
    let found = agents::get(db, agent)?;
    if found.effort.is_some() {
        return Ok(found.effort);
    }
    if found.kind == AgentKind::Worker
        && let Some(starter) = workers::get(db, agent)?.starter
    {
        return effort_of(db, settings, starter);
    }
    Ok(settings.effort)
}

fn named(
    db: &mut Db,
    settings: &Settings,
    agent: AgentId,
) -> Result<String> {
    Ok(match agents::get(db, agent)?.kind {
        AgentKind::Hand => {
            hand::model(db)?.unwrap_or_else(|| settings.model.clone())
        }
        AgentKind::Hire => {
            let hire = hires::get(db, agent)?;
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
                    named(db, settings, starter)?
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
