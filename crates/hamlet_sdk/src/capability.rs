//! What a plugin may be allowed to do, a capability at a time: the
//! names, what a manifest asks for, the chief's choice for each and
//! the verdict the host functions check.

mod ask;
mod choice;
mod granted;
mod name;

use bevy::app::App;

pub use self::ask::{Asked, Asks, Scope};
pub use self::choice::Choice;
pub use self::granted::{Choices, Granted, Verdict};
pub use self::name::{Capability, NEVER, Refusal};

/// Keeps each plugin's [`Granted`] in step with its [`Asks`] and
/// [`Choices`].
pub fn plugin(app: &mut App) {
    app.add_observer(granted::rebuild);
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

    #[test]
    fn granted_follows_the_choices_for_what_was_asked() {
        use Capability::{Net, ReadAgents, ReadChat, ReadPapers};

        let mut app = App::new();
        app.add_plugins(plugin);
        let asks = Asks(
            [ReadAgents, ReadChat, ReadPapers, Net]
                .map(|capability| Asked {
                    capability,
                    reason: "a test".into(),
                    scope: Scope::default(),
                })
                .into(),
        );
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
