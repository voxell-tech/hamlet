use bevy::prelude::*;
use hamlet_sdk::agents::{Agent, AgentList};
use hamlet_sdk::capability::{Capability, Granted};
use hamlet_sdk::dock::Panel;
use hamlet_sdk::icons::Glyph;
use hamlet_sdk::papers::Papers;
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
    run_with(text, agents, &Papers::default(), granted)
}

fn run_with(
    text: &str,
    agents: &AgentList,
    papers: &Papers,
    granted: &Granted,
) -> Evaluated {
    let limits = PanelLimits::default();
    let document = openui_lang::parse_with_limits(
        &PanelLibrary::default().0,
        text,
        limits.runtime(),
    );
    evaluate(
        &document,
        &Inputs::new(),
        agents,
        papers,
        granted,
        &limits,
        &[],
    )
}

fn drawn(content: Vec<Node>) -> Node {
    Node::new(Kind::Column)
        .of(vec![Node::new(Kind::Column).of(content).id(CONTENT)])
}

#[test]
fn a_program_becomes_the_tree_of_its_components() {
    let program = "root = Column([title, name, loud, body, shown, \
                   Divider()])\ntitle = Heading(\"HI\")\nname = \
                   TextField($name)\nloud = Checkbox(\"Loud\", \
                   $loud)\nbody = TextArea($body)\nshown = \
                   Markdown(\"# Hi\")\n$name = \"Ada\"\n$loud = \
                   true\n$body = \"One\\nTwo\"";

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
            Node::new(Kind::TextArea("One\nTwo".into())).id("n.3"),
            Node::new(Kind::Markdown("# Hi".into())),
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
    let allowed = run(
        program,
        &agents,
        &Granted::allowing(&[Capability::ReadAgents]),
    );

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

#[test]
fn the_papers_query_needs_its_own_grant() {
    let program = "root = Column([shown])\nshown = papers.error ? \
                   Note(papers.error) : Column(@Each(papers.rows, \
                   \"p\", Label(p.notes)))\npapers = \
                   Query(\"papers\", {}, {rows: [], error: null})";
    let papers = Papers {
        notes: "tide table".into(),
        ..Papers::default()
    };
    let none = AgentList::default();
    let ran = |granted: &Granted| {
        run_with(program, &none, &papers, granted)
    };

    let denied = ran(&Granted::allowing(&[Capability::ReadAgents]));
    let allowed = ran(&Granted::allowing(&[Capability::ReadPapers]));

    assert_eq!(denied.problems, ["read.papers not granted"]);
    assert!(allowed.problems.is_empty(), "{:?}", allowed.problems);
    assert_eq!(
        allowed.root,
        drawn(vec![
            Node::new(Kind::Column).of(vec![Node::new(Kind::Label(
                "tide table".into()
            ))])
        ])
    );
}
