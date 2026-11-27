use bevy::app::App;
use bevy::ecs::query::With;
use hamlet_client::outbox::Outbox;

use super::prompt::show_prompt;
use super::*;
use crate::testing::{panel_app, press, words};

fn hub_app() -> App {
    let mut app = panel_app();
    app.add_plugins((chat::plugin, crate::hub::plugin));
    app.init_resource::<crate::status::Bar>();
    app
}

#[test]
fn the_header_shows_received_work_and_stops_only_a_busy_agent() {
    let mut app = hub_app();
    let mut state = hamlet_testing::realm::state();
    state.agents[0].busy = true;
    state.agents[0].activity.doing_words =
        "Reading the received orchard ledger".into();
    hamlet_client::receive(
        app.world_mut(),
        protocol::ServerMessage::State(state),
    );
    app.update();
    bevy_fynix::mount::<HamletTheme>(app.world_mut(), living());
    app.update();
    assert!(words(&mut app).iter().any(
        |text| text == "Reading the received orchard ledger"
    ));
    let stop = |app: &mut App| {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<Stopping>>()
            .iter(world)
            .next()
    };
    let button = stop(&mut app).expect("Stop while she works");
    app.world_mut()
        .trigger(bevy::ui_widgets::Activate { entity: button });
    app.world_mut().flush();
    let stopped =
        app.world().resource::<Outbox>().iter().any(|message| {
            let protocol::ClientMessage::Stop(stop) = message else {
                return false;
            };
            stop.agent == Some(protocol::AgentId(1))
        });
    assert!(stopped);
    hamlet_client::receive(
        app.world_mut(),
        protocol::ServerMessage::State(hamlet_testing::realm::state()),
    );
    app.update();
    assert!(
        stop(&mut app).is_none(),
        "Send in its place while the received agent is at rest"
    );
}

/// The Hub on `hire#2`, with her details open.
fn ada_app() -> App {
    let mut app = hub_app();
    let ada = crate::testing::seat_ada(&mut app);
    app.insert_resource(hamlet_realm::pick::Selected(Some(
        Thing::Agent(ada),
    )))
    .insert_resource(Showing::Thread)
    .insert_resource(Sheet::Details);
    app.update();
    bevy_fynix::mount::<HamletTheme>(
        app.world_mut(),
        crate::hub::screen(),
    );
    app.update();
    app
}

fn request(app: &App, what: &str) -> serde_json::Value {
    app.world()
        .resource::<Outbox>()
        .iter()
        .map(|message| serde_json::to_value(message).unwrap())
        .filter(|message| message["what"] == what)
        .last()
        .unwrap_or_else(|| panic!("no read {what}"))
}

fn has_span(app: &mut App, text: &str) -> bool {
    app.world_mut()
        .query::<&bevy::text::TextSpan>()
        .iter(app.world())
        .any(|span| span.0 == text)
}

fn receive(app: &mut App, message: serde_json::Value) {
    hamlet_client::receive(
        app.world_mut(),
        serde_json::from_value(message).unwrap(),
    );
    app.update();
}

#[test]
fn papers_of_the_agent_and_only_its_finished_work_refresh_the_sheet()
{
    use serde_json::json;
    let mut app = ada_app();
    let read = request(&app, "papers");
    assert_eq!(read["of"], json!({"agent": 2}));
    receive(
        &mut app,
        json!({"type": "read", "to": read["id"], "papers": {
            "memory": [], "learned": [], "character": "", "skills": []
        }}),
    );
    app.world_mut()
        .resource_mut::<Outbox>()
        .drain()
        .for_each(drop);
    app.world_mut()
        .trigger(hamlet_client::messages::JobFinished {
            chat: Entity::PLACEHOLDER,
        });
    assert!(app.world().resource::<Outbox>().iter().next().is_none());
    let chat = app.world().resource::<Convo>().session;
    app.world_mut()
        .trigger(hamlet_client::messages::JobFinished { chat });
    app.update();
    let read = request(&app, "papers");
    receive(
        &mut app,
        json!({"type": "read", "to": read["id"], "papers": {
            "memory": [{"path": "notes.md", "description": "", "text": "Received notes"}], "learned": ["notes.md"], "character": "Received character", "skills": [{"name": "orchard", "description": "Tends trees"}]
        }}),
    );
    assert!(has_span(&mut app, "Received character"));
    assert!(
        words(&mut app)
            .iter()
            .any(|text| text.contains("notes.md (learned)"))
    );
    press(&mut app, "notes.md (learned)");
    app.update();
    assert!(has_span(&mut app, "Received notes"));
    let shown = words(&mut app);
    assert!(shown.iter().any(|text| text == "orchard"));
    assert!(shown.iter().any(|text| text == "Tends trees"));
    press(&mut app, "Note what it learned");
    let sent = app
        .world()
        .resource::<Outbox>()
        .iter()
        .map(|m| serde_json::to_value(m).unwrap())
        .find(|m| m["type"] == "say")
        .unwrap();
    assert_eq!(sent["to"], json!(2));
    assert_eq!(sent["text"], "/reflect");
}

#[test]
fn a_prompt_reply_or_failure_is_shown_on_its_page() {
    use serde_json::json;
    let mut app = ada_app();
    show_prompt(app.world_mut());
    assert_eq!(*app.world().resource::<Showing>(), Showing::Prompt);
    let read = request(&app, "prompt");
    assert_eq!(read["agent"], json!(2));
    receive(
        &mut app,
        json!({"type": "read", "to": read["id"], "prompt": [{"name": "charter", "text": "The received charter"}, {"name": "memory", "text": "The received memory"}]}),
    );
    app.update();
    assert_eq!(
        app.world().resource::<Briefing>().whole(),
        "The received charter\n\nThe received memory"
    );
    assert!(words(&mut app).contains(&"Memory".into()));
    show_prompt(app.world_mut());
    let read = request(&app, "prompt");
    receive(
        &mut app,
        json!({"type": "reply", "to": read["id"], "kind": "error", "text": "No such agent"}),
    );
    assert!(words(&mut app).contains(
        &"The prompt could not be read: No such agent".into()
    ));
}

#[test]
fn edits_of_the_character_and_of_a_memory_file_return_to_where_they_began()
 {
    use bevy::ecs::observer::On;
    use bevy::ecs::resource::Resource;
    use bevy::ecs::system::ResMut;
    use hamlet_desktop::editor::{Edit, Edited};
    use serde_json::json;
    #[derive(Resource, Default)]
    struct Edits(Vec<Edit>);
    let mut app = ada_app();
    app.init_resource::<Edits>().add_observer(
        |edit: On<Edit>, mut edits: ResMut<Edits>| {
            edits.0.push(edit.event().clone());
        },
    );
    let papers = request(&app, "papers");
    receive(
        &mut app,
        json!({"type": "read", "to": papers["id"], "papers": {"memory": [{"path": "farm/pears.md", "description": "", "text": "Pears"}], "learned": [], "character": "", "skills": []}}),
    );
    press(&mut app, "Edit");
    let read = request(&app, "papers");
    assert_eq!(read["full"], true);
    let text = "character ".repeat(1000);
    receive(
        &mut app,
        json!({"type": "read", "to": read["id"], "papers": {"memory": [], "learned": [], "character": text, "skills": []}}),
    );
    let edit =
        app.world().resource::<Edits>().0.last().unwrap().clone();
    assert_eq!(edit.text, text);
    app.world_mut().trigger(Edited {
        key: edit.key.clone(),
        text: "Saved character".into(),
    });
    app.update();
    let sent = app
        .world()
        .resource::<Outbox>()
        .iter()
        .map(|m| serde_json::to_value(m).unwrap())
        .find(|m| m["type"] == "edit")
        .unwrap();
    assert_eq!(sent["doc"], "character");
    assert_eq!(sent["profile"], json!(2));
    assert_eq!(sent["text"], "Saved character");
    receive(
        &mut app,
        json!({"type": "reply", "to": sent["id"], "kind": "error", "text": "Character refused"}),
    );
    assert!(
        words(&mut app)
            .iter()
            .any(|text| text == "Character refused")
    );
    // A memory file of the agent's lodge is edited the same way,
    // and removed by sending no text.
    super::super::papers::edit_memory(
        app.world_mut(),
        "farm/pears.md".into(),
    );
    let read = request(&app, "papers");
    receive(
        &mut app,
        json!({"type": "read", "to": read["id"], "papers": {"memory": [{"path": "farm/pears.md", "description": "", "text": "Whole pears"}], "learned": [], "character": "", "skills": []}}),
    );
    let edit =
        app.world().resource::<Edits>().0.last().unwrap().clone();
    assert_eq!(edit.text, "Whole pears");
    app.world_mut().trigger(Edited {
        key: edit.key,
        text: "Ripe pears".into(),
    });
    app.update();
    let sent = app
        .world()
        .resource::<Outbox>()
        .iter()
        .map(|m| serde_json::to_value(m).unwrap())
        .filter(|m| m["doc"] == "memory")
        .last()
        .unwrap();
    assert_eq!(sent["of"], json!({"agent": 2}));
    assert_eq!(sent["path"], "farm/pears.md");
    assert_eq!(sent["text"], "Ripe pears");
    super::super::papers::remove_memory(
        app.world_mut(),
        "farm/pears.md".into(),
    );
    let removal = app
        .world()
        .resource::<Outbox>()
        .iter()
        .map(|m| serde_json::to_value(m).unwrap())
        .filter(|m| m["doc"] == "memory")
        .last()
        .unwrap();
    assert_eq!(removal["text"], "");
}

fn select_scout(app: &mut App) {
    use hamlet_client::chats::Chat;
    let mut state = hamlet_testing::realm::state();
    state
        .agents
        .push(hamlet_testing::realm::agent(3, "home", false));
    hamlet_client::receive(
        app.world_mut(),
        protocol::ServerMessage::State(state),
    );
    let agent =
        hamlet_client::realm::named(app.world_mut(), "hire#3")
            .unwrap();
    let to = protocol::AgentId(3);
    let session = app
        .world_mut()
        .spawn(Chat(protocol::ChatRef {
            kind: None,
            from: None,
            with: to,
            past: None,
        }))
        .id();
    let mut convo = app.world_mut().resource_mut::<Convo>();
    convo.agent = agent;
    convo.session = session;
    convo.with = Some("hire#3".into());
    app.update();
}

#[test]
fn late_reads_cannot_fill_the_next_agents_pages() {
    use serde_json::json;
    let mut app = ada_app();
    let papers = request(&app, "papers");
    show_prompt(app.world_mut());
    let prompt = request(&app, "prompt");
    select_scout(&mut app);
    let scout = request(&app, "papers");
    assert_eq!(scout["of"], json!({"agent": 3}));
    receive(
        &mut app,
        json!({"type": "read", "to": scout["id"], "papers": {"memory": [{"path": "notes.md", "description": "", "text": "Scout notes"}], "learned": [], "character": "Scout character", "skills": []}}),
    );
    receive(
        &mut app,
        json!({"type": "read", "to": papers["id"], "papers": {"memory": [{"path": "notes.md", "description": "", "text": "Wrong notes"}], "learned": [], "character": "Wrong character", "skills": []}}),
    );
    receive(
        &mut app,
        json!({"type": "read", "to": prompt["id"], "prompt": [{"name": "charter", "text": "Wrong prompt"}]}),
    );
    // The prompt page is left for the thread of the next agent.
    assert_eq!(*app.world().resource::<Showing>(), Showing::Thread);
    assert!(has_span(&mut app, "Scout character"));
    assert!(!has_span(&mut app, "Wrong character"));
    press(&mut app, "notes.md");
    app.update();
    assert!(has_span(&mut app, "Scout notes"));
    assert!(!has_span(&mut app, "Wrong notes"));
    assert!(app.world().resource::<Briefing>().prompt.is_empty());
}

#[test]
fn letters_and_questions_are_in_the_session_menu_and_watched_read_only()
 {
    use hamlet_client::chats::Chat;
    use serde_json::json;
    let mut app = ada_app();
    // The chat is under the details, so they are shut.
    app.insert_resource(Sheet::Chat);
    let mut state = hamlet_testing::realm::state();
    state
        .agents
        .push(hamlet_testing::realm::agent(3, "home", false));
    state.agents[1].conversations = serde_json::from_value(json!([
        {"with":3,"kind":"letters","count":2,"last":1000},
        {"with":3,"kind":"questions","count":1,"last":2000}
    ]))
    .unwrap();
    receive(
        &mut app,
        serde_json::to_value(protocol::ServerMessage::State(state))
            .unwrap(),
    );
    let ada = app.world().resource::<Convo>().agent;
    let (items, now) = entries(app.world());
    let names = items
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        ["Session", "Letters with hire#3", "Questions with hire#3"]
    );
    assert_eq!(now, 0);
    for (kind, (_, go)) in
        ["letters", "questions"].iter().zip(&items[1..])
    {
        go_to(&mut app, go);
        let session = app.world().resource::<Convo>().session;
        let chat =
            app.world().get::<Chat>(session).unwrap().0.clone();
        assert_eq!(
            serde_json::to_value(&chat).unwrap(),
            json!({"with":2, "from":3, "kind":kind})
        );
        assert!(app.world().resource::<Outbox>().iter().any(|m| matches!(m, protocol::ClientMessage::Open(open) if open.chat == chat)));
        receive(
            &mut app,
            json!({"type":"transcript","chat":chat,"items":[
                {"id":"1:0","at":1,"kind":"user","text":"Question","author":"hire#3"},
                {"id":"2:0","at":2,"kind":"assistant","text":"Answer"}
            ], "earlier":"1:0","last":null,"working":true,"pending":0,"usage":{
                "input":0,"output":0,"cached":0,"calls":0,"lastInput":0,"reasoning":0,"unreported":0,"summary":"","micros":0,"ms":0
            }}),
        );
        let world = app.world_mut();
        assert_eq!(
            world
                .query_filtered::<Entity, With<chat::ChatField>>()
                .iter(world)
                .count(),
            0
        );
        assert_eq!(
            world.query::<&chat::Rewinds>().iter(world).count(),
            0
        );
        assert!(words(&mut app).contains(&"Read only".into()));
        // The menu shows which of them is open.
        assert_ne!(entries(app.world()).1, 0);
    }
    // The first of the menu leads back to the chief's chat.
    go_to(&mut app, &items[0].1);
    let session = app.world().resource::<Convo>().session;
    assert_eq!(app.world().resource::<Convo>().agent, ada);
    assert!(
        app.world().get::<Chat>(session).unwrap().0.from.is_none()
    );
}

fn go_to(app: &mut App, to: &Go) {
    go(app.world_mut(), to);
    app.update();
}
