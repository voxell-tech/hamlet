//! The agent the chat is with, and the ways to point it at another.

use bevy::app::App;
use bevy::ecs::entity::Entity;
use bevy::ecs::lifecycle::RemovedComponents;
use bevy::ecs::name::Name;
use bevy::ecs::query::{Added, Changed, Or, With};
use bevy::ecs::system::Query;
use bevy::ecs::world::World;
use hamlet_core::agents::{self, Citizen, Mayor};
use hamlet_core::session::{self, SessionKey};
use hamlet_rules::hamlet::HOME;
pub use hamlet_sdk::convo::Convo;
use hamlet_world::pick::{Selected, Thing, select};

use crate::dock::show_tab;

/// Opens the chat with the first mayor.
pub(crate) fn plugin(app: &mut App) {
    // The name of this window's chief: `HAMLET_USER`, then `USER`.
    let user = std::env::var("HAMLET_USER")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "user".into());

    let key = SessionKey {
        channel: "app".into(),
        chat: user.clone(),
        thread: "main".into(),
    };
    let session = session::ensure(app.world_mut(), &key);
    let mayor =
        agents::mayor(app.world_mut()).unwrap_or(Entity::PLACEHOLDER);
    session::gather(app.world_mut(), mayor);
    app.insert_resource(Convo {
        session,
        key,
        user,
        mayor: session,
        agent: mayor,
        with: None,
    });
}

/// The session a chat with `agent` is kept under, and the agent's
/// name. `None` for the name of a mayor, and nothing at all for an
/// agent that is not there.
pub(crate) fn address(
    world: &World,
    agent: Entity,
) -> Option<(SessionKey, Option<String>)> {
    let name = world.get::<Name>(agent)?;
    let with = world
        .get::<Mayor>(agent)
        .is_none()
        .then(|| name.as_str().to_string());
    let convo = world.resource::<Convo>();
    let key = match &with {
        Some(name) => {
            SessionKey::with_agent("app", &convo.user, name)
        }
        None => SessionKey::with_mayor(
            "app",
            &convo.user,
            agents::hamlet_of(world, agent).unwrap_or(HOME),
        ),
    };
    Some((key, with))
}

/// Makes `agent` who the chat is with. False when it is not there.
pub(crate) fn seat(world: &mut World, agent: Entity) -> bool {
    let Some((key, with)) = address(world, agent) else {
        return false;
    };
    let session = session::ensure(world, &key);
    // Its letters to and from other agents are part of its chat.
    session::gather(world, agent);
    let mut convo = world.resource_mut::<Convo>();
    convo.session = session;
    convo.key = key;
    convo.agent = agent;
    convo.with = with;
    true
}

/// Points the chat at one of the hamlet's agents, and selects it in
/// the world.
pub(crate) fn talk_to(world: &mut World, agent: Entity) {
    if !seat(world, agent) {
        return;
    }
    let mayor = agents::mayor(world) == Some(agent);
    let selected =
        world.get_resource::<Selected>().map(|selected| selected.0);
    let same = match selected {
        Some(Some(Thing::Keeper | Thing::Cottage)) => mayor,
        Some(Some(
            Thing::Resident(of) | Thing::ResidentHouse(of),
        )) => of == agent,
        Some(_) => false,
        // Nothing can be selected where there is no world.
        None => true,
    };
    if !same {
        let thing = if mayor {
            Thing::Keeper
        } else {
            Thing::Resident(agent)
        };
        select(world, Some(thing));
    }
    show_tab(world, "agent");
}

/// Points the chat back at the mayor.
pub(crate) fn talk_to_mayor(world: &mut World) {
    if let Some(mayor) = agents::mayor(world) {
        talk_to(world, mayor);
    }
}

/// Points the chat at whoever is mayor of `hamlet`.
pub(crate) fn talk_to_mayor_of(world: &mut World, hamlet: &str) {
    if let Some(mayor) = agents::mayor_of(world, hamlet) {
        talk_to(world, mayor);
    }
}

/// An agent went, an office changed hands or a name changed.
pub(crate) fn roster_moved(
    named: Query<
        (),
        (With<Citizen>, Or<(Added<Mayor>, Changed<Name>)>),
    >,
    mut gone: RemovedComponents<Citizen>,
    mut unseated: RemovedComponents<Mayor>,
) -> bool {
    let removed = gone.read().count() + unseated.read().count();
    removed > 0 || !named.is_empty()
}

/// Keeps [`Convo`] true to the agent the chat is with, and to the
/// first mayor once that agent is gone.
pub(crate) fn reseat(world: &mut World) {
    let convo = world.resource::<Convo>();
    let agent = convo.agent;
    match address(world, agent) {
        Some((key, with)) => {
            if convo.key != key || convo.with != with {
                seat(world, agent);
            }
        }
        None => {
            if let Some(mayor) = agents::mayor(world) {
                seat(world, mayor);
            }
        }
    }
    // An agent that is gone from the world cannot stay selected.
    let gone = match world.get_resource::<Selected>().map(|s| s.0) {
        Some(Some(
            Thing::Resident(agent)
            | Thing::ResidentHouse(agent)
            | Thing::Ruin(agent),
        )) => world.get_entity(agent).is_err(),
        _ => false,
    };
    if gone {
        select(world, None);
    }
}

#[cfg(test)]
mod tests {
    use bevy::ecs::observer::On;
    use bevy::ecs::resource::Resource;
    use bevy::ecs::system::{Commands, ResMut};
    use hamlet_core::agents::{AgentsPlugin, Settlement};
    use hamlet_core::core::HomeDir;
    use hamlet_core::session::UserMessage;
    use hamlet_rules::hamlet::Hamlet;
    use hamlet_testing::Scratch;

    use super::*;

    /// Every message that reached the core, with its session.
    #[derive(Resource, Default)]
    struct Heard(Vec<(SessionKey, String)>);

    /// A home with two agents, a chat with the mayor, and a core
    /// that carries out the chief's commands and nothing else.
    fn app(tag: &str) -> (Scratch, App, Hamlet) {
        let dir = Scratch::new(&format!("ui-me-{tag}"));
        let hamlet = Hamlet::new(dir.to_path_buf());
        hamlet
            .create("librarian", "Keeps the list.", "test")
            .unwrap();
        hamlet.create("scout", "Looks things up.", "test").unwrap();
        let mut app = App::new();
        app.add_plugins(AgentsPlugin {
            home: dir.to_path_buf(),
        })
        .init_resource::<Heard>()
        .insert_resource(HomeDir(hamlet_home::Home {
            root: dir.to_path_buf(),
            workspace: dir.join("workspace"),
        }))
        .add_observer(crate::chat::decreed)
        .add_observer(
            |said: On<UserMessage>,
             mut heard: ResMut<Heard>,
             mut commands: Commands| {
                heard.0.push((said.key.clone(), said.text.clone()));
                let said = said.event().clone();
                commands.queue(move |world: &mut World| {
                    let home = world
                        .query::<&Settlement>()
                        .iter(world)
                        .next()
                        .map(Settlement::record);
                    if let Some(home) = home {
                        let _ = home.decree(&said.user, &said.text);
                    }
                    agents::sync(world);
                });
            },
        );
        let mayor = agents::mayor(app.world_mut()).unwrap();
        app.insert_resource(Convo::of(mayor, mayor, "alice", None));
        (dir, app, hamlet)
    }

    #[test]
    fn a_deed_goes_to_the_hamlet_of_the_agent_it_is_about() {
        use hamlet_rules::realm::{Origin, Realm};

        use crate::world_menu::Deed;

        let (dir, mut app, _) = app("deed");
        let user = "alice".to_string();
        let mill = Realm::new(dir.to_path_buf())
            .found(
                "mill",
                "miller",
                "Grinds.",
                Origin::Human { user },
            )
            .unwrap();
        mill.create("carder", "Cards wool.", "test").unwrap();
        agents::sync(app.world_mut());
        let carder =
            agents::named(app.world_mut(), "carder").unwrap();
        // The chat is with the first hamlet's mayor.
        Deed::Remove(carder).run(app.world_mut());
        let said = &app.world().resource::<Heard>().0;
        assert_eq!(
            said[0].0,
            SessionKey::with_mayor("app", "alice", "mill")
        );
        assert_eq!(said[0].1, "/remove carder");
    }

    #[test]
    fn the_chat_stays_true_to_its_agent_as_offices_change_and_agents_go()
     {
        let (_dir, mut app, hamlet) = app("reseat");
        let world = app.world_mut();
        let scout = agents::named(world, "scout").unwrap();
        assert!(seat(world, scout));
        assert_eq!(
            world.resource::<Convo>().with.as_deref(),
            Some("scout")
        );
        // The agent it is with takes the office.
        hamlet.set_mayor("scout", "chief:alice", "x").unwrap();
        agents::sync(world);
        reseat(world);
        let convo = world.resource::<Convo>();
        assert_eq!(
            (convo.agent, convo.with.as_deref()),
            (scout, None)
        );
        assert_eq!(
            convo.key,
            SessionKey::with_mayor("app", "alice", HOME)
        );
        // One that is gone from the world gives way to the mayor.
        let librarian = agents::named(world, "librarian").unwrap();
        assert!(seat(world, librarian));
        world.despawn(librarian);
        reseat(world);
        assert_eq!(world.resource::<Convo>().agent, scout);
    }
}
