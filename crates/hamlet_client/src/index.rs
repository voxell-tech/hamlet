//! The hamlets, agents, waits, chats and terminals by the ids the
//! server names them with, kept by their components' hooks.

use std::collections::HashMap;
use std::hash::Hash;

use bevy_app::App;
use bevy_ecs::lifecycle::HookContext;
use bevy_ecs::prelude::*;
use bevy_ecs::world::DeferredWorld;
use hamlet_protocol::client as protocol;

use crate::chats::Chat;
use crate::realm::{
    AgentNumber, HOME, HamletId, Left, LivesIn, MAYOR, Mayor,
};
use crate::terminals::TerminalId;
use crate::waits::Waiting;

/// The index and the observers keeping agents' names in it.
pub(crate) fn plugin(app: &mut App) {
    // `Name` is Bevy's, and its hooks cannot be set once any entity
    // has one, so observers follow it.
    app.init_resource::<Index>()
        .add_observer(
            |named: On<Insert<Name>>, world: DeferredWorld| {
                refile(world, named.entity);
            },
        )
        .add_observer(
            |unnamed: On<Discard<Name>>, world: DeferredWorld| {
                edit(world, unnamed.entity, |filed| Filed {
                    name: None,
                    ..filed
                });
            },
        );
}

/// The entities of the server's hamlets, agents, waits, chats and
/// terminals by their protocol ids.
#[derive(Resource, Default)]
pub(crate) struct Index {
    hamlets: HashMap<String, Entity>,
    agents: HashMap<Entity, Filed>,
    numbers: HashMap<(Entity, u64), Vec<Entity>>,
    /// Agents that have not left, by shown name.
    names: HashMap<String, Vec<Entity>>,
    /// Agents in office that have not left, by hamlet.
    mayors: HashMap<Entity, Vec<Entity>>,
    waits: HashMap<String, Entity>,
    // `ChatRef` has no `Hash`, and a window watches few chats.
    chats: Vec<(Entity, protocol::ChatRef)>,
    terminals: HashMap<String, Entity>,
}

/// What an agent is found by.
#[derive(Clone, Debug, PartialEq)]
struct Filed {
    hamlet: Entity,
    number: u64,
    name: Option<String>,
    mayor: bool,
    left: bool,
}

/// What a chat addresses may have changed: an agent arrived, left,
/// was renamed or took the office, or a hamlet came or went.
#[derive(Event)]
pub(crate) struct Refiled;

impl Index {
    /// A hamlet by its protocol id.
    pub(crate) fn hamlet(&self, id: &str) -> Option<Entity> {
        self.hamlets.get(id).copied()
    }

    /// An agent by its hamlet's id and its number, living or not.
    pub(crate) fn agent(
        &self,
        reference: &protocol::AgentRef,
    ) -> Option<Entity> {
        let hamlet = self.hamlet(&reference.hamlet)?;
        first(&self.numbers, &(hamlet, reference.number))
    }

    /// A living agent by name, or the first hamlet's mayor by its
    /// office.
    pub(crate) fn named(&self, name: &str) -> Option<Entity> {
        if name == MAYOR {
            return self.mayor_of(HOME);
        }
        first(&self.names, name)
    }

    /// The current mayor of a hamlet by its protocol id.
    pub(crate) fn mayor_of(&self, id: &str) -> Option<Entity> {
        first(&self.mayors, &self.hamlet(id)?)
    }

    /// The living agent at a chat's address, the mayor by its office.
    pub(crate) fn at(
        &self,
        address: &protocol::Address,
    ) -> Option<Entity> {
        if address.agent == MAYOR {
            return self.mayor_of(&address.hamlet);
        }
        let hamlet = self.hamlet(&address.hamlet)?;
        self.names.get(&address.agent)?.iter().copied().find(
            |agent| {
                self.agents
                    .get(agent)
                    .is_some_and(|filed| filed.hamlet == hamlet)
            },
        )
    }

    /// A wait by its server id.
    pub(crate) fn wait(&self, id: &str) -> Option<Entity> {
        self.waits.get(id).copied()
    }

    /// The chats watching `reference`.
    pub(crate) fn chats_at<'a>(
        &'a self,
        reference: &'a protocol::ChatRef,
    ) -> impl Iterator<Item = Entity> + 'a {
        self.chats
            .iter()
            .filter(move |(_, at)| at == reference)
            .map(|(chat, _)| *chat)
    }

    /// Every watched chat with what it watches.
    pub(crate) fn chats(
        &self,
    ) -> impl Iterator<Item = (Entity, &protocol::ChatRef)> {
        self.chats
            .iter()
            .map(|(chat, reference)| (*chat, reference))
    }

    pub(crate) fn hamlets(
        &self,
    ) -> impl Iterator<Item = Entity> + '_ {
        self.hamlets.values().copied()
    }

    /// Every agent with a number in a hamlet, living or not.
    pub(crate) fn agents(&self) -> impl Iterator<Item = Entity> + '_ {
        self.agents.keys().copied()
    }

    pub(crate) fn waits(&self) -> impl Iterator<Item = Entity> + '_ {
        self.waits.values().copied()
    }

    /// A terminal by its server id.
    pub(crate) fn terminal(&self, id: &str) -> Option<Entity> {
        self.terminals.get(id).copied()
    }

    pub(crate) fn terminals(
        &self,
    ) -> impl Iterator<Item = Entity> + '_ {
        self.terminals.values().copied()
    }

    /// Files `agent` as `now`.
    fn refile(&mut self, agent: Entity, now: Option<Filed>) {
        let was = match &now {
            Some(filed) => self.agents.insert(agent, filed.clone()),
            None => self.agents.remove(&agent),
        };
        let (was, now) = (was.as_ref(), now.as_ref());
        let number = |filed: &Filed| (filed.hamlet, filed.number);
        let name = |filed: &Filed| {
            filed.name.clone().filter(|_| !filed.left)
        };
        let seat = |filed: &Filed| {
            (filed.mayor && !filed.left).then_some(filed.hamlet)
        };
        moved(
            &mut self.numbers,
            agent,
            was.map(number),
            now.map(number),
        );
        moved(
            &mut self.names,
            agent,
            was.and_then(name),
            now.and_then(name),
        );
        moved(
            &mut self.mayors,
            agent,
            was.and_then(seat),
            now.and_then(seat),
        );
    }
}

fn first<K, Q>(
    map: &HashMap<K, Vec<Entity>>,
    key: &Q,
) -> Option<Entity>
where
    K: Hash + Eq + std::borrow::Borrow<Q>,
    Q: Hash + Eq + ?Sized,
{
    map.get(key)?.first().copied()
}

/// Moves `agent` between keys, keeping the order of the others, so
/// the first found stays the first filed.
fn moved<K: Hash + Eq>(
    map: &mut HashMap<K, Vec<Entity>>,
    agent: Entity,
    was: Option<K>,
    now: Option<K>,
) {
    if was == now {
        return;
    }
    if let Some(key) = was
        && let Some(agents) = map.get_mut(&key)
    {
        agents.retain(|filed| *filed != agent);
        if agents.is_empty() {
            map.remove(&key);
        }
    }
    if let Some(key) = now {
        map.entry(key).or_default().push(agent);
    }
}

fn read(world: &World, agent: Entity) -> Option<Filed> {
    let entity = world.get_entity(agent).ok()?;
    Some(Filed {
        hamlet: entity.get::<LivesIn>()?.0,
        number: entity.get::<AgentNumber>()?.0,
        name: entity
            .get::<Name>()
            .map(|name| name.as_str().to_owned()),
        mayor: entity.contains::<Mayor>(),
        left: entity.contains::<Left>(),
    })
}

fn refile(world: DeferredWorld, agent: Entity) {
    let filed = read(&world, agent);
    change(world, agent, |_| filed);
}

/// Edits an agent's filing from what is kept, since a despawn
/// discards each component while the others are still there.
fn edit(
    world: DeferredWorld,
    agent: Entity,
    edit: fn(Filed) -> Filed,
) {
    change(world, agent, |filed| filed.map(edit));
}

fn change(
    mut world: DeferredWorld,
    agent: Entity,
    edit: impl FnOnce(Option<Filed>) -> Option<Filed>,
) {
    let Some(index) = world.get_resource::<Index>() else {
        return;
    };
    let was = index.agents.get(&agent);
    let now = edit(was.cloned());
    // A Name on a robot or a UI node changes nothing here, and must
    // not mark the Index changed.
    if was == now.as_ref() {
        return;
    }
    world.resource_mut::<Index>().refile(agent, now);
    world.commands().trigger(Refiled);
}

/// Files an agent again from its components.
pub(crate) fn file_agent(world: DeferredWorld, context: HookContext) {
    refile(world, context.entity);
}

/// Drops an agent losing its number or its hamlet.
pub(crate) fn unfile_agent(
    world: DeferredWorld,
    context: HookContext,
) {
    change(world, context.entity, |_| None);
}

pub(crate) fn unseat(world: DeferredWorld, context: HookContext) {
    edit(world, context.entity, |filed| Filed {
        mayor: false,
        ..filed
    });
}

pub(crate) fn come_back(world: DeferredWorld, context: HookContext) {
    edit(world, context.entity, |filed| Filed {
        left: false,
        ..filed
    });
}

pub(crate) fn file_hamlet(
    mut world: DeferredWorld,
    context: HookContext,
) {
    let Some(id) =
        world.get::<HamletId>(context.entity).map(|id| id.0.clone())
    else {
        return;
    };
    let Some(mut index) = world.get_resource_mut::<Index>() else {
        return;
    };
    index.hamlets.insert(id, context.entity);
    world.commands().trigger(Refiled);
}

pub(crate) fn unfile_hamlet(
    mut world: DeferredWorld,
    context: HookContext,
) {
    let Some(id) =
        world.get::<HamletId>(context.entity).map(|id| id.0.clone())
    else {
        return;
    };
    let Some(index) = world.get_resource::<Index>() else {
        return;
    };
    if index.hamlets.get(&id) == Some(&context.entity) {
        world.resource_mut::<Index>().hamlets.remove(&id);
        world.commands().trigger(Refiled);
    }
}

pub(crate) fn file_wait(
    mut world: DeferredWorld,
    context: HookContext,
) {
    let Some(id) = world
        .get::<Waiting>(context.entity)
        .map(|wait| wait.id.clone())
    else {
        return;
    };
    if let Some(mut index) = world.get_resource_mut::<Index>() {
        index.waits.insert(id, context.entity);
    }
}

pub(crate) fn unfile_wait(
    mut world: DeferredWorld,
    context: HookContext,
) {
    let Some(id) = world
        .get::<Waiting>(context.entity)
        .map(|wait| wait.id.clone())
    else {
        return;
    };
    if let Some(mut index) = world.get_resource_mut::<Index>()
        && index.waits.get(&id) == Some(&context.entity)
    {
        index.waits.remove(&id);
    }
}

pub(crate) fn file_chat(
    mut world: DeferredWorld,
    context: HookContext,
) {
    let Some(reference) =
        world.get::<Chat>(context.entity).map(|chat| chat.0.clone())
    else {
        return;
    };
    if let Some(mut index) = world.get_resource_mut::<Index>() {
        index.chats.push((context.entity, reference));
    }
}

pub(crate) fn unfile_chat(
    mut world: DeferredWorld,
    context: HookContext,
) {
    if let Some(mut index) = world.get_resource_mut::<Index>() {
        index.chats.retain(|(chat, _)| *chat != context.entity);
    }
}

pub(crate) fn file_terminal(
    mut world: DeferredWorld,
    context: HookContext,
) {
    let Some(id) = world
        .get::<TerminalId>(context.entity)
        .map(|id| id.0.clone())
    else {
        return;
    };
    if let Some(mut index) = world.get_resource_mut::<Index>() {
        index.terminals.insert(id, context.entity);
    }
}

pub(crate) fn unfile_terminal(
    mut world: DeferredWorld,
    context: HookContext,
) {
    let Some(id) = world
        .get::<TerminalId>(context.entity)
        .map(|id| id.0.clone())
    else {
        return;
    };
    if let Some(mut index) = world.get_resource_mut::<Index>()
        && index.terminals.get(&id) == Some(&context.entity)
    {
        index.terminals.remove(&id);
    }
}

#[cfg(test)]
mod tests {
    use bevy_app::App;
    use bevy_ecs::prelude::*;
    use hamlet_protocol::client as protocol;
    use hamlet_testing::realm::{
        agent, hamlet, reference, state, wait,
    };

    use crate::realm::{self, Left};
    use crate::{ClientPlugin, receive, waits};

    fn app(state: protocol::State) -> App {
        let mut app = App::new();
        app.add_plugins(ClientPlugin { server: None });
        receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        app
    }

    fn at(app: &App, hamlet: &str, agent: &str) -> Option<Entity> {
        app.world().resource::<super::Index>().at(
            &protocol::Address {
                hamlet: hamlet.into(),
                agent: agent.into(),
            },
        )
    }

    #[test]
    fn a_renamed_agent_is_found_by_its_new_name_only() {
        let mut app = app(state());
        let founder = realm::named(app.world(), "founder").unwrap();
        app.world_mut()
            .entity_mut(founder)
            .insert(Name::new("keeper"));
        assert_eq!(realm::named(app.world(), "founder"), None);
        assert_eq!(
            realm::named(app.world(), "keeper"),
            Some(founder)
        );
        assert_eq!(at(&app, "home", "keeper"), Some(founder));
    }

    #[test]
    fn an_agent_that_left_is_found_again_when_it_comes_back() {
        let mut app = app(state());
        let founder = realm::mayor(app.world()).unwrap();
        app.world_mut().entity_mut(founder).insert(Left);
        assert_eq!(realm::named(app.world(), "founder"), None);
        assert_eq!(realm::mayor(app.world()), None);
        assert_eq!(at(&app, "home", "mayor"), None);
        let number = reference("home", 0);
        assert_eq!(realm::agent(app.world(), &number), Some(founder));
        app.world_mut().entity_mut(founder).remove::<Left>();
        assert_eq!(
            realm::named(app.world(), "founder"),
            Some(founder)
        );
        assert_eq!(realm::mayor(app.world()), Some(founder));
    }

    #[test]
    fn a_removed_wait_is_no_longer_found() {
        let mut realm = state();
        realm.waits.push(wait("first"));
        realm.waits.push(wait("second"));
        let mut app = app(realm.clone());
        let first = waits::find(app.world(), "first").unwrap();
        realm.waits.remove(0);
        realm.revision += 1;
        receive(
            app.world_mut(),
            protocol::ServerMessage::State(realm),
        );
        assert!(app.world().get_entity(first).is_err());
        assert_eq!(waits::find(app.world(), "first"), None);
        assert!(waits::find(app.world(), "second").is_some());
    }

    #[test]
    fn a_closed_hamlet_takes_its_agents_out_of_the_index() {
        let mut realm = state();
        realm.hamlets.push(hamlet("mill"));
        realm.agents.push(agent("mill", 0, "miller"));
        let mut app = app(realm.clone());
        assert!(realm::mayor_of(app.world(), "mill").is_some());
        realm.hamlets.pop();
        realm.agents.pop();
        realm.revision += 1;
        receive(
            app.world_mut(),
            protocol::ServerMessage::State(realm),
        );
        assert_eq!(realm::hamlet(app.world(), "mill"), None);
        assert_eq!(realm::mayor_of(app.world(), "mill"), None);
        assert_eq!(realm::named(app.world(), "miller"), None);
        let miller = reference("mill", 0);
        assert_eq!(realm::agent(app.world(), &miller), None);
        assert!(realm::mayor(app.world()).is_some());
    }

    #[test]
    fn agents_of_one_name_in_two_hamlets_are_told_apart_by_hamlet() {
        let mut realm = state();
        realm.hamlets.push(hamlet("mill"));
        realm.agents.push(agent("home", 1, "scout"));
        realm.agents.push(agent("mill", 0, "scout"));
        let mut app = app(realm);
        let home = realm::agent(app.world(), &reference("home", 1));
        let mill = realm::agent(app.world(), &reference("mill", 0));
        assert_ne!(home, mill);
        assert_eq!(at(&app, "home", "scout"), home);
        assert_eq!(at(&app, "mill", "scout"), mill);
        assert_eq!(at(&app, "mill", "mayor"), mill);
        assert_eq!(realm::named(app.world(), "scout"), home);
        app.world_mut().entity_mut(home.unwrap()).insert(Left);
        assert_eq!(realm::named(app.world(), "scout"), mill);
        assert_eq!(at(&app, "home", "scout"), None);
    }
}
