use bevy::app::App;
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::ResMut;
use hamlet_client::realm::Providers;
use hamlet_protocol::client as protocol;
use hamlet_realm::pick::{Selected, Thing};
use hamlet_ui::theme::HamletTheme;
use serde_json::json;

use super::*;
use crate::bazaar::models::{Source, WareId};
use crate::bazaar::skills::SkillId;
use crate::bazaar::skills::skills_sh::Unrolled;
use crate::testing::{
    all_words, incoming, panel_app, press_and_update, sent,
};

/// The purchases the world's bazaar cheered.
#[derive(Resource, Default)]
struct Cheered(usize);

/// The bazaar's panel over the fixture state, open at `stall`.
fn bazaar_app(stall: Stall) -> App {
    let mut app = panel_app();
    app.add_plugins((crate::models::plugin, plugin))
        .init_resource::<crate::settings::Pane>()
        .init_resource::<Cheered>()
        .add_observer(
            |_: On<Traded>, mut cheered: ResMut<Cheered>| {
                cheered.0 += 1;
            },
        );
    app.world_mut().resource_mut::<StallShown>().0 = stall;
    // The samples are stocked at startup.
    app.update();
    app
}

fn mount(app: &mut App) {
    bevy_fynix::mount::<HamletTheme>(app.world_mut(), view::screen());
    app.update();
}

fn quick() -> Ware {
    Ware {
        source: Source::OpenRouter,
        id: "acme/quick".into(),
        title: "Acme: Quick".into(),
        description: "Small and fast. Good for chores.".into(),
        input: Some(0.1),
        output: Some(0.4),
        tools: Some(true),
        ..Ware::default()
    }
}

#[test]
fn a_model_from_openrouter_is_listed_then_allowed_in_the_catalogue() {
    let mut app = bazaar_app(Stall::Models);
    app.world_mut().resource_mut::<Providers>().0.push(
        protocol::ProviderView {
            configured: true,
            id: "openrouter".into(),
            label: "Sign in with OpenRouter".into(),
            methods: Vec::new(),
            name: "OpenRouter".into(),
        },
    );
    app.world_mut()
        .resource_mut::<Stock<Ware>>()
        .stock(vec![quick()]);
    app.world_mut().resource_mut::<Shelf<Ware>>().open =
        Some(WareId {
            source: Source::OpenRouter,
            id: "acme/quick".into(),
        });
    mount(&mut app);
    sent(&mut app);
    press_and_update(&mut app, "ADD");
    // The server has not listed it yet, so it is asked to.
    let read = sent(&mut app).remove(0);
    assert_eq!(
        (read["what"].clone(), read["refresh"].clone()),
        (json!("models"), json!(true))
    );
    incoming(
        &mut app,
        json!({"type":"read", "to":read["id"], "models":[
            {"name":"openrouter::acme/quick", "price":null, "priced":"unknown", "context":null, "allowed":false, "note":""}
        ]}),
    );
    let edit = sent(&mut app).remove(0);
    assert_eq!(edit["doc"], "catalogue");
    assert_eq!(edit["model"], "openrouter::acme/quick");
    assert_eq!(
        edit["set"],
        json!({"allowed": true, "note": "Small and fast"})
    );
    incoming(
        &mut app,
        json!({"type":"reply", "to":edit["id"], "kind":"notice", "text":"Saved."}),
    );
    assert!(all_words(&mut app).contains("Added Acme: Quick."));
    assert_eq!(app.world().resource::<Cheered>().0, 1);
}

#[test]
fn a_model_with_no_way_to_openrouter_waits_for_one() {
    let mut app = bazaar_app(Stall::Models);
    app.world_mut()
        .resource_mut::<Stock<Ware>>()
        .stock(vec![quick()]);
    app.world_mut().resource_mut::<Shelf<Ware>>().open =
        Some(WareId {
            source: Source::OpenRouter,
            id: "acme/quick".into(),
        });
    mount(&mut app);
    sent(&mut app);
    press_and_update(&mut app, "ADD");
    assert!(sent(&mut app).is_empty());
    assert!(all_words(&mut app).contains("NO OPENROUTER YET"));
}

#[test]
fn a_skill_taken_from_skills_sh_goes_to_the_library_with_its_files() {
    let mut app = bazaar_app(Stall::Skills);
    let scroll = Scroll {
        source: "acme/tools".into(),
        skill: "maps".into(),
        name: "maps".into(),
        installs: 1200,
        rank: Some(1),
        official: false,
    };
    let id = SkillId("acme/tools/maps".into());
    app.world_mut()
        .resource_mut::<Stock<Scroll>>()
        .stock(vec![scroll]);
    app.world_mut()
        .resource_mut::<skills::Desk>()
        .unrolled
        .insert(
            id.clone(),
            Some(Ok(Unrolled {
                files: vec![
                    (
                        "SKILL.md".into(),
                        "---\nname: maps\ndescription: Draws \
                         maps.\n---\n\nDraw.\n"
                            .into(),
                    ),
                    ("scripts/run.sh".into(), "echo hi\n".into()),
                ],
                left: Vec::new(),
            })),
        );
    app.world_mut().resource_mut::<Shelf<Scroll>>().open = Some(id);
    mount(&mut app);
    sent(&mut app);
    press_and_update(&mut app, "ADD TO LIBRARY");
    let take = sent(&mut app).remove(0);
    assert_eq!(take["type"], "take-skill");
    assert_eq!(take["name"], "maps");
    assert_eq!(take["from"], "skills.sh/acme/tools/maps");
    assert_eq!(
        take["files"][1],
        json!({"path": "scripts/run.sh", "text": "echo hi\n"})
    );
    incoming(
        &mut app,
        json!({"type":"reply", "to":take["id"], "kind":"error", "text":"the library already has a skill named `maps`"}),
    );
    assert!(all_words(&mut app).contains(
        "Couldn't add it to the library: the library already has a \
         skill named `maps`."
    ));
    assert_eq!(app.world().resource::<Cheered>().0, 0);
}

#[test]
fn a_hamlet_template_is_founded_whole_and_cheered() {
    let mut app = bazaar_app(Stall::Hamlets);
    app.world_mut().resource_mut::<Shelf<Template>>().open =
        Some("saas-launch".into());
    mount(&mut app);
    sent(&mut app);
    press_and_update(&mut app, "FOUND THIS HAMLET");
    let found = sent(&mut app).remove(0);
    assert_eq!(found["type"], "found-from");
    let template = &found["template"];
    assert_eq!(template["name"], "saas-launch");
    assert_eq!(template["mayor"]["name"], "launch-lead");
    assert_eq!(template["agents"].as_array().unwrap().len(), 3);
    assert!(
        all_words(&mut app)
            .contains("Founding Indie SaaS Launch Team...")
    );
    incoming(
        &mut app,
        json!({"type":"reply", "to":found["id"], "kind":"notice", "text":"Founded."}),
    );
    assert!(
        all_words(&mut app)
            .contains("Indie SaaS Launch Team stands in your realm")
    );
    assert_eq!(app.world().resource::<Cheered>().0, 1);
}

#[test]
fn a_stall_chosen_in_the_world_is_the_one_shown() {
    let mut app = bazaar_app(Stall::Models);
    app.insert_resource(Selected(Some(Thing::Stall(Stall::Hamlets))));
    app.update();
    assert_eq!(
        app.world().resource::<StallShown>().0,
        Stall::Hamlets
    );
}
