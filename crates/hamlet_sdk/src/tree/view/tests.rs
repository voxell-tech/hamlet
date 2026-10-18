use std::time::Duration;

use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::time::{TimePlugin, TimeUpdateStrategy};
use bevy::ui::widget::Text;
use bevy_fynix::{FynixPlugin, mount};
use hamlet_assets::Tuning;

use super::panel;
use crate::icons::Glyph;
use crate::theme::{HamletTheme, insert_theme};
use crate::tree::node::{Kind, Node};
use crate::tree::store::{Drawn, PluginTrees};

const PANEL: &str = "probe/main";

fn tree(count: &str, hint: bool) -> Node {
    let mut children = vec![
        Node::new(Kind::Row).of(vec![
            Node::new(Kind::Icon(Glyph::Robot)),
            Node::new(Kind::Heading("ROSTER".into())),
        ]),
        Node::new(Kind::Button("Refresh".into())).id("go"),
        Node::new(Kind::TextField("Ada".into())).id("name"),
        Node::new(Kind::Label(count.into())).id("count"),
        Node::new(Kind::Divider),
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
fn a_redraw_keeps_a_typed_text_and_the_views_of_matched_nodes() {
    let mut app = App::new();
    app.add_plugins((
        crate::tree::plugin,
        TimePlugin,
        FynixPlugin::<HamletTheme>::default(),
    ));
    insert_theme(app.world_mut(), HamletTheme::designed());
    app.insert_resource(TimeUpdateStrategy::ManualDuration(
        Duration::from_millis(50),
    ));
    app.world_mut()
        .resource_mut::<PluginTrees>()
        .set(PANEL, Drawn::Tree(tree("0 refreshes", false)));
    mount::<HamletTheme>(app.world_mut(), panel(PANEL));
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
