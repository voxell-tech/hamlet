//! The chief in the window (docs/chief.md): the company its mascot
//! keeps and the look kept between launches.

use bevy::app::{App, Startup, Update};
use bevy::ecs::change_detection::{DetectChanges, Ref};
use bevy::ecs::entity::Entity;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Query, Res};
use bevy::ecs::world::World;
use hamlet_agents_api::convo::Me;
use hamlet_chat::me::Followed;
use hamlet_core::core::HomeDir;
use hamlet_core::session::UserMessage;
use hamlet_realm::chief::{
    self, Beside, Chief, Look, Player, Stands,
};
use hamlet_realm::library::{Library, Stood};
use hamlet_realm::pick::{Selected, Thing};

pub(crate) fn plugin(app: &mut App) {
    app.add_observer(spoke)
        .add_systems(Startup, enter)
        .add_systems(Update, (keep, accompany.after(Followed)));
}

/// Brings the chief this window belongs to into the world, with the
/// look that was kept for them.
pub(crate) fn enter(world: &mut World) {
    let user = world.resource::<Me>().user.clone();
    if let Some(chief) = chief::named(world, &user) {
        world.entity_mut(chief).insert(Player);
        return;
    }
    let home = world.resource::<HomeDir>().0.root.clone();
    let chief = chief::arrive(world, &user, Look::read(&home, &user));
    world.entity_mut(chief).insert(Player);
    if let Some(stands) = Stands::read(&home, &user) {
        world.entity_mut(chief).insert(stands);
    }
    // The place the library was last set down, too.
    let kept = Stands::read_at(&Stood::file(&home));
    if let (Some(Stands(at)), Some(mut stood)) =
        (kept, world.get_resource_mut::<Stood>())
    {
        stood.0 = Some(at);
    }
}

/// The company of the mascot of a chief who has chosen `selected`,
/// when the chat is with `agent`: that agent while it or its house is
/// selected, and a hamlet while its common is.
fn company(selected: Option<Thing>, agent: Entity) -> Option<Entity> {
    match selected {
        Some(
            Thing::Keeper
            | Thing::Cottage
            | Thing::Resident(_)
            | Thing::ResidentHouse(_),
        ) => Some(agent),
        Some(Thing::Common(hamlet)) => Some(hamlet),
        _ => None,
    }
}

/// Sends the mascot to whoever the chat is pointed at, and home when
/// nobody is selected.
pub(crate) fn accompany(
    me: Res<Me>,
    selected: Res<Selected>,
    libraries: Query<Entity, With<Library>>,
    mut chiefs: Query<(&Chief, &mut Beside)>,
) {
    if !me.is_changed() && !selected.is_changed() {
        return;
    }
    // To the library when that is what was chosen.
    let with = match selected.0 {
        Some(Thing::Library) => libraries.iter().next(),
        chosen => company(chosen, me.agent),
    };
    for (chief, mut beside) in &mut chiefs {
        if chief.user == me.user && beside.0 != with {
            beside.0 = with;
        }
    }
}

/// A message sent with nobody selected brings the mascot from home
/// to whoever it was sent to.
pub(crate) fn spoke(
    message: On<UserMessage>,
    me: Res<Me>,
    mut chiefs: Query<(&Chief, &mut Beside)>,
) {
    if message.key != me.key {
        return;
    }
    for (chief, mut beside) in &mut chiefs {
        if chief.user == me.user && beside.0.is_none() {
            beside.0 = Some(me.agent);
        }
    }
}

/// Writes a look that was changed to its chief's file.
pub fn keep(home: Res<HomeDir>, looks: Query<(&Chief, Ref<Look>)>) {
    for (chief, look) in &looks {
        if look.is_changed() && !look.is_added() {
            let _ = look.write(&home.0.root, &chief.user);
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::app::{App, Update};
    use bevy::ecs::name::Name;
    use hamlet_core::session::SessionKey;
    use hamlet_realm::chief::{Cloak, Hat, Light};

    use super::*;

    fn app() -> (App, Entity, Entity) {
        let mut app = App::new();
        let world = app.world_mut();
        let agent = world.spawn(Name::new("Juniper")).id();
        let session = world.spawn_empty().id();
        world.insert_resource(Me::of(
            session,
            agent,
            "wren",
            Some("Juniper"),
        ));
        world.init_resource::<Selected>();
        let chief = chief::arrive(world, "wren", Look::default());
        app.add_systems(Update, accompany).add_observer(spoke);
        (app, chief, agent)
    }

    fn beside(app: &App, chief: Entity) -> Option<Entity> {
        app.world().get::<Beside>(chief).unwrap().0
    }

    #[test]
    fn the_mascot_keeps_whoever_is_selected_company() {
        let (mut app, chief, agent) = app();
        app.update();
        assert_eq!(beside(&app, chief), None);

        let pick = |app: &mut App, thing: Option<Thing>| {
            app.world_mut().resource_mut::<Selected>().0 = thing;
            app.update();
        };
        pick(&mut app, Some(Thing::Resident(agent)));
        assert_eq!(beside(&app, chief), Some(agent));
        pick(&mut app, Some(Thing::ResidentHouse(agent)));
        assert_eq!(beside(&app, chief), Some(agent));

        // Its own place, a hamlet or nothing at all: it goes home.
        pick(&mut app, Some(Thing::ChiefPlace(chief)));
        assert_eq!(beside(&app, chief), None);
        pick(&mut app, Some(Thing::Resident(agent)));
        pick(&mut app, None);
        assert_eq!(beside(&app, chief), None);
    }

    #[test]
    fn a_message_with_nobody_selected_brings_the_mascot() {
        let (mut app, chief, agent) = app();
        app.update();
        let key = app.world().resource::<Me>().key.clone();
        // Another chat's message is not this chief's.
        app.world_mut().trigger(UserMessage {
            key: SessionKey::with_agent("app", "wren", "Sorrel"),
            user: "wren".into(),
            text: "hello".into(),
        });
        assert_eq!(beside(&app, chief), None);
        app.world_mut().trigger(UserMessage {
            key,
            user: "wren".into(),
            text: "hello".into(),
        });
        assert_eq!(beside(&app, chief), Some(agent));
    }

    #[test]
    fn a_changed_look_is_kept_and_a_new_one_is_not_written() {
        let home = hamlet_testing::Scratch::new("chief-look");
        let mut app = App::new();
        let world = app.world_mut();
        world.insert_resource(HomeDir(hamlet_home::Home {
            root: home.to_path_buf(),
            workspace: home.join("workspace"),
        }));
        let chief = chief::arrive(world, "wren", Look::default());
        app.add_systems(Update, keep);
        app.update();
        assert!(!Look::file(&home, "wren").exists());

        let mut look =
            app.world_mut().get_mut::<Look>(chief).unwrap();
        look.hat = Hat::Crown;
        look.light = Light::Moon;
        app.update();
        let kept = Look::read(&home, "wren");
        assert_eq!(kept.hat, Hat::Crown);
        assert_eq!(kept.light, Light::Moon);
        assert_eq!(kept.cloak, Cloak::default());
    }
}
