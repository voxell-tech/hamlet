//! A worker's brief: its one conversation, begun with its task from
//! whoever started it.

use anyhow::Result;
use hamlet_store::ids::{AgentId, ChiefId};

use super::conversations::{self, Begin, Kind};
use super::inputs::{self, InputKind, Keep};
use super::marks::Mark;
use super::tasks::{self, Origin};
use super::{Env, models, runtimes};
use crate::change::Change;
use crate::society::agents;

/// Begins `worker`'s conversation with `brief` from `author`, as a
/// task overseen by `chief`.
pub(crate) fn give(
    cx: &mut Change<'_>,
    env: &Env,
    worker: AgentId,
    brief: &str,
    author: &str,
    chief: Option<ChiefId>,
) -> Result<()> {
    let harness = agents::get(cx.db, worker)?.harness;
    let runtime = runtimes::of_harness(cx.db, &harness)?;
    let choice =
        models::choose(cx.db, &env.settings, &env.overrides, worker)?;
    let conversation = conversations::begin(
        cx,
        Begin::new(worker, Kind::Worker),
        &runtime,
        &choice.model,
        None,
    )?;
    let task =
        tasks::mint(cx, Origin::Worker, worker, chief, Some(brief))?;
    inputs::keep(
        cx,
        Keep {
            conversation,
            request: format!("worker:{worker}"),
            kind: InputKind::Worker,
            task: Some(task),
            content: brief.to_owned(),
            mark: Mark::Worker {
                author: author.to_owned(),
                at: cx.at,
            },
            steer: false,
            boundary: false,
        },
    )?;
    cx.touched.agents.insert(worker);
    Ok(())
}
