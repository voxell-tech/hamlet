//! The bazaar's five stalls: what each sells and who keeps it.

/// A stall of the bazaar.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Stall {
    /// Models: the minds agents think with, from OpenRouter and the
    /// home's endpoints.
    #[default]
    Agents,
    /// Agents other chiefs made, ready to join a hamlet.
    Artisans,
    /// Plugins: contraptions that add to Hamlet itself.
    Plugins,
    /// Skills from skills.sh, for the library.
    Skills,
    /// Whole hamlets: a mayor, its agents and all they know.
    Hamlets,
}

/// The one who keeps a stall, as the chief meets them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Keeper {
    pub name: &'static str,
    /// What they are, after their name.
    pub title: &'static str,
    /// What they say to a chief who comes to the stall, one at a
    /// time.
    pub greetings: [&'static str; 3],
}

impl Stall {
    pub const ALL: [Stall; 5] = [
        Stall::Agents,
        Stall::Artisans,
        Stall::Plugins,
        Stall::Skills,
        Stall::Hamlets,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Stall::Agents => "Agent Stall",
            Stall::Artisans => "Artisans' Stall",
            Stall::Plugins => "Tinker's Stall",
            Stall::Skills => "Skill Stall",
            Stall::Hamlets => "Hamlet Store",
        }
    }

    /// What it sells, in a few words.
    pub fn sells(self) -> &'static str {
        match self {
            Stall::Agents => {
                "Minds for your agents, from OpenRouter and your own \
                 endpoints."
            }
            Stall::Artisans => {
                "Agents other chiefs made by hand, ready to join a \
                 hamlet."
            }
            Stall::Plugins => {
                "Contraptions that add to Hamlet itself."
            }
            Stall::Skills => {
                "Skills from skills.sh, for the library."
            }
            Stall::Hamlets => {
                "Whole hamlets: a mayor, its agents and all they \
                 know."
            }
        }
    }

    pub fn keeper(self) -> Keeper {
        match self {
            Stall::Agents => Keeper {
                name: "Brannoc",
                title: "the mind-monger",
                greetings: [
                    "Minds, fresh from OpenRouter! Quick ones, deep \
                     ones, cheap ones. Every one of them can hold a \
                     tool.",
                    "Tell me what your agent must do, chief, and \
                     I'll tell you which mind to give it.",
                    "Mind the price per million tokens. A clever \
                     mind that bankrupts the hamlet is no bargain.",
                ],
            },
            Stall::Artisans => Keeper {
                name: "Wren Halloway",
                title: "broker of crafted agents",
                greetings: [
                    "Every agent here was made by a chief's own \
                     hand: a purpose, a character and the skills to \
                     match.",
                    "Hire one and it moves into your hamlet the \
                     same day, house and all.",
                    "The makers are paid for every hire. Good work \
                     should feed its maker.",
                ],
            },
            Stall::Plugins => Keeper {
                name: "Tobias Cogsworth",
                title: "the tinker",
                greetings: [
                    "Gears, gadgets and gizmos! Each one teaches \
                     Hamlet a new trick.",
                    "A canvas to draw on, a ledger that counts \
                     itself: bolt one on and the whole realm can \
                     use it.",
                    "Careful with that one. It's still ticking.",
                ],
            },
            Stall::Skills => Keeper {
                name: "Sister Marjoram",
                title: "keeper of the skill-scrolls",
                greetings: [
                    "Scrolls from skills.sh, every one. Take one to \
                     the library and all your agents may learn it.",
                    "A skill is a habit written down. Choose the \
                     ones your agents will use.",
                    "The most read are on top. The librarian will \
                     thank you for them.",
                ],
            },
            Stall::Hamlets => Keeper {
                name: "Lady Ashgrove",
                title: "seller of land and those who keep it",
                greetings: [
                    "Why found a hamlet when you can buy one \
                     standing? Mayor, agents, notes and all.",
                    "Each comes as it was kept: its houses, its \
                     roads and every word its agents wrote.",
                    "Sign here, and it rises on your land before \
                     the ink is dry.",
                ],
            },
        }
    }
}
