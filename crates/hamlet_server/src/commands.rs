//! The chief's commands: typed lines that begin with `/` (H24, H26),
//! said to an agent, whose hamlet is the one they mean.

use hamlet_protocol::client as protocol;

use crate::society::refusal::Refusal;

/// A command a chief can type: its name, how it is written, and what
/// it does.
struct Command {
    name: &'static str,
    usage: &'static str,
    does: &'static str,
}

const TABLE: &[Command] = &[
    Command {
        name: "hand",
        usage: "/hand",
        does: "Talk to the Hand",
    },
    Command {
        name: "hamlet",
        usage: "/hamlet <hamlet>",
        does: "Talk to a hamlet's mayor",
    },
    Command {
        name: "agent",
        usage: "/agent <address>",
        does: "Talk to an agent",
    },
    Command {
        name: "new",
        usage: "/new",
        does: "Start a new conversation",
    },
    Command {
        name: "resume",
        usage: "/resume [n]",
        does: "Go back to an earlier conversation",
    },
    Command {
        name: "retry",
        usage: "/retry",
        does: "Carry on a failed or stopped turn",
    },
    Command {
        name: "stop",
        usage: "/stop [address|all]",
        does: "Stop an agent's work",
    },
    Command {
        name: "to",
        usage: "/to <address> <instruction>",
        does: "Instruct another agent from here",
    },
    Command {
        name: "reflect",
        usage: "/reflect",
        does: "Ask the agent to keep what this session taught it",
    },
    Command {
        name: "worker",
        usage: "/worker <task>",
        does: "Start a worker on a task in this hamlet",
    },
    Command {
        name: "queue",
        usage: "/queue",
        does: "List what waits for you",
    },
    Command {
        name: "allow",
        usage: "/allow [#wait] [reason]",
        does: "Allow what waits, this once",
    },
    Command {
        name: "always",
        usage: "/always [#wait] [reason]",
        does: "Allow it, and the like from now on",
    },
    Command {
        name: "deny",
        usage: "/deny [#wait] [reason]",
        does: "Refuse what waits",
    },
    Command {
        name: "go",
        usage: "/go [#wait] [reason]",
        does: "Carry out an instruction an agent objected to",
    },
    Command {
        name: "withdraw",
        usage: "/withdraw [#wait] [reason]",
        does: "Withdraw an instruction an agent objected to",
    },
    Command {
        name: "reply",
        usage: "/reply [#wait] <text>",
        does: "Answer a question that waits",
    },
    Command {
        name: "dismiss",
        usage: "/dismiss [#wait]",
        does: "Put away a fault or a question",
    },
    Command {
        name: "grants",
        usage: "/grants",
        does: "List what agents may use for good",
    },
    Command {
        name: "revoke",
        usage: "/revoke <address> <network|unconfined>",
        does: "Take back a lasting grant",
    },
    Command {
        name: "tag",
        usage: "/tag <profile#id>[@<commit>] <version> [note]",
        does: "Name a version of a profile",
    },
    Command {
        name: "versions",
        usage: "/versions <profile#id>",
        does: "List a profile's versions and latest commits",
    },
    Command {
        name: "pin",
        usage: "/pin <address> <version|main> [reason]",
        does: "Hold a hire at a version, or let it follow main",
    },
    Command {
        name: "revert",
        usage: "/revert <profile#id> <commit|version> [reason]",
        does: "Undo a commit of a profile's main",
    },
    Command {
        name: "fork",
        usage: "/fork <profile#id>[@<version>] <new name>",
        does: "Make a new profile from a version of another",
    },
    Command {
        name: "purge",
        usage: "/purge <profile#id> <path>",
        does: "Take a file out of a profile's whole history",
    },
    Command {
        name: "unshelve",
        usage: "/unshelve <skill> [reason]",
        does: "Take a skill off the library's shelves",
    },
    Command {
        name: "dream",
        usage: "/dream <profile#id|address>",
        does: "Have a hire bring its learning home, or a profile \
               tidy",
    },
    Command {
        name: "hamlets",
        usage: "/hamlets",
        does: "List the hamlets",
    },
    Command {
        name: "create-hamlet",
        usage: "/create-hamlet [profile#id|blank] <name>[: purpose]",
        does: "Create a hamlet named <name> under a profile as its \
               mayor, or a blank agent named Mayor if none is given",
    },
    Command {
        name: "name-hamlet",
        usage: "/name-hamlet <hamlet> <name>",
        does: "Give a hamlet another name to be shown by",
    },
    Command {
        name: "move-hamlet",
        usage: "/move-hamlet <hamlet> <x> <z>",
        does: "Set a hamlet down elsewhere on the hamlet island",
    },
    Command {
        name: "remove-hamlet",
        usage: "/remove-hamlet <hamlet> [reason]",
        does: "Remove a hamlet and end its hires",
    },
    Command {
        name: "restore-hamlet",
        usage: "/restore-hamlet <hamlet> [reason]",
        does: "Bring a removed hamlet back",
    },
    Command {
        name: "erase-hamlet",
        usage: "/erase-hamlet <hamlet>",
        does: "Erase a removed hamlet for good",
    },
    Command {
        name: "tier",
        usage: "/tier <50|100|200|500>",
        does: "Set how many hires this hamlet holds",
    },
    Command {
        name: "mayor",
        usage: "/mayor <address> [reason]",
        does: "Give this hamlet's office to a hire",
    },
    Command {
        name: "hire",
        usage: "/hire <profile#id>[@<version>]|blank [in <hamlet>] \
                [under <address>] [purpose]",
        does: "Hire a profile into this hamlet, or a blank agent \
               that is not a profile of the city",
    },
    Command {
        name: "end",
        usage: "/end <address> [reason]",
        does: "End a hire",
    },
    Command {
        name: "restore",
        usage: "/restore <address> [reason]",
        does: "Bring an ended hire back",
    },
    Command {
        name: "erase",
        usage: "/erase <address>",
        does: "Erase an ended hire for good",
    },
    Command {
        name: "merge",
        usage: "/merge <address> into <address> [reason]",
        does: "Make one hire of two; /restore splits them again",
    },
    Command {
        name: "model",
        usage: "/model <address> <model|none> [reason]",
        does: "Choose an agent's model",
    },
    Command {
        name: "effort",
        usage: "/effort <address> \
                <off|minimal|low|medium|high|xhigh|max|none>",
        does: "Choose how hard an agent's model thinks",
    },
    Command {
        name: "fast",
        usage: "/fast <address> <on|off>",
        does: "Turn an agent's fast tier on or off",
    },
    Command {
        name: "nickname",
        usage: "/nickname <address> [nickname]",
        does: "Call a hire or a worker something in the world, or \
               by its name again",
    },
    Command {
        name: "tools",
        usage: "/tools <address> -<tool> +<tool>",
        does: "Turn a hire's tools off or back on",
    },
    Command {
        name: "allowance",
        usage: "/allowance <address> <dollars|none>",
        does: "Give a hire an allowance for the period",
    },
    Command {
        name: "road",
        usage: "/road <address> <address> [reason]",
        does: "Lay a road between two hires",
    },
    Command {
        name: "unroad",
        usage: "/unroad <address> <address> [reason]",
        does: "Take up a road",
    },
    Command {
        name: "folder",
        usage: "/folder <folder|none>",
        does: "Choose the folder this hamlet works on",
    },
    Command {
        name: "mode",
        usage: "/mode <manual|auto>",
        does: "Choose how this hamlet's work is overseen",
    },
    Command {
        name: "budget",
        usage: "/budget",
        does: "Show this hamlet's spend",
    },
    Command {
        name: "reset-budget",
        usage: "/reset-budget",
        does: "Start this hamlet's budget period again",
    },
    Command {
        name: "history",
        usage: "/history",
        does: "Show this hamlet's decisions",
    },
    Command {
        name: "profiles",
        usage: "/profiles",
        does: "List the city's profiles",
    },
    Command {
        name: "profile",
        usage: "/profile new <name>[: <character>]",
        does: "Make a profile in the city; a name is a label with \
               no colon, and the character follows the colon",
    },
    Command {
        name: "name-profile",
        usage: "/name-profile <profile#id> <name>",
        does: "Give a profile a new name",
    },
    Command {
        name: "remove-profile",
        usage: "/remove-profile <profile#id> [reason]",
        does: "Archive a profile and end its hires",
    },
    Command {
        name: "restore-profile",
        usage: "/restore-profile <profile#id> [reason]",
        does: "Bring an archived profile back with its hires",
    },
    Command {
        name: "erase-profile",
        usage: "/erase-profile <profile#id>",
        does: "Erase an archived profile for good",
    },
    Command {
        name: "move-house",
        usage: "/move-house <profile#id> <x> <z> [degrees]",
        does: "Set a profile's house down elsewhere in the city",
    },
    Command {
        name: "welcome",
        usage: "/welcome <profile#id> [reason]",
        does: "Let a profile waiting at the harbor into the city",
    },
    Command {
        name: "turn-away",
        usage: "/turn-away <profile#id> [reason]",
        does: "Keep a profile at the harbor, or send it back there",
    },
];

/// The commands a client may offer.
pub(crate) fn deeds() -> Vec<protocol::Deed> {
    TABLE
        .iter()
        .map(|command| protocol::Deed {
            description: command.does.into(),
            name: command.name.into(),
            // A deed's usage is what follows its name.
            usage: command
                .usage
                .strip_prefix('/')
                .and_then(|rest| rest.strip_prefix(command.name))
                .unwrap_or(command.usage)
                .trim()
                .into(),
        })
        .collect()
}

pub(crate) fn usage(name: &str) -> Refusal {
    let usage = TABLE
        .iter()
        .find(|c| c.name == name)
        .map_or("", |c| c.usage);
    Refusal::Usage {
        words: format!("Use {usage}"),
    }
}
