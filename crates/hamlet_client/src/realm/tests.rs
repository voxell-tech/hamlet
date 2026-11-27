use bevy_app::App;
use hamlet_protocol::client as protocol;
use hamlet_testing::realm::{agent, road, state};

use super::{named, road_between, roads_of, shown_of};
use crate::{ClientPlugin, receive};

#[test]
fn a_hire_is_shown_by_its_nickname_else_its_profiles_name() {
    let mut app = App::new();
    app.add_plugins(ClientPlugin { server: None });
    let mut realm = state();
    let world = app.world_mut();
    receive(world, protocol::ServerMessage::State(realm.clone()));
    let ada = named(world, "hire#2").unwrap();
    assert_eq!(shown_of(world, ada), "ada");
    realm.revision = 1;
    realm.profiles[0].name = "Ada Lovelace".into();
    receive(world, protocol::ServerMessage::State(realm.clone()));
    assert_eq!(shown_of(world, ada), "Ada Lovelace");
    realm.revision = 2;
    realm.agents[1].nickname = Some("Countess".into());
    receive(world, protocol::ServerMessage::State(realm));
    assert_eq!(shown_of(world, ada), "Countess");
    assert_eq!(named(world, "hire#2"), Some(ada));
}

#[test]
fn a_road_goes_with_an_end_that_left_or_is_gone_and_is_not_laid_again()
 {
    let mut app = App::new();
    app.add_plugins(ClientPlugin { server: None });
    let mut realm = state();
    realm.agents.push(agent(3, "home", false));
    realm.agents.push(agent(4, "home", false));
    let ada = protocol::AgentId(2);
    realm.hamlets[0].roads = vec![
        road(ada, protocol::AgentId(3), 0),
        road(ada, protocol::AgentId(4), 0),
    ];
    let world = app.world_mut();
    receive(world, protocol::ServerMessage::State(realm.clone()));
    let [ada, scout, miner] = ["hire#2", "hire#3", "hire#4"]
        .map(|name| named(world, name).unwrap());
    assert_eq!(roads_of(world, ada).count(), 2);
    realm.agents[2].left = Some(protocol::Left {
        at: 1,
        by: "chief".into(),
        how: protocol::LeftHow::Ended,
        into: None,
        reason: String::new(),
        words: String::new(),
    });
    realm.revision += 1;
    receive(world, protocol::ServerMessage::State(realm.clone()));
    assert!(world.get_entity(scout).is_ok());
    assert_eq!(road_between(world, ada, scout), None);
    let mine = road_between(world, ada, miner).unwrap();
    realm.agents.pop();
    realm.revision += 1;
    receive(world, protocol::ServerMessage::State(realm));
    assert!(world.get_entity(mine).is_err());
    assert_eq!(roads_of(world, ada).count(), 0);
}
