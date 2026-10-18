//! What a plugin may be allowed to do, a capability at a time: the
//! names, what a manifest asks for, the chief's choice for each and
//! the verdict the host functions check.

mod ask;
mod choice;
mod decide;
mod granted;
mod name;
mod record;
mod store;

use bevy::app::App;

pub use self::ask::{Asked, Asks, Scope};
pub use self::choice::Choice;
pub use self::decide::Decide;
pub use self::granted::{Choices, Granted, Verdict};
pub use self::name::{Capability, NEVER, Refusal};
pub use self::record::{Installation, Record, Standing, standing};
pub use self::store::Store;

/// Keeps each plugin's [`Granted`] in step with its [`Asks`] and
/// [`Choices`], and applies the chief's [`Decide`].
pub fn plugin(app: &mut App) {
    app.add_observer(granted::rebuild)
        .add_observer(decide::decide);
}

#[cfg(test)]
mod tests {
    use bevy::app::App;

    use super::*;

    #[test]
    fn a_capability_that_can_never_be_granted_is_refused() {
        for never in NEVER {
            assert_eq!(
                Capability::parse(never),
                Err(Refusal::Never(never.into()))
            );
        }
        assert!(matches!(
            Capability::parse("read.nothing"),
            Err(Refusal::Unknown(_))
        ));
        for capability in Capability::ALL {
            assert_eq!(
                Capability::parse(capability.name()),
                Ok(capability)
            );
        }
    }

    fn asking(capabilities: &[Capability]) -> Asks {
        Asks(
            capabilities
                .iter()
                .map(|&capability| Asked {
                    capability,
                    reason: "a test".into(),
                    scope: Scope::default(),
                })
                .collect(),
        )
    }

    #[test]
    fn an_update_that_asks_for_more_leaves_the_new_ask_unanswered() {
        use Capability::{ChatSend, Net, ReadAgents};

        let folder = std::env::temp_dir()
            .join(format!("hamlet_grants_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let mut store = Store::open(&folder);
        let choices = Choices::default()
            .decided(ReadAgents, Choice::Allow, false)
            .decided(Net, Choice::Deny, false);
        store.set("p", Record::installed("1.0.0", &choices));
        store.set("q", Record::declined("1.0.0", &asking(&[Net])));

        let store = Store::open(&folder);
        let wanted = asking(&[ReadAgents, ChatSend]);
        let Standing::Installed { choices, fresh } =
            standing(store.get("p"), "2.0.0", &wanted)
        else {
            panic!("p is installed");
        };
        assert_eq!(fresh, [ChatSend]);
        assert_eq!(choices.choice(ReadAgents), Some(Choice::Allow));
        assert_eq!(choices.choice(Net), None);
        assert_eq!(choices.choice(ChatSend), None);

        let asked = asking(&[Net]);
        assert_eq!(
            standing(store.get("q"), "1.0.0", &asked),
            Standing::Declined
        );
        assert_eq!(
            standing(store.get("q"), "1.0.1", &asked),
            Standing::Unknown
        );
        assert_eq!(
            standing(store.get("none"), "1.0.0", &asked),
            Standing::Unknown
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn granted_follows_the_choices_for_what_was_asked() {
        use Capability::{Net, ReadAgents, ReadChat, ReadPapers};

        let mut app = App::new();
        app.add_plugins(plugin);
        let asks = asking(&[ReadAgents, ReadChat, ReadPapers, Net]);
        let choices = Choices::default()
            .decided(ReadAgents, Choice::Allow, false)
            .decided(ReadChat, Choice::AskNextTime, true)
            .decided(ReadPapers, Choice::Deny, false);
        let plugin = app.world_mut().spawn((asks, choices)).id();

        let granted = app.world().get::<Granted>(plugin).unwrap();
        assert_eq!(
            granted.verdict(ReadAgents),
            Some(Verdict::Allowed)
        );
        assert_eq!(granted.verdict(ReadChat), Some(Verdict::Allowed));
        assert_eq!(
            granted.verdict(ReadPapers),
            Some(Verdict::Denied)
        );
        assert_eq!(granted.verdict(Net), Some(Verdict::Ask));
        assert_eq!(granted.verdict(Capability::ChatSend), None);

        let later = Choices::default().decided(
            ReadAgents,
            Choice::AskNextTime,
            false,
        );
        app.world_mut().entity_mut(plugin).insert(later);
        let granted = app.world().get::<Granted>(plugin).unwrap();
        assert!(!granted.holds(ReadAgents));
        assert_eq!(granted.verdict(ReadAgents), Some(Verdict::Ask));
    }
}
