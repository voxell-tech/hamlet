//! The plugins a plugin needs: whether they are there and running,
//! who needs a plugin, and whether the needs go in a circle.

use std::fmt;

use super::{Need, Range, Status, Version};

/// A plugin as the needs of the others find it.
pub struct Present<'a> {
    pub id: &'a str,
    pub version: &'a str,
    /// `None` while it is not installed.
    pub status: Option<Status>,
}

/// Why a need is not met.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Unmet {
    /// No plugin of that id is here.
    Missing(String),
    NotInstalled(String),
    /// Installed, in a version outside the range.
    Version {
        id: String,
        range: String,
        have: String,
    },
    /// Installed and not running.
    Off(String, Status),
}

impl fmt::Display for Unmet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Unmet::Missing(id) => {
                write!(f, "needs `{id}`, which is not here")
            }
            Unmet::NotInstalled(id) => {
                write!(f, "needs `{id}`, which is not installed")
            }
            Unmet::Version { id, range, have } => write!(
                f,
                "needs `{id}` {range}, but {have} is installed"
            ),
            Unmet::Off(id, Status::Failed) => {
                write!(f, "needs `{id}`, which stopped")
            }
            Unmet::Off(id, _) => {
                write!(f, "needs `{id}`, which is switched off")
            }
        }
    }
}

fn check(need: &Need, present: &[Present]) -> Option<Unmet> {
    let id = need.id.clone();
    let Some(found) =
        present.iter().find(|found| found.id == need.id)
    else {
        return Some(Unmet::Missing(id));
    };
    let Some(status) = found.status else {
        return Some(Unmet::NotInstalled(id));
    };
    let fits = Range::parse(&need.range)
        .ok()
        .zip(Version::parse(found.version))
        .is_some_and(|(range, version)| range.holds(version));
    if !fits {
        return Some(Unmet::Version {
            id,
            range: need.range.clone(),
            have: found.version.into(),
        });
    }
    match status {
        Status::Disabled | Status::Failed => {
            Some(Unmet::Off(id, status))
        }
        Status::Starting | Status::Running => None,
    }
}

/// The first of `needs` that is not met.
pub fn unmet(needs: &[Need], present: &[Present]) -> Option<Unmet> {
    needs.iter().find_map(|need| check(need, present))
}

/// The needs that are missing, not installed or in the wrong
/// version, which installing the plugin does not settle on its own.
pub fn lacking(needs: &[Need], present: &[Present]) -> Vec<Unmet> {
    needs
        .iter()
        .filter_map(|need| check(need, present))
        .filter(|unmet| !matches!(unmet, Unmet::Off(..)))
        .collect()
}

/// An installed or waiting plugin, for working out which start.
pub struct Listed<'a> {
    pub id: &'a str,
    pub version: &'a str,
    pub needs: &'a [Need],
    pub installed: bool,
}

/// The installed plugins that cannot start, for what they need, given
/// that those that can are the ones that start.
pub fn blocked(listed: &[Listed]) -> Vec<(String, Unmet)> {
    let mut off = Vec::<(String, Unmet)>::new();
    loop {
        let present = listed
            .iter()
            .map(|plugin| Present {
                id: plugin.id,
                version: plugin.version,
                status: plugin.installed.then(|| {
                    match off.iter().any(|(id, _)| id == plugin.id) {
                        true => Status::Disabled,
                        false => Status::Running,
                    }
                }),
            })
            .collect::<Vec<_>>();
        let more = listed
            .iter()
            .filter(|plugin| plugin.installed)
            .filter(|plugin| {
                !off.iter().any(|(id, _)| id == plugin.id)
            })
            .find_map(|plugin| {
                Some((
                    plugin.id.to_string(),
                    unmet(plugin.needs, &present)?,
                ))
            });
        match more {
            Some(more) => off.push(more),
            None => return off,
        }
    }
}

/// The path of needs from `id` back to itself, if there is one.
pub fn cycle_through<'a>(
    id: &'a str,
    all: &[(&'a str, &[Need])],
) -> Option<Vec<String>> {
    fn walk<'a>(
        at: &'a str,
        goal: &str,
        all: &[(&'a str, &[Need])],
        path: &mut Vec<&'a str>,
    ) -> bool {
        let Some((_, needs)) = all.iter().find(|(id, _)| *id == at)
        else {
            return false;
        };
        path.push(at);
        for need in *needs {
            let next = all
                .iter()
                .map(|(id, _)| *id)
                .find(|id| *id == need.id);
            let Some(next) = next else {
                continue;
            };
            if next == goal
                || (!path.contains(&next)
                    && walk(next, goal, all, path))
            {
                return true;
            }
        }
        path.pop();
        false
    }
    let mut path = Vec::new();
    walk(id, id, all, &mut path).then(|| {
        let mut named =
            path.into_iter().map(String::from).collect::<Vec<_>>();
        named.push(id.to_string());
        named
    })
}

/// The plugins that need `id`, directly or through one another.
pub fn dependents<'a>(
    id: &str,
    all: &[(&'a str, &[Need])],
) -> Vec<&'a str> {
    let mut found = Vec::<&str>::new();
    let mut wanted = vec![id];
    while let Some(wanted_id) = wanted.pop() {
        for (plugin, needs) in all {
            if needs.iter().any(|need| need.id == wanted_id)
                && !found.contains(plugin)
                && *plugin != id
            {
                found.push(plugin);
                wanted.push(plugin);
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn need(id: &str, range: &str) -> Need {
        Need {
            id: id.into(),
            range: range.into(),
        }
    }

    #[test]
    fn a_plugin_starts_only_when_what_it_needs_does() {
        let notes = [need("base", "^0.1")];
        let clipper = [need("notes", ">=0.1")];
        let old = [need("base", ">=2")];
        let listed = |id, version, needs, installed| Listed {
            id,
            version,
            needs,
            installed,
        };
        let blocked = blocked(&[
            listed("base", "0.1.4", &[], true),
            listed("notes", "0.1.0", &notes, true),
            listed("clipper", "0.1.0", &clipper, true),
            listed("fussy", "0.1.0", &old, true),
            listed("later", "0.1.0", &clipper, true),
            listed("far", "0.1.0", &[need("gone", "*")], true),
        ]);
        let reasons = blocked
            .iter()
            .map(|(id, why)| format!("{id}: {why}"))
            .collect::<Vec<_>>();
        assert_eq!(
            reasons,
            [
                "fussy: needs `base` >=2, but 0.1.4 is installed",
                "far: needs `gone`, which is not here"
            ]
        );
    }

    #[test]
    fn a_circle_of_needs_is_found_from_each_plugin_in_it() {
        let a = [need("b", "*")];
        let b = [need("c", "*")];
        let c = [need("a", "*")];
        let d = [need("a", "*")];
        let all: [(&str, &[Need]); 4] =
            [("a", &a), ("b", &b), ("c", &c), ("d", &d)];

        assert_eq!(
            cycle_through("a", &all),
            Some(vec![
                "a".into(),
                "b".into(),
                "c".into(),
                "a".into()
            ])
        );
        assert_eq!(cycle_through("d", &all), None);
        assert_eq!(dependents("b", &all), ["a", "c", "d"]);
    }
}
