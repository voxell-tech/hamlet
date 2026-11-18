//! A hamlet's civic record and budget.

use std::collections::HashMap;

use bevy::app::{App, Update};
use bevy::ecs::prelude::*;
use bevy_fynix::views::BehaviorExt;
use bevy_fynix::{ViewExt, derived, every_frame, keyed};
use hamlet_client::messages::{Reconnected, Response};
use hamlet_client::outbox::Outbox;
use hamlet_client::realm::{
    self as agents, Actions, Agents, Bylaws, HamletId, Identity,
    Lead, Lived, LivesIn, Seen, Treasury, Works,
};
use hamlet_protocol::client as protocol;
use hamlet_ui::AnyView;
use hamlet_ui::styles::{bare, group, note};
use hamlet_ui::widgets::icons::Glyph;

use super::heading;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Annals>()
        .add_observer(answered)
        .add_observer(
            |_: On<Reconnected>, mut annals: ResMut<Annals>| {
                annals.reconnect = true;
            },
        )
        .add_systems(Update, refresh);
}

/// The civic records received, each for its hamlet, so Hubs showing
/// different hamlets each read their own.
#[derive(Resource, Default)]
pub struct Annals {
    records: HashMap<Entity, Record>,
    available: bool,
    reconnect: bool,
}

/// A hamlet's civic record, and the request for it under way.
#[derive(Default)]
struct Record {
    pending: Option<protocol::RequestId>,
    revision: Option<Seen>,
    text: String,
}

impl Annals {
    /// A received civic record for a hamlet.
    pub fn received(hamlet: Entity, text: String) -> Self {
        let record = Record {
            text,
            ..Record::default()
        };
        Self {
            records: HashMap::from([(hamlet, record)]),
            ..Self::default()
        }
    }
}

#[derive(Component)]
struct RecordPanel(Entity);

fn request(world: &mut World, hamlet: Entity, opening: bool) {
    if !world.resource::<Actions>().is_supported("history") {
        return;
    }
    let Some(name) =
        world.get::<HamletId>(hamlet).map(|h| h.0.clone())
    else {
        return;
    };
    let revision = world.get_resource::<Seen>().copied();
    let annals = world.resource::<Annals>();
    if !opening
        && annals
            .records
            .get(&hamlet)
            .is_some_and(|record| record.revision == revision)
    {
        return;
    }
    let Some(to) = agents::lead_of(world, &name)
        .and_then(|lead| agents::id_of(world, lead))
    else {
        return;
    };
    let id = world.resource_mut::<Outbox>().send(|id| {
        protocol::ClientMessage::Say(protocol::Say {
            files: Vec::new(),
            id,
            images: Vec::new(),
            to,
            text: "/history".into(),
        })
    });
    let mut annals = world.resource_mut::<Annals>();
    annals.available = true;
    let record = annals.records.entry(hamlet).or_default();
    record.pending = Some(id);
    record.revision = revision;
}

fn answered(response: On<Response>, mut annals: ResMut<Annals>) {
    let text = match &response.message {
        protocol::ServerMessage::Reply(reply) => &reply.text,
        protocol::ServerMessage::Fault(fault) => &fault.text,
        _ => return,
    };
    let Some(record) = annals
        .records
        .values_mut()
        .find(|record| record.pending.as_ref() == Some(&response.to))
    else {
        return;
    };
    record.text.clone_from(text);
    record.pending = None;
}

fn refresh(world: &mut World) {
    let available =
        world.resource::<Actions>().is_supported("history");
    let mut annals = world.resource_mut::<Annals>();
    let force = annals.reconnect || (available && !annals.available);
    annals.available = available;
    annals.reconnect = false;

    let panels = world
        .query::<&RecordPanel>()
        .iter(world)
        .map(|p| p.0)
        .collect::<Vec<_>>();
    let changed = world
        .query_filtered::<Entity, (
            With<HamletId>,
            Or<(
                Changed<Name>,
                Changed<Agents>,
                Changed<Works>,
                Changed<Bylaws>,
                Changed<Treasury>,
            )>,
        )>()
        .iter(world)
        .collect::<Vec<_>>();
    let agents = world
        .query_filtered::<&LivesIn, (
            With<Identity>,
            Or<(Changed<Name>, Changed<Lived>, Changed<Lead>)>,
        )>()
        .iter(world)
        .map(|l| l.0)
        .collect::<Vec<_>>();
    for hamlet in panels {
        if force
            || changed.contains(&hamlet)
            || agents.contains(&hamlet)
        {
            request(world, hamlet, force);
        }
    }
}

/// The civic record and spending reported for a hamlet.
pub(super) fn record_of(hamlet: Entity) -> AnyView {
    let told = derived(move |world: &World| {
        let annals = world.resource::<Annals>();
        (
            world
                .get::<Treasury>(hamlet)
                .map(|t| t.0.lines.clone())
                .unwrap_or_default(),
            annals
                .records
                .get(&hamlet)
                .map(|record| record.text.clone())
                .unwrap_or_default(),
            world.resource::<Actions>().is_supported("history"),
        )
    })
    .when(every_frame());
    let body = keyed(told, move |(lines, record, records)| {
        let mut parts = Vec::new();
        if *records {
            parts.push(heading(Glyph::History, "Who came and went"));
            parts.push(
                note(if record.is_empty() {
                    "Reading the record...".into()
                } else {
                    record.clone()
                })
                .boxed(),
            );
        }
        parts.push(heading(Glyph::Meter, "The budget"));
        parts.extend(
            lines.iter().map(|line| note(line.clone()).boxed()),
        );
        group(parts).tagged(RecordPanel(hamlet)).boxed()
    })
    .within(bare());
    AnyView::new(move |cx: &mut hamlet_ui::Cx| {
        request(cx.world, hamlet, true);
        cx.build(body)
    })
}

#[cfg(test)]
mod tests {
    use hamlet_client::outbox::Outbox;
    use hamlet_protocol::client as protocol;

    use super::*;
    use crate::testing::{panel_app, words};

    #[test]
    fn the_record_reply_comes_from_the_server_and_commands_reach_its_lead()
     {
        let mut app = panel_app();
        app.add_plugins(crate::overview::plugin);
        let mut state = hamlet_testing::realm::state();
        state.actions = hamlet_testing::realm::actions(&["history"]);
        state
            .agents
            .push(hamlet_testing::realm::agent(3, "home", false));
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state.clone()),
        );
        let hamlet =
            hamlet_client::realm::first_lead(app.world()).unwrap();
        let hamlet = app.world().get::<LivesIn>(hamlet).unwrap().0;
        bevy_fynix::mount::<hamlet_ui::theme::HamletTheme>(
            app.world_mut(),
            record_of(hamlet),
        );
        app.update();
        let record = app
            .world()
            .resource::<Outbox>()
            .iter()
            .find_map(|message| match message {
                protocol::ClientMessage::Say(say)
                    if say.text == "/history" =>
                {
                    Some(say.clone())
                }
                _ => None,
            })
            .expect("the record requested on opening");
        assert_eq!(record.to, protocol::AgentId(2));
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::Reply(protocol::Reply {
                to: record.id,
                kind: protocol::ReplyKind::Notice,
                text: "Scout arrived, and smith was restored.".into(),
                upload: None,
                image: None,
                images: Vec::new(),
                file: None,
                files: Vec::new(),
            }),
        );
        app.update();
        assert!(
            words(&mut app).iter().any(
                |s| s == "Scout arrived, and smith was restored."
            )
        );
        state.hamlets[0].name = "The Hollow".into();
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::Change(protocol::Changes {
                epoch: 1,
                revision: 1,
                changes: vec![protocol::Update::Hamlet {
                    hamlet: state.hamlets.remove(0),
                }],
            }),
        );
        app.update();
        assert_eq!(app.world().resource::<Outbox>().iter().filter(|m| matches!(m, protocol::ClientMessage::Say(s) if s.text == "/history")).count(), 2);
        state.agents[1].lead = false;
        state.agents[2].lead = true;
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::Change(protocol::Changes {
                epoch: 1,
                revision: 2,
                changes: state
                    .agents
                    .into_iter()
                    .map(|agent| protocol::Update::Agent { agent })
                    .collect(),
            }),
        );
        app.update();
        assert_eq!(app.world().resource::<Outbox>().iter().filter(|m| matches!(m, protocol::ClientMessage::Say(s) if s.text == "/history")).count(), 3);
    }
    #[test]
    fn a_record_lost_at_reconnect_is_requested_again_even_without_realm_changes()
     {
        let mut app = panel_app();
        app.add_plugins(crate::overview::plugin);
        let mut state = hamlet_testing::realm::state();
        state.actions = hamlet_testing::realm::actions(&["history"]);
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state.clone()),
        );
        let agent =
            hamlet_client::realm::first_lead(app.world()).unwrap();
        let hamlet = app.world().get::<LivesIn>(agent).unwrap().0;
        bevy_fynix::mount::<hamlet_ui::theme::HamletTheme>(
            app.world_mut(),
            record_of(hamlet),
        );
        app.update();
        let first = app
            .world_mut()
            .resource_mut::<Outbox>()
            .drain()
            .filter_map(|m| match m {
                protocol::ClientMessage::Say(s)
                    if s.text == "/history" =>
                {
                    Some(s.id)
                }
                _ => None,
            })
            .last()
            .unwrap();
        app.world_mut().trigger(Reconnected);
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state.clone()),
        );
        app.update();
        let retry = app
            .world_mut()
            .resource_mut::<Outbox>()
            .drain()
            .find_map(|m| match m {
                protocol::ClientMessage::Say(s)
                    if s.text == "/history" =>
                {
                    Some(s.id)
                }
                _ => None,
            })
            .expect("request again after reconnect");
        assert_ne!(first, retry);
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::Reply(protocol::Reply {
                to: retry,
                kind: protocol::ReplyKind::Notice,
                text: "The record recovered".into(),
                upload: None,
                image: None,
                images: Vec::new(),
                file: None,
                files: Vec::new(),
            }),
        );
        app.update();
        assert!(
            words(&mut app).contains(&"The record recovered".into())
        );
        state.actions = hamlet_testing::realm::actions(&[]);
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state.clone()),
        );
        app.update();
        app.world_mut()
            .resource_mut::<Outbox>()
            .drain()
            .for_each(drop);
        state.actions = hamlet_testing::realm::actions(&["history"]);
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        app.update();
        assert!(app.world().resource::<Outbox>().iter().any(|m| matches!(m,protocol::ClientMessage::Say(s) if s.text == "/history")));
    }
}
