//! Writing `limits.toml`: the commented file a home starts with, and
//! one value set in place.

use std::path::Path;

use anyhow::{Context, Result};

use super::{Limits, path};

/// The file a home with no limits is given, so a chief can find it.
const TEMPLATE: &str = include_str!("template.toml");

/// True when `line` sets `key`: it is not a comment, and opens with
/// the key and an equals sign.
fn sets(line: &str, key: &str) -> bool {
    line.trim_start()
        .strip_prefix(key)
        .is_some_and(|rest| rest.trim_start().starts_with('='))
}
/// Sets `key` in `section` of the limits of `home` to `value`, or
/// takes it out so that its default holds. An empty `section` is the
/// part before the first section. A change that would leave an
/// unreadable file is refused.
pub fn set(
    home: &Path,
    section: &str,
    key: &str,
    value: Option<&str>,
) -> Result<()> {
    write_template(home)?;
    let path = path(home);
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("reading {}", path.display()))?;
    let mut lines =
        text.lines().map(str::to_string).collect::<Vec<_>>();
    let opens = |line: &str| line.trim_start().starts_with('[');
    // The lines the section's own lines begin and end at.
    let begins = match section {
        "" => Some(0),
        name => {
            let header = format!("[{name}]");
            lines
                .iter()
                .position(|line| line.trim() == header)
                .map(|at| at + 1)
        }
    };
    let written = value.map(|value| format!("{key} = {value}"));
    match begins {
        Some(begins) => {
            let ends = lines[begins..]
                .iter()
                .position(|line| opens(line))
                .map_or(lines.len(), |at| begins + at);
            let found =
                (begins..ends).find(|&at| sets(&lines[at], key));
            match (found, written) {
                (Some(at), Some(line)) => lines[at] = line,
                (Some(at), None) => {
                    lines.remove(at);
                }
                (None, Some(line)) => {
                    // After the last value of the section: the
                    // comments under it are the next section's.
                    let after = (begins..ends)
                        .rev()
                        .find(|&at| {
                            let line = lines[at].trim();
                            !line.is_empty() && !line.starts_with('#')
                        })
                        .map_or(begins, |at| at + 1);
                    lines.insert(after, line);
                }
                (None, None) => {}
            }
        }
        None => {
            if let Some(line) = written {
                lines.extend([
                    String::new(),
                    format!("[{section}]"),
                    line,
                ]);
            }
        }
    }
    let mut now = lines.join("\n");
    now.push('\n');
    Limits::read(&now, &path)?;
    std::fs::write(&path, now)
        .with_context(|| format!("writing {}", path.display()))
}

/// Writes the commented default file if the home has none.
pub fn write_template(home: &Path) -> Result<()> {
    let path = path(home);
    if !path.exists() {
        std::fs::write(&path, TEMPLATE)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use hamlet_testing::Scratch;

    use super::super::Task;
    use super::*;

    #[test]
    fn a_missing_file_is_the_defaults_and_so_is_the_template() {
        let home = Scratch::new("limits-template");
        assert_eq!(Limits::load(&home).unwrap(), Limits::default());
        write_template(&home).unwrap();
        assert_eq!(Limits::load(&home).unwrap(), Limits::default());
    }

    #[test]
    fn one_value_is_set_and_the_rest_of_the_file_is_left_alone() {
        let home = Scratch::new("limits-set");
        // A home with no file is given the commented one first.
        set(&home, "votes", "pass", Some("[1, 2]")).unwrap();
        set(&home, "budget", "minutes", Some("90")).unwrap();
        set(&home, "", "most_agents", Some("4")).unwrap();
        set(&home, "budget", "cost", None).unwrap();
        let limits = Limits::load(&home).unwrap();
        assert_eq!(limits.votes.pass, (1, 2));
        assert_eq!(limits.budget.minutes, Some(90));
        assert_eq!(limits.budget.cost, None);
        assert_eq!(limits.most_agents, 4);
        // The rest is as it was, and so are the comments.
        assert_eq!(limits.task, Task::default());
        let text = std::fs::read_to_string(path(&home)).unwrap();
        assert!(text.contains("# The hamlet's budget, in money"));
        assert!(text.contains("# minutes = 120"));
        assert_eq!(text.matches("minutes = 90").count(), 1);

        // A file that cannot be read is refused, and stays.
        assert!(set(&home, "votes", "pass", Some("[4, 3]")).is_err());
        assert!(
            set(&home, "votes", "sealed", Some("maybe")).is_err()
        );
        assert_eq!(
            std::fs::read_to_string(path(&home)).unwrap(),
            text
        );

        // A section the file does not have is added at its end.
        let bare = Scratch::new("limits-set-bare");
        std::fs::write(path(&bare), "most_agents = 3\n").unwrap();
        set(&bare, "hamlets", "most", Some("2")).unwrap();
        let limits = Limits::load(&bare).unwrap();
        assert_eq!((limits.most_agents, limits.hamlets.most), (3, 2));
    }
}
