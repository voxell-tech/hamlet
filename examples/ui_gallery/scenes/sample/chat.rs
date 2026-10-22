//! The gallery's received conversations.

use bevy::ecs::world::World;
use hamlet_client::chats::{Entry, Item, Transcript};
use hamlet_protocol::client as protocol;
use hamlet_ui::links::Roots;

use crate::scenes::Scene;

pub(super) fn conversation(lines: &[(&str, &str)]) -> Transcript {
    Transcript::from_items(
        lines
            .iter()
            .enumerate()
            .map(|(at, (by, text))| Item {
                id: format!("entry-{at}:0"),
                at: 1_791_374_400_000 + at as u64 * 60_000,
                entry: match *by {
                    "" => Entry::Assistant((*text).into()),
                    by => Entry::User {
                        user: by.into(),
                        text: (*text).into(),
                    },
                },
            })
            .collect(),
    )
}

pub(super) fn setup(world: &mut World, scene: Scene) -> Transcript {
    match scene {
        Scene::Quiet => Transcript::default(),
        Scene::Chat => {
            let mut items = Vec::new();
            for turn in 0..24 {
                let start = conversation(&[
                    ("chief", "Look over this row before the frost."),
                    (
                        "",
                        "The pears need straw at the roots.\n\nThe \
                         stake by the gate is split.",
                    ),
                ]);
                for item in start.items() {
                    let mut item = item.clone();
                    item.id = format!("turn-{turn}-{}", item.id);
                    items.push(item);
                }
                for step in 0..3 {
                    items.push(Item {
                        id: format!("turn-{turn}-tool-{step}"),
                        at: 0,
                        entry: Entry::Tool {
                            name: "bash".into(),
                            summary: format!(
                                "grep -n row-{turn} \
                                 notes/orchard/{step}.md"
                            ),
                            exit: Some(0),
                            ms: Some(12),
                            failure: None,
                            change: None,
                        },
                    });
                }
            }
            Transcript::from_items(items)
        }
        Scene::Edits => {
            let mut items = conversation(&[
                ("chief", "Fix the pear count."),
                (
                    "",
                    "I will count the rows before changing the \
                     report.",
                ),
            ])
            .items()
            .to_vec();
            items.push(Item {
                id: "edit:0".into(),
                at: 0,
                entry: Entry::Tool {
                    name: "edit".into(),
                    summary: "src/almanac.rs".into(),
                    exit: Some(0),
                    ms: Some(8),
                    failure: None,
                    change: Some(change()),
                },
            });
            Transcript::from_items(items)
        }
        Scene::Links | Scene::Pictures => {
            let folder = super::scratch().join("folder");
            std::fs::create_dir_all(&folder)
                .expect("a scratch folder");
            std::fs::write(folder.join("plan.md"), "# Orchard plan")
                .expect("a local link");
            world.insert_resource(Roots::new([folder.clone()]));
            if scene == Scene::Pictures {
                use bevy::render::render_resource::{
                    Extent3d, TextureDimension, TextureFormat,
                };
                let image = bevy::image::Image::new(
                    Extent3d {
                        width: 640,
                        height: 180,
                        depth_or_array_layers: 1,
                    },
                    TextureDimension::D2,
                    [125, 160, 95, 255].repeat(640 * 180),
                    TextureFormat::Rgba8UnormSrgb,
                    bevy::asset::RenderAssetUsages::MAIN_WORLD,
                );
                image
                    .try_into_dynamic()
                    .expect("a picture")
                    .save(folder.join("orchard.png"))
                    .expect("a local picture");
                conversation(&[("chief", "Show the orchard."), ("", "![The orchard](orchard.png)\n\n![Missing picture](missing.png)\n\n![A picture on the web](https://images.example/orchard.png)")])
            } else {
                conversation(&[("chief", "Read plan.md."), ("", "See plan.md and [the guide](https://example.com).\n\nA missing path stays plain: missing.md.")])
            }
        }
        Scene::Rewind => conversation(&[
            ("chief", "Tidy the orchard notes."),
            ("", "The notes are in order."),
            ("chief", "Find out who has the key."),
        ]),
        Scene::Group => {
            let mut items = conversation(&[
                ("chief", "Ask Pip about the key."),
                ("", "I asked Pip to check the mill door."),
            ])
            .items()
            .to_vec();
            items.push(Item {
                id: "pip:0".into(),
                at: 0,
                entry: Entry::Agent {
                    name: "Pip".into(),
                    text: "The key is by the mill door.".into(),
                },
            });
            items.push(Item {
                id: "reply:0".into(),
                at: 1_791_374_580_000,
                entry: Entry::User {
                    user: "chief".into(),
                    text: "(Replying to Pip: \"The key is by the \
                           mill door.\")\n\n> by the mill \
                           door\nWrite that into the almanac."
                        .into(),
                },
            });
            Transcript::from_items(items)
        }
        _ => conversation(&[
            (
                "chief",
                "Tidy the orchard notes and tell me what is missing.",
            ),
            (
                "",
                "The notes are in order. Two things are missing: \
                 the date the pears were grafted, and who keeps the \
                 key to the press.",
            ),
        ]),
    }
}

pub(super) fn change() -> protocol::Change {
    protocol::Change {
        path: "src/almanac.rs".into(),
        created: false,
        total: 3,
        added: 1,
        removed: 1,
        count: "+1 -1".into(),
        lines: vec![
            protocol::ChangeLines {
                mark: protocol::ChangeLinesMark::Space,
                text: "let rows = read(&path);".into(),
            },
            protocol::ChangeLines {
                mark: protocol::ChangeLinesMark::Minus,
                text: "let pears = 0;".into(),
            },
            protocol::ChangeLines {
                mark: protocol::ChangeLinesMark::Plus,
                text: "let pears = count(&rows);".into(),
            },
        ],
    }
}
