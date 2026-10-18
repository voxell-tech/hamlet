use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::widget::Text;
use bevy_fynix::mount;
use hamlet_sdk::icons::Glyph;
use hamlet_sdk::theme::HamletTheme;

use crate::fixture::home::{self, Plugin, Scratch};
use crate::fixture::{self, Item, Widget};
use crate::tree::{Kind, Node};
use crate::trees::{Drawn, PluginTrees};
use crate::view;

const PANEL: &str = "probe/main";

fn items() -> Vec<Item> {
    vec![
        Item(None, Widget::Column, 7),
        Item(None, Widget::Row, 2),
        Item(None, Widget::Icon("robot"), 0),
        Item(None, Widget::Heading("ROSTER"), 0),
        Item(Some("go"), Widget::Button("Refresh"), 0),
        Item(Some("name"), Widget::TextField("Ada"), 0),
        Item(Some("count"), Widget::Label("0 refreshes"), 0),
        Item(Some("hint"), Widget::Checkbox("Show hint", false), 0),
        Item(None, Widget::Divider, 0),
        Item(None, Widget::Scroll, 1),
        Item(None, Widget::Note("Nobody yet."), 0),
    ]
}

fn tree(count: &str, hint: bool) -> Node {
    let mut children = vec![
        Node::new(Kind::Row).of(vec![
            Node::new(Kind::Icon(Glyph::Robot)),
            Node::new(Kind::Heading("ROSTER".into())),
        ]),
        Node::new(Kind::Button("Refresh".into())).id("go"),
        Node::new(Kind::TextField("Ada".into())).id("name"),
        Node::new(Kind::Label(count.into())).id("count"),
        Node::new(Kind::Checkbox {
            label: "Show hint".into(),
            checked: false,
        })
        .id("hint"),
        Node::new(Kind::Divider),
        Node::new(Kind::Scroll)
            .of(vec![Node::new(Kind::Note("Nobody yet.".into()))]),
    ];
    if hint {
        children.push(Node::new(Kind::Note("A hint.".into())));
    }
    Node::new(Kind::Column).of(children)
}

fn text_entity(app: &mut App, said: &str) -> Option<Entity> {
    let world = app.world_mut();
    world
        .query::<(Entity, &Text)>()
        .iter(world)
        .find(|(_, text)| text.0 == said)
        .map(|(entity, _)| entity)
}

fn field(app: &mut App) -> Entity {
    let world = app.world_mut();
    world
        .query_filtered::<Entity, With<EditableText>>()
        .single(world)
        .expect("one text field")
}

#[test]
fn a_returned_tree_is_drawn_and_a_redraw_keeps_a_typed_text() {
    let scratch = Scratch::new("tree");
    let mut app = home::app(
        &scratch,
        &[Plugin {
            id: "probe",
            wasm: fixture::component(&fixture::drawing(
                &items(),
                &items(),
            )),
        }],
    );
    home::views(&mut app);
    app.update();
    assert!(
        app.world()
            .resource::<PluginTrees>()
            .shows(PANEL, &Drawn::Tree(tree("0 refreshes", false))),
        "{:?}",
        app.world().resource::<PluginTrees>()
    );

    mount::<HamletTheme>(app.world_mut(), view::panel(PANEL));
    app.update();
    app.update();
    let field_before = field(&mut app);
    let button_before = text_entity(&mut app, "Refresh").unwrap();
    let count_before = text_entity(&mut app, "0 refreshes").unwrap();
    // The user types in the field.
    app.world_mut().insert_resource(InputFocus::default());
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(field_before, FocusCause::Pressed);
    app.world_mut()
        .get_mut::<EditableText>(field_before)
        .unwrap()
        .editor_mut()
        .set_text("Adam");

    // The plugin draws again: a new count and a new line.
    app.world_mut()
        .resource_mut::<PluginTrees>()
        .set(PANEL, Drawn::Tree(tree("1 refresh", true)));
    app.update();

    assert_eq!(field(&mut app), field_before);
    let typed = app
        .world()
        .get::<EditableText>(field_before)
        .unwrap()
        .value()
        .to_string();
    assert_eq!(typed, "Adam");
    assert_eq!(text_entity(&mut app, "Refresh"), Some(button_before));
    assert_eq!(
        text_entity(&mut app, "1 refresh"),
        Some(count_before)
    );
    assert!(text_entity(&mut app, "A hint.").is_some());
}
