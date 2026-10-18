use hamlet_openui::evaluate::CONTENT;
use hamlet_sdk::icons::Glyph;
use hamlet_sdk::tree::node::{Kind, Node};
use hamlet_sdk::tree::store::{Drawn, PluginTrees};

use crate::fixture::home::{self, Plugin, Scratch};
use crate::fixture::{self, Item, Widget};

pub(super) const PANEL: &str = "probe/main";

pub(super) fn items(count: &'static str) -> Vec<Item> {
    vec![
        Item(None, Widget::Column, 7),
        Item(None, Widget::Row, 2),
        Item(None, Widget::Icon("robot"), 0),
        Item(None, Widget::Heading("ROSTER"), 0),
        Item(Some("go"), Widget::Button("Refresh"), 0),
        Item(Some("name"), Widget::TextField("Ada"), 0),
        Item(Some("count"), Widget::Label(count), 0),
        Item(Some("hint"), Widget::Checkbox("Show hint", false), 0),
        Item(None, Widget::Divider, 0),
        Item(None, Widget::Scroll, 1),
        Item(None, Widget::Note("Nobody yet."), 0),
    ]
}

pub(super) fn tree(count: &str, hint: bool) -> Node {
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

#[test]
fn a_returned_program_is_drawn_through_the_language() {
    let scratch = Scratch::new("lang");
    let mut app = home::app(
        &scratch,
        &[Plugin::new(
            "probe",
            fixture::component(&fixture::speaking(
                "root = Column([Label(\"Hi\")])",
            )),
        )],
    );

    app.update();
    app.update();

    let content = Node::new(Kind::Column)
        .of(vec![Node::new(Kind::Label("Hi".into()))])
        .id(CONTENT);
    assert!(
        app.world().resource::<PluginTrees>().shows(
            PANEL,
            &Drawn::Tree(Node::new(Kind::Column).of(vec![content]))
        ),
        "{:?}",
        app.world().resource::<PluginTrees>()
    );
}

#[test]
fn a_returned_tree_is_kept_for_its_panel() {
    let scratch = Scratch::new("tree");
    let mut app = home::app(
        &scratch,
        &[Plugin::new(
            "probe",
            fixture::component(&fixture::drawing(
                &items("0 refreshes"),
                &items("0 refreshes"),
            )),
        )],
    );
    app.update();
    assert!(
        app.world()
            .resource::<PluginTrees>()
            .shows(PANEL, &Drawn::Tree(tree("0 refreshes", false))),
        "{:?}",
        app.world().resource::<PluginTrees>()
    );
}
