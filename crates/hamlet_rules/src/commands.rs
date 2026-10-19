//! Every command a chief can type, named once: its words, what
//! follows it and who carries it out.

/// The one that carries a command out, and which of its commands it
/// is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Handler {
    /// The session it is typed in.
    Session(InSession),
    /// The one that shows the chat.
    Switch(Switch),
    /// The keeper of the things that wait for the chief.
    Oversight(Ruling),
    /// The hamlet the session is with, in `Hamlet::decree`.
    Hamlet(OnHamlet),
    /// The realm, in `chief::command`.
    Realm(OnRealm),
}

/// A command about the session it is typed in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InSession {
    New,
    Resume,
    Reflect,
    Retry,
    Budget,
    Vote,
}

/// A command that changes who the chief talks to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Switch {
    Agent,
    Hamlet,
}

/// A command about what waits for the chief and what agents are
/// allowed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ruling {
    Queue,
    Allow,
    Always,
    Deny,
    Go,
    Withdraw,
    Reply,
    Dismiss,
    Undo,
    Grants,
    Revoke,
    Stop,
    To,
}

/// A command about one hamlet.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OnHamlet {
    Record,
    Restore,
    Erase,
    Remove,
    Merge,
    Road,
    Unroad,
    Give,
    Folder,
    Mode,
    ResetBudget,
}

/// A command about the hamlets of the realm and their votes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OnRealm {
    Hamlets,
    Votes,
    Overturn,
    Mayor,
    Found,
    RemoveHamlet,
    RestoreHamlet,
    EraseHamlet,
    NameHamlet,
    MoveHamlet,
    Unshelve,
    /// The list of these commands.
    Help,
}

/// One command.
#[derive(Clone, Copy, Debug)]
pub struct Command {
    pub name: &'static str,
    /// The words after the name, as a chief reads them. Empty for
    /// none.
    pub args: &'static str,
    pub does: &'static str,
    pub by: Handler,
}

const fn command(
    name: &'static str,
    args: &'static str,
    does: &'static str,
    by: Handler,
) -> Command {
    Command {
        name,
        args,
        does,
        by,
    }
}

/// The command that goes back to a past session.
pub const RESUME: &str = "/resume";

/// The commands.
pub const ALL: &[Command] = &[
    command(
        "/new",
        "",
        "Start a new session and keep this one in history",
        Handler::Session(InSession::New),
    ),
    command(
        RESUME,
        "<number>",
        "Go back to a past session, by its number in History",
        Handler::Session(InSession::Resume),
    ),
    command(
        "/retry",
        "",
        "Carry on a turn that failed or was stopped, from where it \
         ended",
        Handler::Session(InSession::Retry),
    ),
    command(
        "/agent",
        "<name>",
        "Talk to that agent, in whichever hamlet",
        Handler::Switch(Switch::Agent),
    ),
    command(
        "/hamlet",
        "<name>",
        "Talk to that hamlet's mayor",
        Handler::Switch(Switch::Hamlet),
    ),
    command(
        "/to",
        "<agent> <instruction>",
        "Instruct an agent directly",
        Handler::Oversight(Ruling::To),
    ),
    command(
        "/stop",
        "[agent|all]",
        "Stop an agent's work, or everyone's",
        Handler::Oversight(Ruling::Stop),
    ),
    command(
        "/queue",
        "",
        "List what waits for you",
        Handler::Oversight(Ruling::Queue),
    ),
    command(
        "/allow",
        "[id] [reason]",
        "Allow what waits, this once",
        Handler::Oversight(Ruling::Allow),
    ),
    command(
        "/always",
        "[reason]",
        "Allow it, and the like from now on",
        Handler::Oversight(Ruling::Always),
    ),
    command(
        "/deny",
        "[id] [reason]",
        "Refuse what waits",
        Handler::Oversight(Ruling::Deny),
    ),
    command(
        "/go",
        "[reason]",
        "Tell an agent that objected to go ahead",
        Handler::Oversight(Ruling::Go),
    ),
    command(
        "/withdraw",
        "[reason]",
        "Take back what an agent objected to",
        Handler::Oversight(Ruling::Withdraw),
    ),
    command(
        "/reply",
        "<text>",
        "Answer an agent's question",
        Handler::Oversight(Ruling::Reply),
    ),
    command(
        "/dismiss",
        "",
        "Put away a fault or a lapsed request",
        Handler::Oversight(Ruling::Dismiss),
    ),
    command(
        "/undo",
        "",
        "Reverse the latest thing that can be reversed",
        Handler::Oversight(Ruling::Undo),
    ),
    command(
        "/grants",
        "",
        "List what agents are allowed for good",
        Handler::Oversight(Ruling::Grants),
    ),
    command(
        "/revoke",
        "<agent> <network|unconfined>",
        "Take back a lasting permission",
        Handler::Oversight(Ruling::Revoke),
    ),
    command(
        "/remove",
        "<agent> [reason]",
        "Remove an agent and archive its house",
        Handler::Hamlet(OnHamlet::Remove),
    ),
    command(
        "/merge",
        "<absorbed> into <kept> [reason]",
        "Merge one agent into another",
        Handler::Hamlet(OnHamlet::Merge),
    ),
    command(
        "/road",
        "<agent> <agent> [reason]",
        "Build a road between two agents",
        Handler::Hamlet(OnHamlet::Road),
    ),
    command(
        "/unroad",
        "<agent> <agent> [reason]",
        "Take up the road between two agents",
        Handler::Hamlet(OnHamlet::Unroad),
    ),
    command(
        "/give",
        "<agent> <skill> [from <agent>]",
        "Give an agent a starter skill, or another's",
        Handler::Hamlet(OnHamlet::Give),
    ),
    command(
        "/folder",
        "<folder|none>",
        "Point this hamlet at a folder to work on, or at none",
        Handler::Hamlet(OnHamlet::Folder),
    ),
    command(
        "/mode",
        "<manual|auto>",
        "Whether agents ask before they change the folder",
        Handler::Hamlet(OnHamlet::Mode),
    ),
    command(
        "/reset-budget",
        "",
        "Start this hamlet's budget again from nothing spent",
        Handler::Hamlet(OnHamlet::ResetBudget),
    ),
    command(
        "/restore",
        "<agent> [as <new-name>] [reason]",
        "Bring back a removed or merged agent",
        Handler::Hamlet(OnHamlet::Restore),
    ),
    command(
        "/erase",
        "<agent>",
        "Delete a removed agent for good, to free its room on disk",
        Handler::Hamlet(OnHamlet::Erase),
    ),
    command(
        "/record",
        "",
        "Who was created, removed, merged and restored",
        Handler::Hamlet(OnHamlet::Record),
    ),
    command(
        "/budget",
        "",
        "The hamlet's budget and each agent's spend",
        Handler::Session(InSession::Budget),
    ),
    command(
        "/reflect",
        "",
        "Have the agent reflect on this conversation now",
        Handler::Session(InSession::Reflect),
    ),
    command(
        "/mayor",
        "<agent> [reason]",
        "Make an agent of this hamlet its mayor",
        Handler::Realm(OnRealm::Mayor),
    ),
    command(
        "/vote",
        "<agent> [reason]",
        "Ask this hamlet's agents to vote on making that agent \
         their mayor",
        Handler::Session(InSession::Vote),
    ),
    command(
        "/votes",
        "",
        "This hamlet's votes, with every ballot",
        Handler::Realm(OnRealm::Votes),
    ),
    command(
        "/overturn",
        "<vote> [reason]",
        "Reverse a vote that passed, or stop an open one",
        Handler::Realm(OnRealm::Overturn),
    ),
    command(
        "/hamlets",
        "",
        "List the hamlets and their mayors",
        Handler::Realm(OnRealm::Hamlets),
    ),
    command(
        "/found",
        "<hamlet> <mayor> <purpose>",
        "Found a hamlet of your own under a new mayor",
        Handler::Realm(OnRealm::Found),
    ),
    command(
        "/remove-hamlet",
        "<hamlet> [reason]",
        "Remove a hamlet and archive its houses",
        Handler::Realm(OnRealm::RemoveHamlet),
    ),
    command(
        "/restore-hamlet",
        "<hamlet>",
        "Bring a removed hamlet back",
        Handler::Realm(OnRealm::RestoreHamlet),
    ),
    command(
        "/unshelve",
        "<skill> [reason]",
        "Take a skill out of the library",
        Handler::Realm(OnRealm::Unshelve),
    ),
    command(
        "/erase-hamlet",
        "<hamlet>",
        "Delete a removed hamlet for good",
        Handler::Realm(OnRealm::EraseHamlet),
    ),
    command(
        "/name-hamlet",
        "<hamlet> <new-name>",
        "Call a hamlet something else",
        Handler::Realm(OnRealm::NameHamlet),
    ),
    command(
        "/move-hamlet",
        "<hamlet> <x> <z>",
        "Set a hamlet's town down somewhere else",
        Handler::Realm(OnRealm::MoveHamlet),
    ),
    command(
        "/chief",
        "",
        "The commands about hamlets and votes",
        Handler::Realm(OnRealm::Help),
    ),
];

/// The width a command and what follows it are set in, in the list of
/// `/chief`.
const HELP_COLUMN: usize = 34;

impl Command {
    /// The command as a chief types it, with what follows.
    pub fn usage(&self) -> String {
        match self.args {
            "" => self.name.to_string(),
            args => format!("{} {args}", self.name),
        }
    }
}

/// A line read as a command.
#[derive(Clone, Copy, Debug)]
pub struct Typed<'a> {
    pub command: &'static Command,
    /// The words after the command's name.
    pub rest: &'a str,
}

/// The command `line` opens with. `None` for a line that opens with
/// none.
pub fn read(line: &str) -> Option<Typed<'_>> {
    let line = line.trim();
    let (name, rest) = line
        .split_once(char::is_whitespace)
        .map_or((line, ""), |(name, rest)| (name, rest.trim()));
    let command = ALL.iter().find(|c| c.name == name)?;
    Some(Typed { command, rest })
}

/// The commands `/chief` lists, in its order, each with the words
/// its line adds to what the command does.
const LISTED: &[(&str, &str)] = &[
    ("/hamlets", ""),
    ("/hamlet", ""),
    ("/agent", ""),
    ("/votes", ""),
    ("/vote", ""),
    ("/overturn", ""),
    ("/mayor", ""),
    ("/found", ""),
    ("/remove-hamlet", ""),
    ("/restore-hamlet", ""),
    ("/erase-hamlet", ""),
    ("/unshelve", ""),
    ("/name-hamlet", "; its own name stays for commands"),
    ("/move-hamlet", " in the world"),
];

/// The commands about hamlets and votes, one a line.
pub fn help() -> String {
    LISTED
        .iter()
        .filter_map(|(name, more)| {
            let command = ALL.iter().find(|c| c.name == *name)?;
            let mut does = command.does.chars();
            let first = does.next().map(|c| c.to_ascii_lowercase());
            Some(format!(
                "{:<HELP_COLUMN$}{}{}{more}",
                command.usage(),
                first.map(String::from).unwrap_or_default(),
                does.as_str()
            ))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_is_named_once_and_read_by_its_name() {
        for (i, command) in ALL.iter().enumerate() {
            assert!(
                command.name.starts_with('/'),
                "{}",
                command.name
            );
            assert!(!command.does.is_empty(), "{}", command.name);
            assert!(
                ALL[..i].iter().all(|c| c.name != command.name),
                "{} twice",
                command.name
            );
        }
        let typed =
            read("  /remove scout  no longer needed ").unwrap();
        assert_eq!(
            typed.command.by,
            Handler::Hamlet(OnHamlet::Remove)
        );
        assert_eq!(typed.rest, "scout  no longer needed");
        assert_eq!(read("/new").unwrap().rest, "");
        assert!(read("hello /new").is_none());
        assert!(read("/nothing").is_none());
        let help = help();
        assert_eq!(help.lines().count(), LISTED.len());
        assert!(help.contains(
            "/found <hamlet> <mayor> <purpose> found a hamlet of \
             your own under a new mayor"
        ));
        assert!(help.ends_with(
            "set a hamlet's town down somewhere else in the world"
        ));
    }
}
