use bevy::app::App;
use bevy::ecs::entity::Entity;
use hamlet_core::agents;
use hamlet_core::catalogue::ModelCatalogue;
use hamlet_core::core::{
    CorePlugin, Link, Listing, Settings, Skills,
};
use hamlet_engine_api::agents::{
    AgentsReply, ListedModel, Recipient, Selected, Settler,
    SettlerSkill, SkillFile, Template,
};
use hamlet_engine_api::{Failure, Reply, encode};
use hamlet_model::catalogue::{Catalogue, Priced};
use hamlet_model::config::{Config, Endpoint};
use hamlet_rules::hamlet::HOME;
use hamlet_rules::library::Library;
use hamlet_testing::Scratch;
use tokio::runtime::Runtime;

use crate::market::MarketWorldExt;
use crate::setup::SetupState;
use crate::{Chief, views};

const KEY: &str = "sk-never-out";

fn keyed() -> Endpoint {
    Endpoint {
        name: "openai".into(),
        base_url: "https://api.openai.com/v1".into(),
        chatgpt: false,
        api_key: Some(KEY.into()),
    }
}

#[test]
fn no_reply_carries_a_key() {
    let settings = Settings {
        config: Config {
            endpoints: vec![keyed()],
            ..Config::default()
        },
        error: None,
    };
    let home = std::env::temp_dir().join("hamlet-no-such-home");
    let told = views::settings_of(
        &settings,
        &Link::Unknown,
        &Listing::default(),
        &home,
    );
    assert!(told.endpoints[0].has_key);
    let setup = SetupState {
        asked: Some(keyed()),
        ..SetupState::default()
    };
    for reply in [
        AgentsReply::Settings(told),
        AgentsReply::Setup(setup.view()),
    ] {
        let bytes = encode(&Reply::Agents(reply)).unwrap();
        assert!(
            !bytes.windows(KEY.len()).any(|at| at == KEY.as_bytes())
        );
    }
}

#[test]
fn a_plugin_writes_only_one_plain_limit() {
    let refused = [
        ("permissions", "unconfined", "[\"mayor\"]"),
        ("", "permissions", "{ unconfined = [\"mayor\"] }"),
        ("", "votes", "{ least_voters = 1 }"),
        ("mayor", "permissions.unconfined", "[\"mayor\"]"),
        ("mayor", "build_roads", "true\n[permissions]"),
        ("mayor]\n[permissions", "unconfined", "[]"),
    ];
    for (section, key, value) in refused {
        assert!(
            views::unwritable(section, key, Some(value)).is_some(),
            "{section}.{key} = {value} was written"
        );
    }
    assert_eq!(
        views::unwritable("mayor", "build_roads", Some("true")),
        None
    );
    assert_eq!(views::unwritable("", "most_agents", None), None);
}

/// The agent core in `home`, whose model never answers, with the
/// chief `alice`.
fn core(home: &Scratch, runtime: &Runtime) -> App {
    let mut app = App::new();
    app.add_plugins(CorePlugin {
        home: hamlet_home::Home::open(home.to_path_buf(), None)
            .unwrap(),
        sandbox: &hamlet_sandbox::Unsandboxed,
        tokio: runtime.handle().clone(),
        config: Config {
            endpoints: vec![keyed()],
            ..Config::default()
        },
        replay: Some(Vec::new()),
    })
    .insert_resource(Chief {
        name: "alice".into(),
        session: Entity::PLACEHOLDER,
        with: Recipient::Mayor(HOME.into()),
        selected: Selected::Nothing,
    });
    app.update();
    app
}

fn runtime() -> Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

#[test]
fn a_model_added_is_allowed_and_kept() {
    let home = Scratch::new("agents-add-model");
    let runtime = runtime();
    let mut app = core(&home, &runtime);
    let world = app.world_mut();
    let large = ListedModel {
        id: "vendor/large".into(),
        context: Some(200_000),
        input: Some(3.0),
        output: Some(15.0),
        note: "For hard\nquestions.".into(),
    };
    assert_eq!(
        world.add_model("nowhere", large.clone()),
        Err(Failure::NotFound)
    );
    let priceless = ListedModel {
        input: Some(f64::NAN),
        ..large.clone()
    };
    assert!(world.add_model("openai", priceless).is_err());
    let name = world.add_model("openai", large).unwrap();
    assert_eq!(name, "openai::vendor/large");
    // The catalogue is read again at the next update.
    app.update();
    for all in [
        &app.world().resource::<ModelCatalogue>().catalogue,
        &Catalogue::load(&home),
    ] {
        let model = all.get(&name).unwrap();
        assert!(model.allowed);
        assert_eq!(model.priced, Priced::Listed);
        assert_eq!(model.context, Some(200_000));
        assert_eq!(model.note, "For hard questions.");
    }
}

#[test]
fn a_skill_taken_stays_in_its_folder_and_is_listed() {
    let home = Scratch::new("agents-take-skill");
    let runtime = runtime();
    let mut app = core(&home, &runtime);
    let world = app.world_mut();
    let file = |path: &str, text: &str| SkillFile {
        path: path.into(),
        text: text.into(),
    };
    let skill =
        "---\nname: tides\ndescription: Tells the tide.\n---\n";
    let refused = [
        ("../tides", vec![file("SKILL.md", skill)]),
        ("Tides", vec![file("SKILL.md", skill)]),
        (
            "tides",
            vec![file("SKILL.md", skill), file("../../out.md", "x")],
        ),
        (
            "tides",
            vec![file("SKILL.md", skill), file("/out.md", "x")],
        ),
    ];
    for (name, files) in refused {
        assert!(world.take_skill(name, files, "test").is_err());
    }
    let library = Library::of(&home);
    assert!(!library.root().join("tides").exists());
    assert!(!home.join("out.md").exists());

    let tides = vec![
        file("SKILL.md", skill),
        file("scripts/go.sh", "echo high\n"),
    ];
    world.take_skill("tides", tides.clone(), "test").unwrap();
    world.flush();
    assert!(library.root().join("tides/scripts/go.sh").is_file());
    let listed = &world.resource::<Skills>().0;
    assert!(listed.iter().any(|skill| skill.name == "tides"));
    // A name the library has is never written over.
    let other = vec![file(
        "SKILL.md",
        "---\nname: tides\ndescription: Other.\n---\n",
    )];
    assert!(world.take_skill("tides", other, "test").is_err());
    assert!(
        std::fs::read_to_string(
            library.root().join("tides/SKILL.md")
        )
        .unwrap()
        .contains("Tells the tide.")
    );
}

#[test]
fn a_hamlet_from_a_template_comes_up_with_its_agents() {
    let home = Scratch::new("agents-found-from");
    let runtime = runtime();
    let mut app = core(&home, &runtime);
    let world = app.world_mut();
    let settler = |name: &str| Settler {
        name: name.into(),
        purpose: format!("{name} works."),
        character: format!("{name} is calm."),
        notes: String::new(),
        skills: vec![SettlerSkill {
            name: "dig".into(),
            description: "Digs.".into(),
            body: "Step by step.".into(),
        }],
    };
    let template = Template {
        name: "mill".into(),
        title: "The Mill".into(),
        purpose: "Grind the grain.".into(),
        mayor: settler("miller"),
        agents: vec![settler("carter")],
    };
    world.found_from(template.clone()).unwrap();
    let miller = agents::mayor_of(world, "mill").unwrap();
    assert_eq!(agents::named(world, "miller"), Some(miller));
    let carter = agents::named(world, "carter").unwrap();
    assert_eq!(agents::hamlet_of(world, carter), Some("mill"));
    // Founded again, it is refused.
    assert!(world.found_from(template).is_err());
}
