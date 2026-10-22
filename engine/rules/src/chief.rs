//! A chief's typed commands: to the realm here (H6, H13, H18, H19,
//! H20), to one hamlet in `decree`, and to the things that wait in
//! `rulings`.

pub mod asking;
mod decree;
pub mod rulings;

use anyhow::{Result, bail};

pub use crate::chief::decree::Decree;
use crate::commands::{self, Handler, OnRealm, Switch};
use crate::realm::{Origin, Realm};
use crate::vote;

/// The one a chief asks to talk to next.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// The mayor of this hamlet.
    Mayor(String),
    Agent(String),
}

/// The one a `/hamlet <name>` or `/agent <name>` line asks to talk
/// to.
pub fn switch(text: &str) -> Option<Target> {
    let typed = commands::read(text)?;
    let Handler::Switch(to) = typed.command.by else {
        return None;
    };
    let name = typed.rest.to_string();
    match to {
        _ if name.is_empty() => None,
        Switch::Hamlet => Some(Target::Mayor(name)),
        Switch::Agent => Some(Target::Agent(name)),
    }
}

/// Runs `text` if it is a chief's command about the realm, as `user`
/// talking to the hamlet `hamlet`. `None` when it is not one.
pub fn command(
    realm: &Realm,
    hamlet: &str,
    user: &str,
    text: &str,
) -> Option<Result<String>> {
    let typed = commands::read(text)?;
    let Handler::Realm(command) = typed.command.by else {
        return None;
    };
    Some(carry_out(realm, hamlet, user, command, typed.rest))
}

/// Runs the command `command` about the realm, as `user` talking to
/// the hamlet `hamlet`. `rest` is what followed its name.
pub fn carry_out(
    realm: &Realm,
    hamlet: &str,
    user: &str,
    command: OnRealm,
    rest: &str,
) -> Result<String> {
    let mut words = rest.split_whitespace();
    let first = words.next().unwrap_or_default();
    let rest = &words.collect::<Vec<_>>().join(" ");
    let here = realm.hamlet(hamlet)?;
    let chief = || {
        if !realm.is_chief(hamlet, user) {
            bail!("only a chief of `{hamlet}` can do this");
        }
        Ok(())
    };
    let given = |what: &str| {
        if rest.is_empty() {
            format!("{what} by the chief {user}")
        } else {
            rest.to_string()
        }
    };
    match command {
        OnRealm::Help => Ok(commands::help()),
        OnRealm::Hamlets => Ok(hamlets(realm)),
        OnRealm::Votes => {
            chief()?;
            let votes = vote::votes(&here);
            if votes.is_empty() {
                return Ok(format!("No votes in `{hamlet}` yet."));
            }
            Ok(votes
                .iter()
                .map(|v| v.report(&realm.rules))
                .collect::<Vec<_>>()
                .join("\n"))
        }
        OnRealm::Overturn => {
            chief()?;
            let Ok(id) = first.parse::<u64>() else {
                bail!("which vote? /overturn <number> [reason]");
            };
            vote::overturn(
                &here,
                &realm.rules,
                id,
                user,
                &given("Overturned"),
            )
        }
        OnRealm::Mayor => {
            chief()?;
            if first.is_empty() {
                bail!("who? /mayor <agent> [reason]");
            }
            let old = here.set_mayor(
                first,
                &format!("chief:{user}"),
                &given("Replaced"),
            )?;
            Ok(format!(
                "`{first}` is now the mayor of `{hamlet}`; `{}` \
                 stays as an ordinary agent.",
                old.name
            ))
        }
        OnRealm::Found => {
            let (mayor, purpose) =
                rest.split_once(' ').unwrap_or((rest, ""));
            if first.is_empty() || mayor.is_empty() {
                bail!("/found <hamlet> <mayor> <purpose>");
            }
            realm.found(
                first,
                mayor,
                purpose,
                Origin::Human {
                    user: user.to_string(),
                },
            )?;
            Ok(format!(
                "The hamlet `{first}` is founded under the mayor \
                 `{mayor}`, and it is yours. /hamlet {first} talks \
                 to it."
            ))
        }
        OnRealm::RemoveHamlet => {
            if first.is_empty() {
                bail!(
                    "which hamlet? /remove-hamlet <hamlet> [reason]"
                );
            }
            let archive =
                realm.remove(first, user, &given("Removed"))?;
            Ok(format!(
                "The hamlet `{first}` is removed. Every house is \
                 archived in {}. /restore-hamlet {first} brings it \
                 back.",
                archive.display()
            ))
        }
        OnRealm::NameHamlet => {
            if first.is_empty() {
                bail!(
                    "which hamlet? /name-hamlet <hamlet> <new name>"
                );
            }
            realm.name_hamlet(first, user, rest)?;
            Ok(match rest.trim() {
                "" => format!(
                    "The hamlet `{first}` goes by its own name \
                     again."
                ),
                title => format!(
                    "The hamlet `{first}` is now called {title}. \
                     Commands still know it as `{first}`."
                ),
            })
        }
        OnRealm::MoveHamlet => {
            let mut numbers =
                rest.split_whitespace().map(str::parse::<f32>);
            let (Some(Ok(x)), Some(Ok(z)), None) =
                (numbers.next(), numbers.next(), numbers.next())
            else {
                bail!("where to? /move-hamlet <hamlet> <x> <z>");
            };
            realm.move_hamlet(first, user, (x, z))?;
            Ok(format!(
                "The town of `{first}` is set down at {x:.0}, \
                 {z:.0}, if the ground there can take it."
            ))
        }
        OnRealm::RestoreHamlet => {
            realm.restore(first, user)?;
            Ok(format!("The hamlet `{first}` is back as it was."))
        }
        OnRealm::EraseHamlet => {
            if first.is_empty() {
                bail!("which hamlet? /erase-hamlet <hamlet>");
            }
            realm.erase(first, user)?;
            Ok(format!(
                "What was archived of the hamlet `{first}` is \
                 deleted. It cannot be restored."
            ))
        }
        OnRealm::Unshelve => {
            if first.is_empty() {
                bail!("which skill? /unshelve <skill> [reason]");
            }
            crate::library::Library::of(realm.home()).remove(
                first,
                &format!("chief:{user}"),
                &given("Taken out"),
            )?;
            Ok(format!(
                "The skill `{first}` is out of the library. No \
                 agent has it now."
            ))
        }
    }
}

fn hamlets(realm: &Realm) -> String {
    let mut out = String::new();
    for place in realm.hamlets() {
        let Ok(hamlet) = realm.hamlet(&place.name) else {
            continue;
        };
        let agents = hamlet
            .residents()
            .into_iter()
            .map(|r| r.name)
            .collect::<Vec<_>>();
        out.push_str(&format!(
            "- {}: mayor `{}`; agents: {}; chiefs: {}{}\n",
            place.name,
            hamlet.mayor().name,
            if agents.is_empty() {
                "(none)".to_string()
            } else {
                agents.join(", ")
            },
            if place.chiefs.is_empty() {
                "you".to_string()
            } else {
                place.chiefs.join(", ")
            },
            match &place.from {
                Some(from) => {
                    format!("; founded by a vote of `{from}`")
                }
                None => String::new(),
            }
        ));
        let removal = realm.removal(&place.name);
        if removal.ready {
            out.push_str(&format!(
                "  Its removal has been asked for{}. /remove-hamlet \
                 {} confirms it.\n",
                if removal.own { " by its own mayor" } else { "" },
                place.name
            ));
        }
    }
    let across = realm
        .roads()
        .into_iter()
        .filter(|r| {
            !r.ends.0.contains('@') || !r.ends.1.contains('@')
        })
        .map(|r| format!("{} - {}", r.ends.0, r.ends.1))
        .collect::<Vec<_>>();
    if !across.is_empty() {
        out.push_str(&format!(
            "Roads between hamlets, besides the mayors' own: {}\n",
            across.join("; ")
        ));
    }
    let gone = realm.archived();
    if !gone.is_empty() {
        out.push_str(&format!(
            "Removed: {}\n",
            gone.into_iter()
                .map(|(name, _)| name)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use hamlet_testing::Scratch;

    use super::*;
    use crate::hamlet::{FOUNDER, HOME};

    fn ok(realm: &Realm, hamlet: &str, text: &str) -> String {
        command(realm, hamlet, "alice", text).unwrap().unwrap()
    }

    #[test]
    fn a_chief_replaces_a_mayor_and_founds_and_removes_a_hamlet() {
        let dir = Scratch::new("chief-commands");
        let realm = Realm::open(&dir).unwrap();
        let home = realm.hamlet(HOME).unwrap();
        home.create("scout", "Looks.", "t").unwrap();
        assert!(command(&realm, HOME, "alice", "hello").is_none());
        assert!(command(&realm, HOME, "alice", "/new").is_none());

        ok(&realm, HOME, "/mayor scout it knows the way");
        assert_eq!(home.mayor().name, "scout");
        assert!(home.residents().iter().any(|r| r.name == FOUNDER));
        assert!(
            command(&realm, HOME, "alice", "/mayor nobody")
                .unwrap()
                .is_err()
        );

        ok(&realm, HOME, "/found mill miller Grinds the grain.");
        let mill = realm.hamlet("mill").unwrap();
        assert_eq!(mill.mayor().name, "miller");
        // A hamlet a human founds is that human's (C7).
        assert_eq!(realm.chiefs("mill"), ["alice"]);
        let bob = command(&realm, "mill", "bob", "/mayor miller");
        assert!(bob.unwrap().is_err());
        let list = ok(&realm, HOME, "/hamlets");
        assert!(list.contains("- mill: mayor `miller`"));
        assert!(list.contains("- home: mayor `scout`"));

        // An agent is removed and restored by other commands.
        assert!(
            command(&realm, HOME, "alice", "/remove x").is_none()
        );
        let bob = command(&realm, HOME, "bob", "/remove-hamlet mill");
        assert!(bob.unwrap().is_err());
        ok(&realm, HOME, "/remove-hamlet mill");
        assert!(realm.hamlet("mill").is_err());
        ok(&realm, HOME, "/restore-hamlet mill");
        // One that stands is not deleted. Removed, it is, with
        // its folder, and then it cannot come back.
        let stands =
            command(&realm, HOME, "alice", "/erase-hamlet mill");
        assert!(stands.unwrap().is_err());
        ok(&realm, HOME, "/remove-hamlet mill");
        let (_, archive) = realm.archived().pop().unwrap();
        assert!(archive.is_dir());
        ok(&realm, HOME, "/erase-hamlet mill");
        assert!(!archive.exists() && realm.archived().is_empty());
        let back =
            command(&realm, HOME, "alice", "/restore-hamlet mill");
        assert!(back.unwrap().is_err());
        ok(&realm, HOME, "/found mill miller Grinds the grain.");
        // A chief takes a skill out of the library, once.
        let library =
            crate::library::Library::open(realm.home()).unwrap();
        assert!(library.has("weather"));
        ok(&realm, HOME, "/unshelve weather it is wrong");
        assert!(!library.has("weather"));
        let gone =
            command(&realm, HOME, "alice", "/unshelve weather");
        assert!(gone.unwrap().is_err());
        // Any hamlet can be called something else, the first too.
        ok(&realm, HOME, "/name-hamlet home Pear Tree Hollow");
        let places = realm.hamlets();
        assert_eq!(places[0].shown(), "Pear Tree Hollow");
        assert_eq!(places[0].name, HOME);
        ok(&realm, HOME, "/name-hamlet home");
        assert_eq!(realm.hamlets()[0].shown(), HOME);
        // A chief sets a town down somewhere else, the first too,
        // and only a chief of it.
        assert_eq!(realm.hamlets()[1].at, None);
        ok(&realm, HOME, "/move-hamlet mill 120.5 -64");
        ok(&realm, HOME, "/move-hamlet home -300 12");
        ok(&realm, HOME, "/move-hamlet mill 400 80");
        let places = realm.hamlets();
        assert_eq!(places[0].at, Some((-300.0, 12.0)));
        assert_eq!(places[1].at, Some((400.0, 80.0)));
        for bad in [
            "/move-hamlet mill",
            "/move-hamlet mill 4",
            "/move-hamlet mill here there",
            "/move-hamlet mill 1 2 3",
            "/move-hamlet nowhere 1 2",
            "/move-hamlet mill inf 2",
        ] {
            let said = command(&realm, HOME, "alice", bad);
            assert!(said.unwrap().is_err(), "{bad}");
        }
        let bob =
            command(&realm, "mill", "bob", "/move-hamlet mill 1 2");
        assert!(bob.unwrap().is_err());
        assert_eq!(realm.hamlets()[1].at, Some((400.0, 80.0)));
        // It stands there still after it was removed and restored.
        ok(&realm, HOME, "/remove-hamlet mill");
        ok(&realm, HOME, "/restore-hamlet mill");
        let back = realm.hamlets();
        let mill = back.iter().find(|p| p.name == "mill").unwrap();
        assert_eq!(mill.at, Some((400.0, 80.0)));
        assert_eq!(
            realm.hamlet("mill").unwrap().mayor().name,
            "miller"
        );
        assert_eq!(
            switch("/hamlet mill"),
            Some(Target::Mayor("mill".into()))
        );
        assert_eq!(
            switch("/agent scout"),
            Some(Target::Agent("scout".into()))
        );
        assert_eq!(switch("/hamlets"), None);
    }
}
