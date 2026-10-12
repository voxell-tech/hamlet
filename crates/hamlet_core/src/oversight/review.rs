//! The mayor's review of an instruction a chief gave one of its
//! agents (H10): one model call beside the work, charged to the
//! instruction's task, in which the mayor may object.

use anyhow::Result;
use bevy_ecs::prelude::*;
use hamlet_model::log::Log;
use hamlet_model::provider::Completion;
use hamlet_rules::budget;
use hamlet_rules::chief::rulings;
use hamlet_rules::hamlet::MAYOR;
use serde_json::json;
use tokio::sync::mpsc::{
    UnboundedReceiver, UnboundedSender, unbounded_channel,
};

use super::hamlet;
use super::instruction::{Instruction, mayor_objects};
use crate::core::{HomeDir, Model, Settings, Tokio};

struct Reviewed {
    instruction: Entity,
    told: Instruction,
    /// The task the call is charged to.
    task: String,
    /// The question put to the mayor.
    asked: String,
    /// The size of the call, for a provider that reports none.
    estimate: u64,
    result: Result<Completion>,
}

/// Reviews that have come back from the model.
#[derive(Resource)]
pub(super) struct Reviews {
    tx: UnboundedSender<Reviewed>,
    rx: UnboundedReceiver<Reviewed>,
}

impl Reviews {
    pub(super) fn new() -> Self {
        let (tx, rx) = unbounded_channel();
        Reviews { tx, rx }
    }
}

/// True when the endpoint at `base_url` is on another machine.
pub(super) fn remote(base_url: &str) -> bool {
    let rest = base_url
        .split_once("://")
        .map_or(base_url, |(_, rest)| rest);
    let host = rest.split('/').next().unwrap_or_default();
    // An address in brackets is IPv6; a port follows the bracket.
    let host = match host.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or_default(),
        None => host.split(':').next().unwrap_or_default(),
    };
    !(host.is_empty()
        || host == "localhost"
        || host == "::1"
        || host == "0.0.0.0"
        || host.starts_with("127."))
}

/// Asks the mayor about `told` without holding the work up. Nothing
/// is asked when the budget does not allow the mayor a call.
pub(super) fn ask(
    world: &mut World,
    instruction: Entity,
    told: &Instruction,
) {
    let hamlet = hamlet(world);
    let task =
        told.task.clone().unwrap_or_else(|| budget::new_task(MAYOR));
    if hamlet.stopped(&task).is_some() {
        return;
    }
    let model = hamlet.model_of(MAYOR);
    let home = world.resource::<HomeDir>().0.clone();
    let notes = std::fs::read_to_string(home.notes())
        .unwrap_or_default()
        .chars()
        .take(world.resource::<super::Rules>().review_notes)
        .collect::<String>();
    let agents = hamlet
        .residents()
        .into_iter()
        .map(|r| format!("- {}: {}\n", r.name, r.purpose))
        .collect::<String>();
    let (system, asked) = rulings::review_prompt(
        &format!("# Your agents\n{agents}"),
        &notes,
        &told.agent,
        &told.chief,
        &told.text,
    );
    let provider = world.resource::<Model>().provider();
    let tx = world.resource::<Reviews>().tx.clone();
    let told = told.clone();
    world.resource::<Tokio>().0.spawn(async move {
        let messages = [
            json!({"role": "system", "content": system}),
            json!({"role": "user", "content": asked}),
        ];
        let tools = [rulings::review_definition()];
        let result = provider
            .complete(
                &messages,
                &tools,
                model.as_deref(),
                &mut |_| {},
            )
            .await;
        let estimate = result.as_ref().map_or(0, |done| {
            budget::estimate(&messages, &tools, &done.message)
        });
        let _ = tx.send(Reviewed {
            instruction,
            told,
            task,
            asked,
            estimate,
            result,
        });
    });
}

/// True when a review has come back.
pub(super) fn heard(reviews: Res<Reviews>) -> bool {
    !reviews.rx.is_empty()
}

/// Takes in the reviews that have come back: charges each, and
/// raises the mayor's objection if it made one.
pub(super) fn hear(world: &mut World) {
    while let Ok(reviewed) =
        world.resource_mut::<Reviews>().rx.try_recv()
    {
        // A review that failed is no objection.
        let Ok(done) = reviewed.result else {
            continue;
        };
        let _ = hamlet(world).spend(
            MAYOR,
            &reviewed.task,
            &done.call,
            reviewed.estimate,
        );
        let home = world.resource::<HomeDir>().0.clone();
        let secrets = world.resource::<Settings>().config.keys();
        // The question and the answer are kept with the mayor's
        // sessions.
        let path = home.sessions().join("hamlet_review_main.jsonl");
        if let Ok(mut log) = Log::open(&path, secrets) {
            let _ = log.user(&reviewed.told.chief, &reviewed.asked);
            let _ = log.assistant(&done.message);
            let _ = log.model_call(&done.call);
        }
        if let Some(reason) = rulings::review_reason(&done.message) {
            mayor_objects(
                world,
                reviewed.instruction,
                &reviewed.told,
                &reason,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::remote;

    #[test]
    fn a_model_on_this_machine_is_not_remote() {
        for local in [
            "http://localhost:11434/v1",
            "http://127.0.0.1:8080",
            "http://[::1]:11434/v1",
            "localhost:11434",
            "",
        ] {
            assert!(!remote(local), "{local}");
        }
        for far in [
            "https://api.deepseek.com/v1",
            "https://openrouter.ai/api/v1",
            "http://192.168.1.20:11434/v1",
        ] {
            assert!(remote(far), "{far}");
        }
    }
}
