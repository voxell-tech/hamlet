//! Markdown in the chat: a transcript entry's blocks and its
//! pictures.

mod picture;

use std::sync::{Arc, Mutex};

use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use bevy_fynix::{Signal, component};
use hamlet_client::chats::{Entry, Transcript};
use hamlet_ui::markdown::{
    Block, Reading, hashed, is_address_in, unsettle,
};

pub(crate) use self::picture::picture_view;

fn words<'a>(
    world: &'a World,
    chat: Entity,
    id: &str,
) -> Option<&'a str> {
    match &world.get::<Transcript>(chat)?.item(id)?.entry {
        Entry::Assistant(text)
        | Entry::Agent { text, .. }
        | Entry::Summary(text) => Some(text),
        _ => None,
    }
}

/// Whether entry `id` is the last unqueued one of an answer under
/// way.
fn is_growing(world: &World, chat: Entity, id: &str) -> bool {
    world.get::<Transcript>(chat).is_some_and(|transcript| {
        transcript.is_answering()
            && transcript
                .items()
                .iter()
                .rfind(|item| !item.queued)
                .map(|item| item.id.as_str())
                == Some(id)
    })
}

/// A transcript entry's blocks, parsed again only when it changes.
pub(crate) fn entry(
    chat: Entity,
    id: String,
) -> Signal<Vec<Arc<Block>>> {
    let mut moved = component::<Transcript, _>(chat, |_| ());
    let read = Arc::new(Mutex::new(Read::default()));
    let shared = read.clone();
    let read_id = id.clone();
    Signal::new(
        move |world: &World| {
            let mut read = shared.lock().unwrap();
            read.update(world, chat, &read_id);
            read.blocks.clone()
        },
        move |world: &World| {
            moved.changed(world)
                && read.lock().unwrap().update(world, chat, &id)
        },
    )
}

/// What an entry's blocks were made from, and the blocks.
#[derive(Default)]
struct Read {
    /// The length, hash and growth of the words read.
    seen: Option<Option<(usize, u64, bool)>>,
    reading: Reading,
    blocks: Vec<Arc<Block>>,
}

impl Read {
    /// True if the blocks are not those of the words as they are now.
    fn update(
        &mut self,
        world: &World,
        chat: Entity,
        id: &str,
    ) -> bool {
        let now = words(world, chat, id);
        let grows = is_growing(world, chat, id);
        let stamp = now.map(|text| (text.len(), hashed(text), grows));
        if self.seen == Some(stamp) {
            return false;
        }
        self.seen = Some(stamp);
        self.blocks = now
            .map(|text| self.reading.read(text, is_address_in(world)))
            .unwrap_or_default();
        if grows {
            unsettle(&mut self.blocks);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use bevy::app::App;
    use bevy::text::TextSpan;
    use bevy::time::TimePlugin;
    use bevy_fynix::tokens::Tone;
    use bevy_fynix::{FynixPlugin, mount};
    use hamlet_client::chats::Item;
    use hamlet_ui::markdown::{Anchor, PathLinks, document, fixed};
    use hamlet_ui::theme::{self, HamletTheme};

    use super::*;
    use crate::links::{Roots, seek};

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((
            TimePlugin,
            FynixPlugin::<HamletTheme>::default(),
        ))
        .insert_resource(PathLinks {
            resolve: seek,
            generation: 0,
        })
        .insert_resource(crate::reference::links());
        theme::show(app.world_mut(), HamletTheme::designed());
        app.update();
        app
    }

    /// A chat where each of `replies` answers a message.
    fn talk(
        app: &mut App,
        replies: &[&str],
    ) -> (Entity, Vec<String>) {
        let items = replies
            .iter()
            .enumerate()
            .flat_map(|(at, reply)| {
                [
                    Item {
                        id: format!("{at}:user"),
                        at: 0,
                        queued: false,
                        entry: Entry::Chief {
                            name: "a".into(),
                            text: "go".into(),
                            images: Vec::new(),
                            files: Vec::new(),
                        },
                    },
                    Item {
                        id: format!("{at}:reply"),
                        at: 0,
                        queued: false,
                        entry: Entry::Assistant((*reply).into()),
                    },
                ]
            })
            .collect::<Vec<_>>();
        let ids = items.iter().map(|item| item.id.clone()).collect();
        (
            app.world_mut()
                .spawn(
                    Transcript::from_items(items)
                        .with_answering(true),
                )
                .id(),
            ids,
        )
    }

    fn spans(app: &mut App) -> Vec<(Entity, String)> {
        let world = app.world_mut();
        world
            .query::<(Entity, &TextSpan)>()
            .iter(world)
            .map(|(entity, span)| (entity, span.0.clone()))
            .collect()
    }

    #[test]
    fn a_path_is_a_link_where_its_file_is_and_its_word_is_whole() {
        let folder = hamlet_testing::Scratch::new("markdown-links");
        std::fs::write(folder.join("plan.md"), "").unwrap();
        let mut app = app();
        app.insert_resource(Roots::new([folder.to_path_buf()]));
        let (chat, ids) = talk(
            &mut app,
            &["Read plan.md and gone.md, then plan.md"],
        );
        mount::<HamletTheme>(
            app.world_mut(),
            document(entry(chat, ids[1].clone()), Tone::Body),
        );
        app.update();
        let said = |app: &mut App| {
            let mut spans = spans(app)
                .into_iter()
                .map(|(span, text)| {
                    (text, app.world().get::<Anchor>(span).cloned())
                })
                .collect::<Vec<_>>();
            spans.sort();
            spans
        };
        let file = Some(Anchor::File(folder.join("plan.md")));
        let no = |text: &str| (text.to_string(), None);
        assert_eq!(
            said(&mut app),
            [
                no(" and gone.md, then plan.md"),
                no("Read "),
                ("plan.md".to_string(), file.clone()),
            ]
        );

        let items = app
            .world()
            .get::<Transcript>(chat)
            .unwrap()
            .items()
            .to_vec();
        app.world_mut()
            .entity_mut(chat)
            .insert(Transcript::from_items(items));
        app.update();
        assert_eq!(
            said(&mut app),
            [
                no(" and gone.md, then "),
                no("Read "),
                ("plan.md".to_string(), file.clone()),
                ("plan.md".to_string(), file),
            ]
        );
    }

    #[test]
    fn received_folders_and_absolute_local_files_link_but_house_paths_stay_text()
     {
        use hamlet_client::{ClientPlugin, receive};
        use hamlet_protocol::client as protocol;
        let scratch =
            hamlet_testing::Scratch::new("client-markdown-links");
        let first = scratch.join("first");
        let second = scratch.join("second");
        let house = scratch.join("house");
        for folder in [&first, &second, &house] {
            std::fs::create_dir_all(folder).unwrap();
        }
        for folder in [&first, &second] {
            std::fs::write(folder.join("plan.md"), "").unwrap();
        }
        std::fs::write(house.join("MEMORY.md"), "").unwrap();
        std::fs::create_dir_all(house.join("skills")).unwrap();
        let absolute = scratch.join("local.md");
        std::fs::write(&absolute, "").unwrap();
        let mut app = app();
        app.add_plugins(ClientPlugin { server: None });
        let mut state = hamlet_testing::realm::state();
        state.hamlets[0].folders =
            vec![first.to_string_lossy().into_owned()];
        receive(
            app.world_mut(),
            protocol::ServerMessage::State(state.clone()),
        );
        app.add_plugins((
            hamlet_core::panels::sets,
            hamlet_core::roll::plugin,
            crate::links::plugin,
        ));
        app.update();
        crate::testing::seat_ada(&mut app);
        mount::<HamletTheme>(
            app.world_mut(),
            fixed(
                &format!(
                    "Read {} and plan.md, MEMORY.md and skills/.",
                    absolute.display()
                ),
                Tone::Body,
            ),
        );
        app.update();
        let anchors = |app: &mut App| {
            app.world_mut()
                .query::<&Anchor>()
                .iter(app.world())
                .cloned()
                .collect::<Vec<_>>()
        };
        let found = anchors(&mut app);
        assert!(found.contains(&Anchor::File(absolute)));
        assert!(found.contains(&Anchor::File(first.join("plan.md"))));
        assert_eq!(found.len(), 2);
        state.hamlets[0].folders =
            vec![second.to_string_lossy().into_owned()];
        receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        app.update();
        let found = anchors(&mut app);
        assert!(
            found.contains(&Anchor::File(second.join("plan.md")))
        );
        assert!(
            !found.contains(&Anchor::File(first.join("plan.md")))
        );
    }
}
