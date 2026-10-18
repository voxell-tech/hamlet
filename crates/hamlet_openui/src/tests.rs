use bevy::prelude::*;
use hamlet_sdk::agents::{Agent, AgentList};
use hamlet_sdk::dock::Panel;
use hamlet_sdk::grant::{Granted, READ_AGENTS};
use hamlet_sdk::icons::Glyph;
use hamlet_sdk::tree::event::{Act, Interaction};
use hamlet_sdk::tree::node::{Kind, Node};
use hamlet_sdk::tree::store::PluginTrees;
use hamlet_sdk::tree::view;
use openui_lang::Inputs;

use crate::evaluate::{CONTENT, Evaluated, PROBLEM, evaluate};
use crate::{PanelLibrary, PanelLimits, Source, plugin};

const PANEL: &str = "probe/main";

fn run(
    text: &str,
    agents: &AgentList,
    granted: &Granted,
) -> Evaluated {
    let limits = PanelLimits::default();
    let document = openui_lang::parse_with_limits(
        &PanelLibrary::default().0,
        text,
        limits.runtime(),
    );
    evaluate(&document, &Inputs::new(), agents, granted, &limits, &[])
}

fn drawn(content: Vec<Node>) -> Node {
    Node::new(Kind::Column)
        .of(vec![Node::new(Kind::Column).of(content).id(CONTENT)])
}

#[test]
fn a_program_becomes_the_tree_of_its_components() {
    let program = "root = Column([title, name, loud, \
                   Divider()])\ntitle = Heading(\"HI\")\nname = \
                   TextField($name)\nloud = Checkbox(\"Loud\", \
                   $loud)\n$name = \"Ada\"\n$loud = true";

    let out =
        run(program, &AgentList::default(), &Granted::default());

    assert!(out.problems.is_empty(), "{:?}", out.problems);
    assert_eq!(
        out.root,
        drawn(vec![
            Node::new(Kind::Heading("HI".into())),
            Node::new(Kind::TextField("Ada".into())).id("n.1"),
            Node::new(Kind::Checkbox {
                label: "Loud".into(),
                checked: true
            })
            .id("n.2"),
            Node::new(Kind::Divider),
        ])
    );
}

#[test]
fn typing_in_a_bound_field_changes_the_state_and_the_draw() {
    let mut app = App::new();
    app.add_plugins(plugin);
    app.world_mut().spawn((
        Panel::by_id(PANEL, "PROBE", Glyph::Book, view::panel),
        Source(
            "root = Column([name, hello])\nname = \
             TextField($name)\nhello = Label(\"Hello, \" + \
             $name)\n$name = \"Ada\""
                .into(),
        ),
    ));
    let hello = |app: &App| {
        let trees = app.world().resource::<PluginTrees>();
        let content =
            trees.node(PANEL, &[]).map(|root| &root.children[0]);
        content.map(|content| content.children[1].text().to_string())
    };
    app.update();
    assert_eq!(hello(&app).as_deref(), Some("Hello, Ada"));

    Interaction::send(
        app.world_mut(),
        PANEL,
        "n.0",
        Act::Typed("Bo".into()),
    );
    app.update();

    assert_eq!(hello(&app).as_deref(), Some("Hello, Bo"));
}

#[test]
fn a_program_that_cannot_be_drawn_says_so_and_does_not_panic() {
    let none = AgentList::default();
    let granted = Granted::default();
    let deep = format!(
        "root = {}{}",
        "Column([".repeat(100),
        "])".repeat(100)
    );

    for program in ["root = Column([Bogus(\"x\")])", deep.as_str()] {
        let out = run(program, &none, &granted);

        assert!(!out.problems.is_empty());
        let Kind::Note(first) = &out.root.children[0].kind else {
            panic!("no problem line in {:?}", out.root);
        };
        assert_eq!(out.root.children[0].id.as_deref(), Some(PROBLEM));
        assert_eq!(first, &out.problems[0]);
    }
}

#[test]
fn the_agents_query_needs_its_grant() {
    let program = "root = Column([shown])\nshown = agents.error ? \
                   Note(agents.error) : Column(@Each(agents.rows, \
                   \"a\", Label(a.name)))\nagents = \
                   Query(\"agents\", {}, {rows: [], error: null})";
    let agents = AgentList(vec![Agent {
        name: "ada".into(),
        hamlet: "home".into(),
    }]);

    let denied = run(program, &agents, &Granted::default());
    let allowed =
        run(program, &agents, &Granted(vec![READ_AGENTS.into()]));

    assert_eq!(denied.problems, ["read.agents not granted"]);
    assert_eq!(
        denied.root,
        Node::new(Kind::Column).of(vec![
            Node::new(Kind::Note("read.agents not granted".into()))
                .id(PROBLEM),
            Node::new(Kind::Column)
                .of(vec![Node::new(Kind::Note(
                    "read.agents not granted".into()
                ))])
                .id(CONTENT),
        ])
    );
    assert!(allowed.problems.is_empty(), "{:?}", allowed.problems);
    assert_eq!(
        allowed.root,
        drawn(vec![
            Node::new(Kind::Column)
                .of(vec![Node::new(Kind::Label("ada".into()))])
        ])
    );
}
