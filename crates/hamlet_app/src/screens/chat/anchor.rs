//! Draws the chat's rows at their new heights in the frame they
//! change.

use bevy::app::{App, PostUpdate};
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::query::{With, Without};
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::Query;
use bevy::math::{Affine2, Vec2};
use bevy::ui::{
    ComputedNode, IgnoreScroll, UiGlobalTransform, UiSystems,
};
use bevy_fynix::views::VirtualList;
use hamlet_ui::widgets::shift;

use super::ChatScroll;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        PostUpdate,
        settle
            .after(UiSystems::Layout)
            .before(UiSystems::PostLayout),
    );
}

/// A row of the list as it was laid out, in physical pixels.
struct Laid {
    slot: Entity,
    top: f32,
    /// The height the list gave it.
    given: f32,
    /// The height of what it holds.
    whole: f32,
}

/// Moves the rows and the view to where the list puts them next
/// frame.
fn settle(
    lists: Query<
        (Entity, &VirtualList, &ComputedNode, &Children),
        With<ChatScroll>,
    >,
    tracks: Query<&Children, With<IgnoreScroll>>,
    mut slots: Query<&mut ComputedNode, Without<ChatScroll>>,
    mut transforms: Query<&mut UiGlobalTransform>,
    inside: Query<&Children>,
) {
    for (entity, list, area, kids) in &lists {
        let Some(track) =
            kids.iter().find_map(|&kid| tracks.get(kid).ok())
        else {
            continue;
        };
        let Ok(view) = transforms
            .get(entity)
            .map(|at| at.translation.y - area.size.y / 2.0)
        else {
            continue;
        };
        let mut rows = track
            .iter()
            .filter_map(|&slot| {
                let laid = slots.get(slot).ok()?;
                let top = transforms.get(slot).ok()?.translation.y
                    - laid.size.y / 2.0;
                let given = laid.size.y;
                let whole = match laid.content_size.y {
                    0.0 => given,
                    held => held,
                };
                Some(Laid {
                    slot,
                    top,
                    given,
                    whole,
                })
            })
            .collect::<Vec<_>>();
        if rows.iter().all(|row| row.whole == row.given) {
            continue;
        }
        rows.sort_by(|a, b| a.top.total_cmp(&b.top));
        let seen = area.size.y - area.scrollbar_size.y;
        let scrolled = area.scroll_position.y;
        let grown =
            rows.iter().map(|row| row.whole - row.given).sum::<f32>();
        let furthest = (area.content_size.y + grown - seen).max(0.0);
        let moved = match list.is_following() {
            true => furthest.floor() - scrolled,
            // The first row shown keeps its place as rows above grow.
            false => {
                let above = rows
                    .iter()
                    .filter(|row| row.top + row.given <= view)
                    .map(|row| row.whole - row.given)
                    .sum::<f32>();
                (scrolled + above).clamp(0.0, furthest) - scrolled
            }
        };
        let mut pushed = 0.0;
        for row in &rows {
            let by = pushed - moved;
            if by != 0.0 {
                shift(
                    row.slot,
                    Vec2::new(0.0, by),
                    &mut transforms,
                    &inside,
                );
            }
            if row.whole != row.given {
                if let Ok(mut laid) = slots.get_mut(row.slot) {
                    laid.size.y = row.whole;
                }
                if let Ok(mut at) = transforms.get_mut(row.slot) {
                    let mut grown = Affine2::from(*at);
                    grown.translation.y +=
                        (row.whole - row.given) / 2.0;
                    *at = grown.into();
                }
            }
            pushed += row.whole - row.given;
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::asset::{AssetApp, AssetPlugin};
    use bevy::camera::{
        Camera, Camera2d, ComputedCameraValues, RenderTargetInfo,
    };
    use bevy::ecs::hierarchy::ChildOf;
    use bevy::ecs::world::World;
    use bevy::image::{Image, TextureAtlasLayout};
    use bevy::math::UVec2;
    use bevy::text::TextPlugin;
    use bevy::ui::{Node, ScrollPosition, UiPlugin, px};
    use hamlet_client::chats::Chat;
    use hamlet_protocol::client as protocol;
    use hamlet_ui::theme::HamletTheme;
    use hamlet_ui::widgets::platform::install_fonts;
    use protocol::TranscriptOperation as Op;

    use super::*;
    use crate::screens::chat::{plugin, transcript};

    const REPLY: &str =
        "Hamlet (agent#2) reported 7 agents, not the 6 it said \
         earlier. None is known to be blocked.\n\n| Agent | Role \
         |\n|---|---|\n| agent#2 (Usability) | Rail, badge, chat \
         width, terminal and the chat's timeline |\n| agent#2 \
         (Hamlet Reorg Design) | Design for reorganizing hamlets \
         |\n| agent#2 (Stop and Errors) | Stop, errors and \
         streaming |\n\n- Ended since the last report: nothing.\n- \
         A request between leads: none.\n\nThe lead can ask each \
         agent how far along it is.";

    /// The top and bottom of each row drawn, and of the view.
    fn drawn(world: &mut World) -> (Vec<(f32, f32)>, (f32, f32)) {
        let (list, area, kids) = world
            .query_filtered::<(Entity, &ComputedNode, &Children), With<ChatScroll>>()
            .single(world)
            .unwrap();
        let edges = |world: &World, node: Entity| {
            let size =
                world.get::<ComputedNode>(node).unwrap().size.y;
            let at = world.get::<UiGlobalTransform>(node).unwrap();
            (
                at.translation.y - size / 2.0,
                at.translation.y + size / 2.0,
            )
        };
        let (top, bottom) = edges(world, list);
        let view = (top, bottom - area.scrollbar_size.y);
        let track = kids
            .iter()
            .find(|&&kid| world.get::<IgnoreScroll>(kid).is_some())
            .copied()
            .unwrap();
        let mut rows = world
            .get::<Children>(track)
            .unwrap()
            .iter()
            .map(|&slot| edges(world, slot))
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| a.0.total_cmp(&b.0));
        (rows, view)
    }

    #[test]
    fn a_reply_that_streams_stays_at_the_end_and_leaves_rows_read_still()
     {
        let mut app = crate::testing::panel_app();
        app.add_plugins((
            AssetPlugin::default(),
            bevy::transform::TransformPlugin,
            bevy::input::InputPlugin,
            bevy::picking::PickingPlugin,
            bevy::picking::InteractionPlugin,
            TextPlugin,
            UiPlugin,
            plugin,
            hamlet_ui::markdown::plugin,
        ))
        .init_asset::<Image>()
        .init_asset::<TextureAtlasLayout>()
        .insert_resource(crate::reference::links());
        install_fonts(app.world_mut());
        app.world_mut().spawn((
            Camera2d,
            Camera {
                computed: ComputedCameraValues {
                    target_info: Some(RenderTargetInfo {
                        physical_size: UVec2::splat(1000),
                        scale_factor: 1.0,
                    }),
                    ..Default::default()
                },
                ..Default::default()
            },
        ));
        let hub = crate::testing::hub(&mut app);
        let chat = crate::testing::convo(&app).chat;
        let reference =
            app.world().get::<Chat>(chat).unwrap().0.clone();
        let chat_view = bevy_fynix::mount::<HamletTheme>(
            app.world_mut(),
            transcript(hub),
        );
        let room = app
            .world_mut()
            .spawn(Node {
                width: px(420.0),
                height: px(500.0),
                ..Node::default()
            })
            .id();
        app.world_mut().entity_mut(chat_view).insert(ChildOf(room));
        let send = |app: &mut App, operations| {
            hamlet_client::receive(
                app.world_mut(),
                protocol::ServerMessage::Turn(protocol::Turn {
                    chat: reference.clone(),
                    operations,
                }),
            );
            app.update();
        };
        // A response from a lead, each answered with a reply that
        // streams, the next response queued while it does.
        let input = |turn: usize, queued: bool| protocol::Item {
            id: format!("in:{turn}"),
            at: 1_000,
            kind: protocol::ItemKind::User,
            message: Some(protocol::ItemMessage::Response),
            text: "Status of the hamlet".into(),
            chief: None,
            author: Some("Lead".into()),
            event: None,
            from: None,
            queued,
            rewindable: None,
            tool: None,
            images: Vec::new(),
            files: Vec::new(),
        };
        let reply = |turn: usize| protocol::Item {
            id: format!("reply:{turn}"),
            kind: protocol::ItemKind::Assistant,
            text: String::new(),
            message: None,
            author: None,
            ..input(turn, false)
        };
        // Each frame: the bottom of the last row drawn, the bottom of
        // the view, and the top of the row drawn first.
        let mut frames = Vec::new();
        let look = |app: &mut App| {
            let (rows, view) = drawn(app.world_mut());
            let first = rows.iter().find(|row| row.1 > view.0 + 0.5);
            (rows.last().unwrap().1, view.1, first.unwrap().0)
        };
        let letters = REPLY.chars().collect::<Vec<_>>();
        let mut read_from = None;
        for turn in 0..3 {
            let started = Op::Add {
                item: input(turn, false),
                replaces: (turn > 0).then(|| format!("in:{turn}")),
            };
            let working = Op::Status {
                compacting: false,
                context: None,
                held: false,
                last: None,
                pending: 1,
                working: true,
            };
            send(&mut app, vec![started, working]);
            frames.push(look(&mut app));
            send(
                &mut app,
                vec![Op::Add {
                    item: reply(turn),
                    replaces: None,
                }],
            );
            for (at, words) in letters.chunks(24).enumerate() {
                let mut operations = vec![Op::Append {
                    item: format!("reply:{turn}"),
                    text: words.iter().collect(),
                }];
                if at == 10 {
                    operations.push(Op::Add {
                        item: input(turn + 1, true),
                        replaces: None,
                    });
                }
                // The chief scrolls up to read in the last turn.
                if turn == 2 && at == 5 {
                    let world = app.world_mut();
                    world
                        .query_filtered::<&mut ScrollPosition, With<ChatScroll>>()
                        .single_mut(world)
                        .unwrap()
                        .y -= 200.0;
                    read_from = Some(frames.len());
                }
                send(&mut app, operations);
                frames.push(look(&mut app));
            }
        }

        let read_from = read_from.unwrap();
        let tall = frames
            .iter()
            .position(|&(last, view, _)| last >= view - 0.5)
            .unwrap();
        for &(last, view, _) in &frames[tall..read_from] {
            assert!(
                (last - view).abs() <= 0.5,
                "{last} drawn to {view}"
            );
        }
        let read = frames[read_from].2;
        for &(.., first) in &frames[read_from..] {
            assert!(
                (first - read).abs() <= 0.5,
                "{first} from {read}"
            );
        }
    }
}
