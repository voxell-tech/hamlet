use hamlet_actor::{Actor, ActorConfig, Registry};

/// The kind `kind`, an actor of the trait API `A`, with a database.
pub fn kind<A: Actor>(registry: &mut Registry, kind: &str) {
    registry.register_actor_with::<A>(
        kind,
        ActorConfig {
            has_database: true,
            ..ActorConfig::default()
        },
    );
}
