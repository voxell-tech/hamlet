//! Markdown in the chat, on the kit's: the blocks of a transcript's
//! entry ([`entry`]) and pictures drawn ([`picture`]). The `links`
//! screen sets the kit's `PathLinks` and the `pictures` screen its
//! `PictureView`.

mod picture;

use std::sync::{Arc, Mutex};

use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use bevy_fynix::{Signal, component};
use hamlet_client::chats::{Entry, Transcript};
pub use hamlet_ui::markdown::*;

pub(crate) use self::picture::picture_view;

fn words<'a>(
    world: &'a World,
    session: Entity,
    id: &str,
) -> Option<&'a str> {
    match &world.get::<Transcript>(session)?.item(id)?.entry {
        Entry::Assistant(text) | Entry::Agent { text, .. } => {
            Some(text)
        }
        _ => None,
    }
}

/// True while the entry `id` is the last of its conversation and
/// may have more words to come.
fn is_growing(world: &World, session: Entity, id: &str) -> bool {
    world.get::<Transcript>(session).is_some_and(|transcript| {
        transcript.is_answering()
            && transcript.items().last().map(|item| item.id.as_str())
                == Some(id)
    })
}

/// The blocks of an entry of a transcript, parsed again only when
/// that entry's own text has changed or has stopped growing.
pub(crate) fn entry(
    session: Entity,
    id: String,
) -> Signal<Vec<Arc<Block>>> {
    let mut moved = component::<Transcript, _>(session, |_| ());
    let read = Arc::new(Mutex::new(Read::default()));
    let shared = read.clone();
    let read_id = id.clone();
    Signal::new(
        move |world: &World| {
            let mut read = shared.lock().unwrap();
            read.update(world, session, &read_id);
            read.blocks.clone()
        },
        move |world: &World| {
            moved.changed(world)
                && read.lock().unwrap().update(world, session, &id)
        },
    )
}

/// What an entry's blocks were made from, and the blocks.
#[derive(Default)]
struct Read {
    /// The length, the hash and the growth of the words read.
    seen: Option<Option<(usize, u64, bool)>>,
    reading: Reading,
    blocks: Vec<Arc<Block>>,
}

impl Read {
    /// True if the blocks are not those of the words as they are now.
    fn update(
        &mut self,
        world: &World,
        session: Entity,
        id: &str,
    ) -> bool {
        let now = words(world, session, id);
        let grows = is_growing(world, session, id);
        let stamp = now.map(|text| (text.len(), hashed(text), grows));
        if self.seen == Some(stamp) {
            return false;
        }
        self.seen = Some(stamp);
        self.blocks = now
            .map(|text| self.reading.read(text))
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
    use hamlet_assets::Tuning;
    use hamlet_client::chats::Item;

    use super::*;
    use crate::links::{Roots, seek};
    use crate::testing::{plain, top};
    use crate::theme::{self, HamletTheme};

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((
            TimePlugin,
            FynixPlugin::<HamletTheme>::default(),
        ))
        .insert_resource(PathLinks {
            resolve: seek,
            generation: 0,
        });
        theme::insert_theme(app.world_mut(), HamletTheme::designed());
        app.update();
        app
    }

    /// A session where each of `replies` answers a message.
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
                        entry: Entry::Chief {
                            name: "a".into(),
                            text: "go".into(),
                        },
                    },
                    Item {
                        id: format!("{at}:reply"),
                        at: 0,
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

    fn grow(app: &mut App, session: Entity, more: &str) {
        let transcript =
            app.world().get::<Transcript>(session).unwrap();
        let mut items = transcript.items().to_vec();
        if let Some(Item {
            entry: Entry::Assistant(text),
            ..
        }) = items.last_mut()
        {
            text.push_str(more);
        }
        app.world_mut().entity_mut(session).insert(
            Transcript::from_items(items).with_answering(true),
        );
    }

    /// Every text span in the world, with what it says.
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
        let (session, ids) = talk(
            &mut app,
            &["Read plan.md and gone.md, then plan.md"],
        );
        mount::<HamletTheme>(
            app.world_mut(),
            document(entry(session, ids[1].clone()), Tone::Body),
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
            .get::<Transcript>(session)
            .unwrap()
            .items()
            .to_vec();
        app.world_mut()
            .entity_mut(session)
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
    fn only_the_entry_that_changed_is_read_again() {
        let mut app = app();
        let (session, ids) = talk(&mut app, &["old", "new"]);
        let mut old = entry(session, ids[1].clone());
        let mut new = entry(session, ids[3].clone());
        assert!(old.changed(app.world()));
        assert!(new.changed(app.world()));

        grow(&mut app, session, " words");
        assert!(!old.changed(app.world()));
        assert!(new.changed(app.world()));
        let blocks = new.get(app.world());
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            *blocks[0],
            top(Kind::Paragraph(vec![plain("new words")]))
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
        std::fs::write(house.join("NOTES.md"), "").unwrap();
        std::fs::create_dir_all(house.join("skills")).unwrap();
        let absolute = scratch.join("local.md");
        std::fs::write(&absolute, "").unwrap();
        let mut app = app();
        app.add_plugins(ClientPlugin { server: None });
        let mut state = hamlet_testing::realm::state();
        state.hamlets[0].folder =
            Some(first.to_string_lossy().into_owned());
        receive(
            app.world_mut(),
            protocol::ServerMessage::State(state.clone()),
        );
        app.add_plugins((
            crate::panel_sets,
            crate::convo::plugin,
            crate::roll::plugin,
            crate::links::plugin,
        ));
        app.update();
        mount::<HamletTheme>(
            app.world_mut(),
            fixed(
                &format!(
                    "Read {} and plan.md, NOTES.md and skills/.",
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
        state.hamlets[0].folder =
            Some(second.to_string_lossy().into_owned());
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
