//! A hamlet's received votes, civic record and budget.

use bevy::app::{App, Update};
use bevy::ecs::prelude::*;
use bevy::ui::{UiRect, px};
use bevy_fynix::views::{BehaviorExt, FrameProps, column};
use bevy_fynix::{ViewExt, derived, every_frame, keyed};
use hamlet_client::messages::{Reconnected, Response};
use hamlet_client::outbox::Outbox;
use hamlet_client::realm::{
    Bylaws, Citizen, Citizens, Deeds, Lived, LivesIn, Mayor, Seen,
    Settlement, Treasury, Votes, Works,
};
use hamlet_protocol::v0;

use super::{heading, tell};
use crate::AnyView;
use crate::icons::Glyph;
use crate::looks::{bad_action, bare, note};

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Annals>()
        .add_observer(answered)
        .add_observer(
            |_: On<Reconnected>, mut annals: ResMut<Annals>| {
                annals.reconnect = true
            },
        )
        .add_systems(Update, refresh);
}

/// The civic record received for the displayed hamlet.
#[derive(Resource, Default)]
pub struct Annals {
    hamlet: Option<Entity>,
    pending: Option<String>,
    revision: Option<Seen>,
    text: String,
    available: bool,
    reconnect: bool,
}

impl Annals {
    /// A received civic record for a hamlet.
    pub fn received(hamlet: Entity, text: String) -> Self {
        Self {
            hamlet: Some(hamlet),
            text,
            ..Self::default()
        }
    }
}

#[derive(Component)]
struct RecordPanel(Entity);

fn request(world: &mut World, hamlet: Entity, opening: bool) {
    if !world.resource::<Deeds>().has("record") {
        return;
    }
    let Some(name) =
        world.get::<Settlement>(hamlet).map(|h| h.name.clone())
    else {
        return;
    };
    let revision = world.get_resource::<Seen>().copied();
    let annals = world.resource::<Annals>();
    if !opening
        && annals.hamlet == Some(hamlet)
        && annals.revision == revision
    {
        return;
    }
    let id = world.resource_mut::<Outbox>().send(|id| {
        v0::ClientMessage::Say(v0::Say {
            id,
            to: v0::Address {
                hamlet: name,
                agent: "mayor".into(),
            },
            text: "/record".into(),
        })
    });
    let mut annals = world.resource_mut::<Annals>();
    if annals.hamlet != Some(hamlet) {
        annals.text.clear();
    }
    annals.hamlet = Some(hamlet);
    annals.pending = Some(id);
    annals.available = true;
    annals.revision = revision;
}

fn answered(response: On<Response>, mut annals: ResMut<Annals>) {
    if annals.pending.as_ref() != Some(&response.to) {
        return;
    }
    let text = match &response.message {
        v0::ServerMessage::Reply(reply) => &reply.text,
        v0::ServerMessage::Fault(fault) => &fault.text,
        _ => return,
    };
    annals.text.clone_from(text);
    annals.pending = None;
}

fn refresh(world: &mut World) {
    let available = world.resource::<Deeds>().has("record");
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
            With<Settlement>,
            Or<(
                Changed<Name>,
                Changed<Citizens>,
                Changed<Works>,
                Changed<Bylaws>,
                Changed<Treasury>,
                Changed<Votes>,
            )>,
        )>()
        .iter(world)
        .collect::<Vec<_>>();
    let agents = world
        .query_filtered::<&LivesIn, (
            With<Citizen>,
            Or<(Changed<Name>, Changed<Lived>, Changed<Mayor>)>,
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

/// The votes, civic record and spending reported for a hamlet.
pub(super) fn record_of(hamlet: Entity) -> AnyView {
    let told = derived(move |world: &World| {
        let annals = world.resource::<Annals>();
        (
            world
                .get::<Votes>(hamlet)
                .map(|v| v.0.clone())
                .unwrap_or_default(),
            world
                .get::<Treasury>(hamlet)
                .map(|t| t.0.lines.clone())
                .unwrap_or_default(),
            if annals.hamlet == Some(hamlet) {
                annals.text.clone()
            } else {
                String::new()
            },
            world.resource::<Deeds>().has("overturn"),
            world.resource::<Deeds>().has("record"),
        )
    })
    .when(every_frame());
    let body = keyed(
        told,
        move |(votes, lines, record, overturn, records)| {
            let mut parts = vec![heading(
                Glyph::Ballot,
                format!("VOTES ({})", votes.len()),
            )];
            for vote in votes {
                parts.push(note(vote.report.clone()).boxed());
                if vote.overturnable && *overturn {
                    let id = vote.id;
                    parts.push(bad_action(
                        Glyph::Restore,
                        "OVERTURN",
                        move |world| {
                            if world
                                .resource::<Deeds>()
                                .has("overturn")
                            {
                                tell(
                                    world,
                                    hamlet,
                                    format!("/overturn {id}"),
                                );
                            }
                        },
                    ));
                }
            }
            if *records {
                parts.push(heading(
                    Glyph::History,
                    "WHO CAME AND WENT",
                ));
                parts.push(
                    note(if record.is_empty() {
                        "Reading the record...".into()
                    } else {
                        record.clone()
                    })
                    .boxed(),
                );
            }
            parts.push(heading(Glyph::Meter, "THE BUDGET"));
            parts.extend(
                lines.iter().map(|line| note(line.clone()).boxed()),
            );
            column(parts)
                .gap(8.0)
                .padding(UiRect::top(px(4.0)))
                .tagged(RecordPanel(hamlet))
                .boxed()
        },
    )
    .within(bare());
    AnyView::new(move |cx: &mut crate::Cx| {
        request(cx.world, hamlet, true);
        cx.build(body)
    })
}

#[cfg(test)]
mod tests {
    use hamlet_client::outbox::Outbox;
    use hamlet_protocol::v0;

    use super::*;
    use crate::me::tests::{panel_app, press, words};

    #[test]
    fn votes_and_the_record_reply_come_from_the_server_and_commands_reach_its_mayor()
     {
        let mut app = panel_app();
        app.add_plugins(crate::overview::plugin);
        let mut state = hamlet_testing::realm::state();
        state.deeds = Some(vec!["record".into(), "overturn".into()]);
        state
            .agents
            .push(hamlet_testing::realm::agent("home", 1, "scout"));
        state.hamlets[0].votes = Some(vec![
            v0::VoteView {
                id: 7,
                motion: "scout for mayor".into(),
                by: "chief".into(),
                open: false,
                overturnable: true,
                report: "Scout became mayor.".into(),
                voters: vec![],
            },
            v0::VoteView {
                id: 8,
                motion: "found mill".into(),
                by: "scout".into(),
                open: false,
                overturnable: false,
                report: "The founding vote failed.".into(),
                voters: vec![],
            },
        ]);
        hamlet_client::receive(
            app.world_mut(),
            v0::ServerMessage::State(state.clone()),
        );
        let hamlet =
            hamlet_client::realm::mayor(app.world_mut()).unwrap();
        let hamlet = app
            .world()
            .get::<hamlet_client::realm::LivesIn>(hamlet)
            .unwrap()
            .0;
        bevy_fynix::mount::<crate::theme::HamletTheme>(
            app.world_mut(),
            record_of(hamlet),
        );
        app.update();
        let shown = words(&mut app);
        assert!(shown.iter().any(|s| s == "VOTES (2)"));
        assert!(shown.iter().any(|s| s == "Scout became mayor."));
        assert!(
            shown.iter().any(|s| s == "The founding vote failed.")
        );
        assert_eq!(
            shown.iter().filter(|s| *s == "OVERTURN").count(),
            1
        );
        let record = app
            .world()
            .resource::<Outbox>()
            .iter()
            .find_map(|message| match message {
                v0::ClientMessage::Say(say)
                    if say.text == "/record" =>
                {
                    Some(say.clone())
                }
                _ => None,
            })
            .expect("the record requested on opening");
        assert_eq!(record.to.agent, "mayor");
        hamlet_client::receive(
            app.world_mut(),
            v0::ServerMessage::Reply(v0::Reply {
                to: record.id,
                kind: v0::ReplyKind::Notice,
                text: "Scout arrived, and smith was restored.".into(),
                upload: None,
            }),
        );
        for _ in 0..3 {
            app.update();
        }
        assert!(
            words(&mut app).iter().any(
                |s| s == "Scout arrived, and smith was restored."
            )
        );
        press(&mut app, "OVERTURN");
        assert!(app.world().resource::<Outbox>().iter().any(|m| matches!(m, v0::ClientMessage::Say(s) if s.text == "/overturn 7" && s.to.hamlet == "home")));
        state.hamlets[0].name = "The Hollow".into();
        hamlet_client::receive(
            app.world_mut(),
            v0::ServerMessage::Change(v0::Changes {
                epoch: 1,
                revision: 1,
                changes: vec![v0::Update::Hamlet {
                    hamlet: state.hamlets.remove(0),
                }],
            }),
        );
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(app.world().resource::<Outbox>().iter().filter(|m| matches!(m, v0::ClientMessage::Say(s) if s.text == "/record")).count(), 2);
        state.agents[0].mayor = false;
        state.agents[1].mayor = true;
        hamlet_client::receive(
            app.world_mut(),
            v0::ServerMessage::Change(v0::Changes {
                epoch: 1,
                revision: 2,
                changes: state
                    .agents
                    .into_iter()
                    .map(|agent| v0::Update::Agent { agent })
                    .collect(),
            }),
        );
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(app.world().resource::<Outbox>().iter().filter(|m| matches!(m, v0::ClientMessage::Say(s) if s.text == "/record")).count(), 3);
    }
    #[test]
    fn a_record_lost_at_reconnect_is_requested_again_even_without_realm_changes()
     {
        let mut app = panel_app();
        app.add_plugins(crate::overview::plugin);
        let mut state = hamlet_testing::realm::state();
        state.deeds = Some(vec!["record".into()]);
        hamlet_client::receive(
            app.world_mut(),
            v0::ServerMessage::State(state.clone()),
        );
        let agent =
            hamlet_client::realm::mayor(app.world_mut()).unwrap();
        let hamlet = app.world().get::<LivesIn>(agent).unwrap().0;
        bevy_fynix::mount::<crate::theme::HamletTheme>(
            app.world_mut(),
            record_of(hamlet),
        );
        app.update();
        let first = app
            .world_mut()
            .resource_mut::<Outbox>()
            .drain()
            .filter_map(|m| match m {
                v0::ClientMessage::Say(s) if s.text == "/record" => {
                    Some(s.id)
                }
                _ => None,
            })
            .last()
            .unwrap();
        app.world_mut()
            .trigger(hamlet_client::messages::Reconnected);
        hamlet_client::receive(
            app.world_mut(),
            v0::ServerMessage::State(state.clone()),
        );
        app.update();
        let retry = app
            .world_mut()
            .resource_mut::<Outbox>()
            .drain()
            .find_map(|m| match m {
                v0::ClientMessage::Say(s) if s.text == "/record" => {
                    Some(s.id)
                }
                _ => None,
            })
            .expect("request again after reconnect");
        assert_ne!(first, retry);
        hamlet_client::receive(
            app.world_mut(),
            v0::ServerMessage::Reply(v0::Reply {
                to: retry,
                kind: v0::ReplyKind::Notice,
                text: "The record recovered".into(),
                upload: None,
            }),
        );
        app.update();
        app.update();
        assert!(
            words(&mut app).contains(&"The record recovered".into())
        );
        state.deeds = Some(vec![]);
        hamlet_client::receive(
            app.world_mut(),
            v0::ServerMessage::State(state.clone()),
        );
        app.update();
        app.world_mut()
            .resource_mut::<Outbox>()
            .drain()
            .for_each(drop);
        state.deeds = Some(vec!["record".into()]);
        hamlet_client::receive(
            app.world_mut(),
            v0::ServerMessage::State(state),
        );
        app.update();
        assert!(app.world().resource::<Outbox>().iter().any(|m| matches!(m,v0::ClientMessage::Say(s) if s.text == "/record")));
    }
}
