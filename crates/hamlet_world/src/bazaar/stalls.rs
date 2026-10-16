//! The bazaar's six stalls: what each sells and who keeps it.

/// A stall of the bazaar.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Stall {
    /// The models agents run, from OpenRouter and the home's
    /// endpoints.
    #[default]
    Models,
    /// Agents other users made, ready to join a hamlet.
    Community,
    /// Extensions that add features to Hamlet itself.
    Plugins,
    /// Skills from skills.sh, for the library.
    Skills,
    /// Whole hamlets, mayor, agents and notes, ready to use.
    Hamlets,
    /// The official assets: skins, emotes and meshes for agents and
    /// hamlets to wear.
    Assets,
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
    pub const ALL: [Stall; 6] = [
        Stall::Models,
        Stall::Community,
        Stall::Plugins,
        Stall::Skills,
        Stall::Hamlets,
        Stall::Assets,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Stall::Models => "AI Models",
            Stall::Community => "Community Agents",
            Stall::Plugins => "Plugins",
            Stall::Skills => "Skills",
            Stall::Hamlets => "Hamlet Templates",
            Stall::Assets => "Official Assets",
        }
    }

    /// What it sells, in a few words.
    pub fn sells(self) -> &'static str {
        match self {
            Stall::Models => {
                "Language models for your agents, from OpenRouter \
                 and your own endpoints."
            }
            Stall::Community => {
                "Ready-made agents built by other users."
            }
            Stall::Plugins => {
                "Extensions that add features to Hamlet."
            }
            Stall::Skills => {
                "Skills from skills.sh to add to your library."
            }
            Stall::Hamlets => {
                "Complete hamlets with a mayor, agents and notes, \
                 ready to use."
            }
            Stall::Assets => {
                "Skins, emotes and meshes for your agents and \
                 hamlets, from the Hamlet team."
            }
        }
    }

    pub fn keeper(self) -> Keeper {
        match self {
            Stall::Models => Keeper {
                name: "Brannoc",
                title: "sells AI models",
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
            Stall::Community => Keeper {
                name: "Wren Halloway",
                title: "sells community agents",
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
                title: "sells plugins",
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
                title: "sells skills",
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
                title: "sells hamlet templates",
                greetings: [
                    "Why found a hamlet when you can buy one \
                     standing? Mayor, agents, notes and all.",
                    "Each comes as it was kept: its houses, its \
                     roads and every word its agents wrote.",
                    "Sign here, and it rises on your land before \
                     the ink is dry.",
                ],
            },
            Stall::Assets => Keeper {
                name: "Madame Perpetua",
                title: "sells official assets",
                greetings: [
                    "Skins, emotes and whole new frames, chief! \
                     Every one cut by the Hamlet team.",
                    "Turn it on the turntable before you buy. A \
                     look should please from every side.",
                    "Bought once, worn by whichever agent or hamlet \
                     you like. Change it on their page.",
                ],
            },
        }
    }
}
