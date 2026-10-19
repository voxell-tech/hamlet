//! What a plugin may be allowed to do, a capability at a time: the
//! names, what a manifest asks for, the chief's choice for each and
//! the verdict the host functions check.

mod ask;
mod choice;
mod decide;
mod dependency;
mod gate;
mod granted;
mod install;
mod name;
mod petition;
mod record;
mod store;
mod subject;
mod wanted;

use bevy::app::{App, Startup};

pub use self::ask::{Asked, Asks, Scope};
pub use self::choice::Choice;
pub use self::decide::{Decide, Forget};
pub use self::gate::allowed;
pub use self::granted::{Choices, Granted, Verdict};
pub use self::install::{
    AskToInstall, AskToRemove, Installed, Removed, Uninstalled,
};
pub use self::name::{Capability, NATIVE_ONLY, NEVER, Refusal};
pub use self::petition::{
    Answer, Answered, AskForMore, Petition, Replied,
};
pub use self::record::{Installation, Record, Standing, standing};
pub use self::store::{Store, open_store};
pub use self::subject::Subject;
pub use self::wanted::Wanted;

/// Keeps each plugin's [`Granted`] in step with its [`Asks`] and
/// [`Choices`], applies the chief's [`Decide`] and carries out the
/// chief's answers to a plugin's petitions.
pub fn plugin(app: &mut App) {
    app.add_systems(Startup, store::open_store)
        .add_observer(granted::rebuild)
        .add_observer(decide::decide)
        .add_observer(decide::forget)
        .add_observer(install::ask)
        .add_observer(install::ask_to_remove)
        .add_observer(petition::more)
        .add_observer(petition::answered)
        .add_observer(wanted::wanted);
}

#[cfg(test)]
mod tests {
    use bevy::app::App;

    use super::*;
    use crate::plugin::PluginInfo;

    #[test]
    fn a_capability_that_can_never_be_granted_is_refused() {
        for never in NEVER {
            assert_eq!(
                Capability::parse(never),
                Err(Refusal::Never(never.into()))
            );
        }
        assert_eq!(
            Capability::parse("render.compute"),
            Err(Refusal::NativeOnly("render.compute".into()))
        );
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
    fn an_update_that_changes_a_scope_is_asked_again() {
        let net = |hosts: &[&str]| {
            let hosts = hosts.iter().map(|h| h.to_string());
            Asks(vec![
                Asked::parse(
                    "net",
                    "a test",
                    &hosts.collect::<Vec<_>>(),
                )
                .unwrap(),
            ])
        };
        let choices = Choices::allowing(&[Capability::Net]);
        let record = Record::installed("1.0.0", &choices)
            .scoped(&net(&["a.com"]));
        let fresh = |version, hosts: &[&str]| match standing(
            Some(&record),
            version,
            &net(hosts),
        ) {
            Standing::Installed { choices, fresh } => {
                (fresh, choices.choice(Capability::Net))
            }
            other => panic!("{other:?}"),
        };

        let (fresh_same, kept) = fresh("2.0.0", &["a.com"]);
        assert!(fresh_same.is_empty());
        assert_eq!(kept, Some(Choice::Allow));
        let (fresh_other, kept) = fresh("2.0.0", &["b.com"]);
        assert_eq!(fresh_other, [Capability::Net]);
        assert_eq!(kept, None);

        let mut old = record.clone();
        old.scopes.clear();
        let unknown = |version| {
            let Standing::Installed { fresh, .. } =
                standing(Some(&old), version, &net(&["a.com"]))
            else {
                panic!("installed");
            };
            fresh
        };
        // A record that never kept the scope cannot vouch for one.
        assert_eq!(unknown("1.0.0"), [Capability::Net]);
        assert_eq!(unknown("2.0.0"), [Capability::Net]);
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
    fn a_choice_made_in_settings_replaces_granted_at_once() {
        use Capability::{Net, ReadChat};

        let mut app = App::new();
        app.add_plugins(plugin);
        let info = PluginInfo {
            id: "p".into(),
            name: "P".into(),
            version: "1".into(),
            builtin: false,
            essential: false,
        };
        let plugin = app
            .world_mut()
            .spawn((info, asking(&[Net]), Choices::default()))
            .id();
        let mut choose = |capability, choice, this_run| {
            app.world_mut().trigger(Decide {
                plugin: "p".into(),
                capability,
                target: None,
                choice,
                this_run,
            });
            app.world_mut().flush();
            app.world().get::<Granted>(plugin).unwrap().verdict(Net)
        };

        assert_eq!(
            choose(Net, Choice::Allow, false),
            Some(Verdict::Allowed)
        );
        assert_eq!(
            choose(Net, Choice::Deny, false),
            Some(Verdict::Denied)
        );
        assert_eq!(
            choose(Net, Choice::AskNextTime, true),
            Some(Verdict::Allowed)
        );
        // Chosen again in Settings, it asks at the next use.
        assert_eq!(
            choose(Net, Choice::AskNextTime, false),
            Some(Verdict::Ask)
        );
        // A capability the manifest does not name cannot be chosen.
        assert_eq!(
            choose(ReadChat, Choice::Allow, false),
            Some(Verdict::Ask)
        );
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

    #[test]
    fn letting_a_plugin_in_leaves_the_acts_as_the_chief_to_be_asked()
    {
        use Capability::{ChatSend, CommandsRun, Net, ReadAgents};

        let all = [ReadAgents, Net, ChatSend, CommandsRun];
        let granted =
            Granted::of(&asking(&all), &Choices::allowing(&all));
        assert!(granted.holds(ReadAgents));
        assert!(granted.holds(Net));
        assert_eq!(granted.verdict(ChatSend), Some(Verdict::Ask));
        assert_eq!(granted.verdict(CommandsRun), Some(Verdict::Ask));
    }

    #[test]
    fn a_scoped_capability_is_chosen_for_each_target_and_kept_in_the_record()
     {
        use Capability::PluginUse;

        let ask = |scope: &[String]| {
            Asks(vec![
                Asked::parse("plugin.use", "a test", scope).unwrap(),
            ])
        };
        let limited = ask(&["notes".into(), "tide".into()]);
        let choices = Choices::allowing(&[PluginUse])
            .decided_for(PluginUse, "notes", Choice::Allow, false)
            .decided_for(PluginUse, "tide", Choice::Deny, false);

        let granted = Granted::of(&limited, &choices);
        let verdict = |granted: &Granted, to| {
            granted.verdict_for(PluginUse, to)
        };
        assert_eq!(
            verdict(&granted, "notes"),
            Some(Verdict::Allowed)
        );
        assert_eq!(verdict(&granted, "tide"), Some(Verdict::Denied));
        assert_eq!(verdict(&granted, "other"), None);
        let open = Granted::of(&ask(&[]), &choices);
        assert_eq!(verdict(&open, "other"), Some(Verdict::Ask));

        let record =
            Record::installed("1.0.0", &choices).scoped(&limited);
        let Standing::Installed {
            choices: kept,
            fresh,
        } = standing(Some(&record), "1.0.0", &limited)
        else {
            panic!("installed");
        };
        assert!(fresh.is_empty());
        assert_eq!(kept, choices);
    }
}
