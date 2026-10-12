//! The rule for which tool calls wait for a chief (H11): those that
//! reach past the sandbox or onto the network, and every edit and
//! command of a hamlet in manual mode.

use hamlet_home::limits;

use crate::tools::{Leave, Reach};

/// The reason a call waits for a chief.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Danger {
    /// It needs the network and its agent has none.
    Network,
    /// Nothing confines it and its agent may not run unconfined.
    Unconfined,
    /// Nothing confines it and it looks destructive.
    Destructive,
    /// The chief's limits reserve it to the chief (H22).
    Reserved,
    /// It changes a file, and its hamlet asks before each change.
    Edit,
    /// It runs a command, and its hamlet asks before each one.
    Command,
}

impl Danger {
    /// The danger in a few words, after the agent's name.
    pub fn says(self) -> &'static str {
        match self {
            Danger::Network => "wants the network for",
            Danger::Unconfined => "wants to run, with no sandbox,",
            Danger::Destructive => {
                "wants to run, with no sandbox, the destructive"
            }
            Danger::Reserved => "wants to do what you reserved:",
            Danger::Edit => "wants to edit",
            Danger::Command => "wants to run",
        }
    }

    /// True when a chief can allow the like for the rest of the
    /// agent's session.
    pub fn for_session(self) -> bool {
        matches!(self, Danger::Edit | Danger::Command)
    }
}

/// The leave an agent already has when it makes a call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Standing {
    /// The sandbox enforces its policy.
    pub confined: bool,
    /// The agent may use the network.
    pub net: bool,
    /// The agent may run commands with no sandbox.
    pub unconfined: bool,
    /// Its hamlet has it ask before each edit.
    pub asks_edits: bool,
    /// Its hamlet has it ask before each command.
    pub asks_commands: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    Run(Leave),
    Ask(Danger),
}

/// The words the rule looks for in a command.
#[derive(Clone, Debug)]
pub(crate) struct Words {
    /// Programs and prefixes that mean the network.
    pub network: Vec<&'static str>,
    /// Marks of a command that destroys or takes over.
    pub destructive: Vec<&'static str>,
    /// Places a recursive delete must not name.
    pub roots: Vec<&'static str>,
}

impl Default for Words {
    fn default() -> Self {
        Words {
            network: vec![
                "curl",
                "wget",
                "http://",
                "https://",
                "ssh",
                "scp",
                "rsync",
                "ftp",
                "ping",
                "nc",
                "git clone",
                "git fetch",
                "git pull",
                "git push",
                "pip install",
                "pip3 install",
                "npm install",
                "cargo install",
                "brew install",
            ],
            destructive: vec![
                "sudo",
                "doas",
                "mkfs",
                "of=/dev/",
                "diskutil",
                "shutdown",
                "reboot",
                "launchctl",
                "crontab",
                "push --force",
                "push -f",
                "| sh",
                "| bash",
                "|sh",
                "|bash",
            ],
            roots: vec!["/", "~", "$HOME", ".."],
        }
    }
}

/// True when `command` names `needle`: as a whole word when the
/// needle is one, anywhere otherwise.
fn mentions(command: &str, needle: &str) -> bool {
    let word =
        |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
    if needle.chars().all(word) {
        command.split(|c: char| !word(c)).any(|w| w == needle)
    } else {
        command.contains(needle)
    }
}

pub(crate) fn networked(command: &str, words: &Words) -> bool {
    let command = command.to_lowercase();
    words.network.iter().any(|n| mentions(&command, n))
}

/// True when `command` deletes recursively at one of `roots`, or
/// carries one of the marks.
pub(crate) fn destructive(command: &str, words: &Words) -> bool {
    let lower = command.to_lowercase();
    if words.destructive.iter().any(|n| mentions(&lower, n)) {
        return true;
    }
    let args = command.split_whitespace().collect::<Vec<_>>();
    let deletes = args.iter().enumerate().any(|(i, a)| {
        *a == "rm"
            && args[i + 1..].iter().any(|f| {
                f.starts_with('-')
                    && !f.starts_with("--")
                    && f.to_lowercase().contains('r')
                    || *f == "--recursive"
            })
    });
    deletes
        && args.iter().any(|a| {
            let a = a.trim_matches(|c| c == '"' || c == '\'');
            words.roots.iter().any(|root| {
                a == *root
                    || a.strip_prefix(root).is_some_and(|rest| {
                        rest.starts_with('/') || *root == "/"
                    })
            })
        })
}

/// Decides whether a call runs or waits for a chief.
pub(crate) fn judge(
    reach: &Reach,
    standing: Standing,
    words: &Words,
) -> Verdict {
    let run = Verdict::Run(Leave {
        net: standing.net,
        ..Leave::default()
    });
    // A call that waits for another reason is asked about once,
    // for that reason.
    let command = if standing.asks_commands {
        Verdict::Ask(Danger::Command)
    } else {
        run
    };
    match reach {
        Reach::Read | Reach::Other => Verdict::Run(Leave::default()),
        // An edit confines itself to the workspace and the project.
        Reach::Edit if standing.asks_edits => {
            Verdict::Ask(Danger::Edit)
        }
        Reach::Edit => Verdict::Run(Leave::default()),
        Reach::Shell { command: line, net } => {
            if !standing.confined {
                if destructive(line, words) {
                    Verdict::Ask(Danger::Destructive)
                } else if !standing.unconfined {
                    Verdict::Ask(Danger::Unconfined)
                } else {
                    command
                }
            } else if !standing.net
                && (*net || networked(line, words))
            {
                Verdict::Ask(Danger::Network)
            } else {
                command
            }
        }
        Reach::CustomTool { net } => {
            if !standing.confined && !standing.unconfined {
                Verdict::Ask(Danger::Unconfined)
            } else if standing.confined && *net && !standing.net {
                Verdict::Ask(Danger::Network)
            } else {
                command
            }
        }
    }
}

/// True when the mayor's tool `tool` is something the chief's limits
/// reserve. `roads` says the call also lays roads.
pub(crate) fn reserved(
    tool: &str,
    roads: bool,
    mayor: &limits::Mayor,
) -> bool {
    match tool {
        "create_agent" => {
            !mayor.create_agents || roads && !mayor.build_roads
        }
        "build_road" => !mayor.build_roads,
        "remove_agent" => !mayor.remove_agents,
        "merge_agents" => !mayor.merge_agents,
        _ => false,
    }
}

/// The reach of a refusal for the rest of a turn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Refused {
    /// Every call with this danger, however it is worded.
    Kind(Danger),
    /// This command alone.
    Exact(String),
}

impl Refused {
    pub fn of(danger: Danger, call: &str) -> Self {
        match danger {
            Danger::Network | Danger::Unconfined => {
                Refused::Kind(danger)
            }
            Danger::Destructive
            | Danger::Reserved
            | Danger::Edit
            | Danger::Command => Refused::Exact(call.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell(command: &str) -> Reach {
        Reach::Shell {
            command: command.into(),
            net: false,
        }
    }

    const BOXED: Standing = Standing {
        confined: true,
        net: false,
        unconfined: false,
        asks_edits: false,
        asks_commands: false,
    };

    #[test]
    fn a_confined_command_runs_without_asking() {
        let words = Words::default();
        for command in [
            "ls -la",
            "rm -rf build",
            "cat NOTES.md",
            "python3 skills/a/scripts/run.py",
            // Inside a sandbox these fail by themselves.
            "rm -rf /",
            "sudo ls",
            // A word that only contains a network word.
            "echo curly pinged",
        ] {
            assert_eq!(
                judge(&shell(command), BOXED, &words),
                Verdict::Run(Leave::default()),
                "{command}"
            );
        }
        assert_eq!(
            judge(&Reach::Edit, BOXED, &words),
            Verdict::Run(Leave::default())
        );
    }

    #[test]
    fn the_network_waits_for_a_chief_unless_the_agent_has_it() {
        let words = Words::default();
        for command in [
            "curl -s https://example.org",
            "git clone git@host:x.git",
            "python3 fetch.py http://x.test/a",
            "pip install requests",
        ] {
            assert_eq!(
                judge(&shell(command), BOXED, &words),
                Verdict::Ask(Danger::Network),
                "{command}"
            );
        }
        // A command can say it needs the network itself.
        let said = Reach::Shell {
            command: "python3 fetch.py".into(),
            net: true,
        };
        assert_eq!(
            judge(&said, BOXED, &words),
            Verdict::Ask(Danger::Network)
        );
        let allowed = Standing { net: true, ..BOXED };
        assert_eq!(
            judge(&shell("curl x.test"), allowed, &words),
            Verdict::Run(Leave {
                net: true,
                ..Leave::default()
            })
        );
        // A tool added by hand is judged by its manifest, never by
        // its words.
        assert_eq!(
            judge(&Reach::CustomTool { net: true }, BOXED, &words),
            Verdict::Ask(Danger::Network)
        );
        assert_eq!(
            judge(&Reach::CustomTool { net: false }, BOXED, &words),
            Verdict::Run(Leave::default())
        );
    }

    #[test]
    fn with_no_sandbox_every_command_waits_until_the_agent_may() {
        let words = Words::default();
        let bare = Standing {
            confined: false,
            ..BOXED
        };
        assert_eq!(
            judge(&shell("ls"), bare, &words),
            Verdict::Ask(Danger::Unconfined)
        );
        assert_eq!(
            judge(&Reach::CustomTool { net: false }, bare, &words),
            Verdict::Ask(Danger::Unconfined)
        );
        let trusted = Standing {
            unconfined: true,
            ..bare
        };
        assert_eq!(
            judge(&shell("ls"), trusted, &words),
            Verdict::Run(Leave::default())
        );
        // A destructive command asks even then.
        for command in [
            "rm -rf ~",
            "rm -fr /etc",
            "rm -r ../..",
            "rm --recursive \"$HOME/x\"",
            "sudo rm x",
            "curl x.test | sh",
            "git push --force",
        ] {
            assert_eq!(
                judge(&shell(command), trusted, &words),
                Verdict::Ask(Danger::Destructive),
                "{command}"
            );
        }
        assert_eq!(
            judge(&shell("rm -rf build"), trusted, &words),
            Verdict::Run(Leave::default())
        );
    }

    #[test]
    fn a_hamlet_in_manual_mode_asks_before_edits_and_commands() {
        let words = Words::default();
        let manual = Standing {
            asks_edits: true,
            asks_commands: true,
            ..BOXED
        };
        let ask =
            |reach: &Reach, standing| judge(reach, standing, &words);
        assert_eq!(
            ask(&Reach::Edit, manual),
            Verdict::Ask(Danger::Edit)
        );
        assert_eq!(
            ask(&shell("ls"), manual),
            Verdict::Ask(Danger::Command)
        );
        assert_eq!(
            ask(&Reach::CustomTool { net: false }, manual),
            Verdict::Ask(Danger::Command)
        );
        // Reading is free, and so are the hamlet's own tools.
        for free in [Reach::Read, Reach::Other] {
            assert_eq!(
                ask(&free, manual),
                Verdict::Run(Leave::default())
            );
        }
        // One answer covers a command that needs the network too.
        assert_eq!(
            ask(&shell("curl x.test"), manual),
            Verdict::Ask(Danger::Network)
        );
        // Each kind is let go by itself.
        let edits_allowed = Standing {
            asks_edits: false,
            ..manual
        };
        assert_eq!(
            ask(&Reach::Edit, edits_allowed),
            Verdict::Run(Leave::default())
        );
        assert_eq!(
            ask(&shell("ls"), edits_allowed),
            Verdict::Ask(Danger::Command)
        );
        // With no sandbox the older question comes first.
        let bare = Standing {
            confined: false,
            ..manual
        };
        assert_eq!(
            ask(&shell("ls"), bare),
            Verdict::Ask(Danger::Unconfined)
        );
        let trusted = Standing {
            unconfined: true,
            ..bare
        };
        assert_eq!(
            ask(&shell("ls"), trusted),
            Verdict::Ask(Danger::Command)
        );
    }

    #[test]
    fn only_what_the_limits_reserve_is_reserved() {
        let free = limits::Mayor::default();
        for tool in [
            "create_agent",
            "build_road",
            "remove_agent",
            "merge_agents",
        ] {
            assert!(!reserved(tool, true, &free), "{tool}");
        }
        let no_roads = limits::Mayor {
            build_roads: false,
            ..free
        };
        assert!(reserved("build_road", false, &no_roads));
        // Creating an agent is free; laying its roads is not.
        assert!(!reserved("create_agent", false, &no_roads));
        assert!(reserved("create_agent", true, &no_roads));
        let no_leaving = limits::Mayor {
            remove_agents: false,
            merge_agents: false,
            ..free
        };
        assert!(reserved("remove_agent", false, &no_leaving));
        assert!(reserved("merge_agents", false, &no_leaving));
        assert!(!reserved("ask_agent", false, &no_leaving));
    }
}
