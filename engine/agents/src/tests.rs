use bevy::app::App;
use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use hamlet_core::agents;
use hamlet_core::catalogue::ModelCatalogue;
use hamlet_core::core::{
    CorePlugin, Link, Listing, Settings, Skills,
};
use hamlet_engine_api::agents::{
    AgentsEvent, AgentsReply, Delivery, Destination, HousedTool,
    ListedModel, PACKAGE_PART, PackageKind, Packing, Recipient,
    Selected, Settler, SettlerSkill, SkillFile, Template,
};
use hamlet_engine_api::{Failure, Reply, encode};
use hamlet_model::catalogue::{Catalogue, Priced};
use hamlet_model::config::{Config, Endpoint};
use hamlet_model::market::package::{
    Agent, CHARACTER, Contents, File, Hamlet, MANIFEST, MOST_BYTES,
    NOTES, Package, Tool, Tools,
};
use hamlet_rules::hamlet::HOME;
use hamlet_rules::library::Library;
use hamlet_rules::realm::Realm;
use hamlet_testing::Scratch;
use tokio::runtime::Runtime;

use crate::market::MarketWorldExt;
use crate::setup::SetupState;
use crate::watch::Watchers;
use crate::{Chief, market, roster, views};

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
    .add_plugins(market::plugin)
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

fn packed(contents: Contents) -> Vec<u8> {
    Package::new(contents).bytes().unwrap()
}

fn text(path: &str, text: &str) -> File {
    File {
        path: path.into(),
        data: text.as_bytes().to_vec(),
        executable: false,
    }
}

fn scribe(character: &str) -> Vec<u8> {
    packed(Contents::Agent {
        agent: Agent {
            name: "scribe".into(),
            purpose: "Writes letters.".into(),
            model: None,
            files: vec![
                text(CHARACTER, character),
                text(
                    "workspace/skills/ink/SKILL.md",
                    "---\nname: ink\n---\n",
                ),
            ],
        },
    })
}

/// A scribe whose skill holds a file too big for one part.
fn heavy_scribe() -> Vec<u8> {
    let mut atlas = text("workspace/skills/ink/atlas.bin", "");
    atlas.data = vec![7; PACKAGE_PART * 2];
    packed(Contents::Agent {
        agent: Agent {
            name: "scribe".into(),
            purpose: "Writes letters.".into(),
            model: None,
            files: vec![text(CHARACTER, "Careful."), atlas],
        },
    })
}

fn delivery(
    listing: &str,
    version: &str,
    package: Vec<u8>,
    to: Destination,
) -> Delivery {
    Delivery {
        listing: listing.into(),
        title: listing.into(),
        version: version.into(),
        package,
        part: 0,
        parts: 1,
        to,
    }
}

/// The plugin that sends and packs in the tests.
const BAZAAR: Entity = Entity::PLACEHOLDER;

/// Settles `whole` a part at a time, as the bazaar sends it.
fn settle(
    world: &mut World,
    whole: Delivery,
) -> Result<String, Failure> {
    let parts =
        whole.package.chunks(PACKAGE_PART).collect::<Vec<_>>();
    for (part, bytes) in parts.iter().enumerate() {
        let delivery = Delivery {
            package: bytes.to_vec(),
            part: part as u32,
            parts: parts.len() as u32,
            ..whole.clone()
        };
        match world.settle_part(BAZAAR, delivery)? {
            AgentsReply::Done if part + 1 < parts.len() => {}
            AgentsReply::Settled(words)
                if part + 1 == parts.len() =>
            {
                return Ok(words);
            }
            other => panic!("part {part} gave {other:?}"),
        }
    }
    Err(Failure::Refused("no parts".into()))
}

/// Packs `packing` and reads it back a part at a time.
fn pack(
    world: &mut World,
    packing: Packing,
) -> Result<Vec<u8>, Failure> {
    let AgentsReply::Packed { id, size } =
        world.hold_pack(BAZAAR, packing)?
    else {
        panic!("not packed");
    };
    let mut bytes = Vec::new();
    while (bytes.len() as u64) < size {
        let part =
            world.packed_part(BAZAAR, id, bytes.len() as u64)?;
        assert!(!part.is_empty() && part.len() <= PACKAGE_PART);
        bytes.extend(part);
    }
    // Read to its end, it is let go.
    assert_eq!(
        world.packed_part(BAZAAR, id, 0),
        Err(Failure::NotFound)
    );
    Ok(bytes)
}

#[test]
fn an_agent_bought_moves_in_and_is_brought_up_to_a_new_version() {
    let home = Scratch::new("agents-settle-agent");
    let runtime = runtime();
    let mut app = core(&home, &runtime);
    let world = app.world_mut();
    let into = Destination::Hamlet(HOME.into());
    // Not a package, too big, or one that climbs out of its house.
    let climbing = packed(Contents::Agent {
        agent: Agent {
            name: "scribe".into(),
            purpose: "Writes.".into(),
            model: None,
            files: vec![text("../out.md", "x")],
        },
    });
    for package in
        [b"{}".to_vec(), vec![b' '; MOST_BYTES + 1], climbing]
    {
        let refused =
            settle(world, delivery("l", "1", package, into.clone()));
        assert!(matches!(refused, Err(Failure::Refused(_))));
    }
    // An agent is not founded as a hamlet.
    assert!(
        settle(
            world,
            delivery("l", "1", scribe("x"), Destination::Realm)
        )
        .is_err()
    );
    assert!(world.bought().is_empty());

    let words = settle(
        world,
        delivery("l", "1.0", scribe("Careful."), into.clone()),
    )
    .unwrap();
    assert!(words.contains("scribe"), "{words}");
    assert!(agents::named(world, "scribe").is_some());
    let house = Realm::new(home.to_path_buf())
        .hamlet(HOME)
        .unwrap()
        .house("scribe")
        .unwrap();
    assert!(house.skills().join("ink/SKILL.md").is_file());

    settle(world, delivery("l", "1.1", scribe("Bolder."), into))
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(house.character()).unwrap(),
        "Bolder."
    );
    let bought = world.bought();
    let scribe = &bought["l"];
    assert_eq!(scribe.kind, PackageKind::Agent);
    assert_eq!(scribe.version, "1.1");
    assert_eq!(scribe.names, ["scribe"]);
    assert_eq!(scribe.hamlet.as_deref(), Some(HOME));
    // Brought up, not hired twice.
    assert!(agents::named(world, "scribe-ii").is_none());
}

#[test]
fn a_hamlet_bought_comes_up_with_its_agents() {
    let home = Scratch::new("agents-settle-hamlet");
    let runtime = runtime();
    let mut app = core(&home, &runtime);
    let world = app.world_mut();
    let agent = |name: &str| Agent {
        name: name.into(),
        purpose: format!("{name} works."),
        model: None,
        files: vec![text(NOTES, &format!("# Notes\n{name}"))],
    };
    let press = packed(Contents::Hamlet {
        hamlet: Hamlet {
            name: "lamplight".into(),
            title: "Lamplight Press".into(),
            kind: "Publishing house".into(),
            purpose: "Turns a manuscript into a book.".into(),
            seed: "7341".into(),
            mayor: agent("publisher"),
            agents: vec![agent("editor")],
        },
    });
    // A hamlet goes into the realm, not into a hamlet.
    let into = Destination::Hamlet(HOME.into());
    assert!(
        settle(world, delivery("l", "1", press.clone(), into))
            .is_err()
    );
    settle(
        world,
        delivery("l", "1", press.clone(), Destination::Realm),
    )
    .unwrap();
    let publisher = agents::mayor_of(world, "lamplight").unwrap();
    assert_eq!(agents::named(world, "publisher"), Some(publisher));
    let editor = agents::named(world, "editor").unwrap();
    assert_eq!(agents::hamlet_of(world, editor), Some("lamplight"));
    // Another listing of the same name is refused; the same one
    // brings it up.
    assert!(
        settle(
            world,
            delivery("other", "1", press.clone(), Destination::Realm)
        )
        .is_err()
    );
    settle(world, delivery("l", "2", press, Destination::Realm))
        .unwrap();
    assert_eq!(world.bought()["l"].version, "2");
    assert!(!world.bought().contains_key("other"));
}

#[test]
fn what_is_packed_settles_back_whole_in_another_home() {
    let runtime = runtime();
    let seller = Scratch::new("agents-pack-seller");
    let buyer = Scratch::new("agents-pack-buyer");
    let mut selling = core(&seller, &runtime);
    let mut buying = core(&buyer, &runtime);
    let sell = selling.world_mut();
    let buy = buying.world_mut();

    // Tools are added to a house and listed.
    let tools = packed(Contents::Tools {
        tools: Tools {
            tools: vec![Tool {
                name: "cal".into(),
                files: vec![
                    text(
                        MANIFEST,
                        r#"{"name":"cal","exec":["./run"]}"#,
                    ),
                    File {
                        executable: true,
                        ..text("run", "#!/bin/sh\n")
                    },
                ],
            }],
        },
    });
    let mayor = Realm::new(seller.to_path_buf())
        .hamlet(HOME)
        .unwrap()
        .mayor()
        .name;
    let house = Destination::House {
        hamlet: HOME.into(),
        agent: mayor.clone(),
    };
    let words =
        settle(sell, delivery("t", "1", tools, house)).unwrap();
    assert!(words.contains("cal"), "{words}");
    let housed = sell.housed_tools();
    assert_eq!(
        housed,
        [HousedTool {
            hamlet: HOME.into(),
            agent: mayor,
            name: "cal".into(),
        }]
    );
    assert_eq!(sell.bought()["t"].kind, PackageKind::Tools);

    settle(
        sell,
        delivery(
            "a",
            "1",
            heavy_scribe(),
            Destination::Hamlet(HOME.into()),
        ),
    )
    .unwrap();
    let mill = Template {
        name: "mill".into(),
        title: "The Mill".into(),
        purpose: "Grind the grain.".into(),
        mayor: Settler {
            name: "miller".into(),
            purpose: "Grinds.".into(),
            character: "Calm.".into(),
            notes: "# Notes\nflour".into(),
            skills: Vec::new(),
        },
        agents: Vec::new(),
    };
    sell.found_from(mill).unwrap();

    // Nothing that is not there is packed.
    let unknown = [
        Packing::Agent {
            hamlet: HOME.into(),
            name: "nobody".into(),
            notes: true,
        },
        Packing::Hamlet {
            name: "nowhere".into(),
            kind: "Town".into(),
            notes: true,
        },
        Packing::Tools(vec![HousedTool {
            name: "clock".into(),
            ..housed[0].clone()
        }]),
    ];
    for packing in unknown {
        assert_eq!(pack(sell, packing), Err(Failure::NotFound));
    }

    let agent = pack(
        sell,
        Packing::Agent {
            hamlet: HOME.into(),
            name: "scribe".into(),
            notes: false,
        },
    )
    .unwrap();
    let hamlet = pack(
        sell,
        Packing::Hamlet {
            name: "mill".into(),
            kind: "Mill".into(),
            notes: true,
        },
    )
    .unwrap();
    let tools = pack(sell, Packing::Tools(housed)).unwrap();
    // The first hamlet's mayor has no purpose, which the bazaar's
    // server needs.
    let first = pack(
        sell,
        Packing::Hamlet {
            name: HOME.into(),
            kind: "Home".into(),
            notes: false,
        },
    )
    .unwrap();
    let Contents::Hamlet { hamlet: first } =
        Package::read(&first).unwrap().contents
    else {
        panic!("not a hamlet");
    };
    assert!(!first.purpose.trim().is_empty());
    assert!(!first.mayor.purpose.trim().is_empty());

    settle(
        buy,
        delivery("a", "1", agent, Destination::Hamlet(HOME.into())),
    )
    .unwrap();
    settle(buy, delivery("h", "1", hamlet, Destination::Realm))
        .unwrap();
    settle(
        buy,
        delivery(
            "t",
            "1",
            tools,
            Destination::House {
                hamlet: "mill".into(),
                agent: "miller".into(),
            },
        ),
    )
    .unwrap();
    let realm = Realm::new(buyer.to_path_buf());
    let read = |path: std::path::PathBuf| {
        std::fs::read_to_string(path).unwrap()
    };
    let scribe = realm.hamlet(HOME).unwrap().house("scribe").unwrap();
    assert_eq!(read(scribe.character()), "Careful.");
    let atlas =
        std::fs::metadata(scribe.skills().join("ink/atlas.bin"));
    assert_eq!(atlas.unwrap().len(), PACKAGE_PART as u64 * 2);
    let miller =
        realm.hamlet("mill").unwrap().house("miller").unwrap();
    assert_eq!(read(miller.notes()), "# Notes\nflour");
    assert!(agents::mayor_of(buy, "mill").is_some());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let run = std::fs::metadata(miller.tools().join("cal/run"))
            .unwrap();
        assert_ne!(run.permissions().mode() & 0o111, 0);
    }
    assert_eq!(buy.housed_tools().len(), 1);
}

#[test]
fn a_package_settles_only_from_its_parts_in_order() {
    let home = Scratch::new("agents-settle-parts");
    let runtime = runtime();
    let mut app = core(&home, &runtime);
    app.add_plugins(roster::plugin).init_resource::<Watchers>();
    app.world_mut().resource_mut::<Watchers>().watch(BAZAAR, 1);
    app.update();
    app.world_mut().resource_mut::<Watchers>().told();
    let world = app.world_mut();
    let whole = heavy_scribe();
    let parts = whole.chunks(PACKAGE_PART).collect::<Vec<_>>();
    assert_eq!(parts.len(), 3);
    let part = |at: usize, of: usize, bytes: &[u8]| Delivery {
        part: at as u32,
        parts: of as u32,
        ..delivery(
            "l",
            "1",
            bytes.to_vec(),
            Destination::Hamlet(HOME.into()),
        )
    };
    let refused = |reply| matches!(reply, Err(Failure::Refused(_)));
    let send = |world: &mut World, at, of, bytes: &[u8]| {
        world.settle_part(BAZAAR, part(at, of, bytes))
    };

    // A part skipped, a count that changes or a part too big drops
    // what came, so the next part is out of order too.
    for wrong in [(2, 3), (1, 4)] {
        assert_eq!(
            send(world, 0, 3, parts[0]),
            Ok(AgentsReply::Done)
        );
        assert!(refused(send(
            world,
            wrong.0,
            wrong.1,
            parts[wrong.0.min(2)]
        )));
        assert!(refused(send(world, 1, 3, parts[1])));
    }
    assert_eq!(send(world, 0, 3, parts[0]), Ok(AgentsReply::Done));
    assert!(refused(send(world, 1, 3, &vec![0; PACKAGE_PART + 1])));
    assert!(refused(send(world, 1, 3, parts[1])));
    assert!(refused(send(world, 3, 3, parts[0])));
    assert!(agents::named(world, "scribe").is_none());

    for (at, bytes) in parts.iter().enumerate() {
        let reply = send(world, at, 3, bytes).unwrap();
        match reply {
            AgentsReply::Done => assert!(at < 2),
            AgentsReply::Settled(_) => assert_eq!(at, 2),
            other => panic!("{other:?}"),
        }
    }
    assert!(agents::named(world, "scribe").is_some());
    // A pack is only for the plugin that packed it.
    let packed = world
        .hold_pack(
            BAZAAR,
            Packing::Agent {
                hamlet: HOME.into(),
                name: "scribe".into(),
                notes: false,
            },
        )
        .unwrap();
    let AgentsReply::Packed { id, .. } = packed else {
        panic!("{packed:?}");
    };
    let other = world.spawn_empty().id();
    assert_eq!(
        world.packed_part(other, id, 0),
        Err(Failure::NotFound)
    );
    assert_eq!(
        world.packed_part(BAZAAR, id + 1, 0),
        Err(Failure::NotFound)
    );

    // The watchers hear of the agent moved in.
    app.update();
    let told = app.world_mut().resource_mut::<Watchers>().told();
    let heard = told.iter().any(|event| {
        matches!(event, AgentsEvent::Roster(roster)
            if roster.agents.iter().any(|agent| agent.name == "scribe"))
    });
    assert!(heard, "{told:?}");
}
