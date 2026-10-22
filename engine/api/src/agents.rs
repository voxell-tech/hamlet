//! The agents service: the agents' settings, their models, signing
//! in, the charter, the realm's limits, the lasting permissions, the
//! status and the sessions. A plugin asks with an [`AgentsRequest`]
//! and is answered with an [`AgentsReply`]. A key goes in and never
//! comes out: no reply holds one.

mod conversation;
mod market;
mod needs;
mod roster;

use serde::{Deserialize, Serialize};

pub use crate::agents::conversation::{
    Change, ChangeLine, Focus, Gift, Mark, Papers, Part, Past, Pasts,
    Related, Selected, Skill,
};
pub use crate::agents::market::{
    ListedModel, Settler, SettlerSkill, SkillFile, Template,
};
pub use crate::agents::needs::{
    Choice, Danger, Need, NeedKind, Offer, Pick, Question, Questions,
    Sheet,
};
pub use crate::agents::roster::{
    Bylaws, ClosedHamlet, Doing, Paper, Record, RoadUse, Roster,
    RosterAgent, RosterHamlet, RosterRoad, Vote,
};
use crate::capability::Capability;

/// The target of every agents capability: the agents run on this
/// machine.
pub const LOCAL: &str = "local";

/// What a plugin asks of the agents service.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub enum AgentsRequest {
    /// Sends each [`AgentsEvent`] from now on, as
    /// [`AgentsReply::Event`] with this request's ticket.
    Watch,

    /// The settings: [`AgentsReply::Settings`].
    Settings,
    /// The model that runs where nobody chose one, `endpoint::model`.
    SetModel(String),
    /// Turns between two reflections; 0 is never.
    SetReflectEvery(u32),
    /// Network access for the agents' tools.
    SetAllowNet(bool),
    /// Takes an endpoint out, forgets its key, and signs out of
    /// ChatGPT if it was the plan's.
    RemoveEndpoint(String),
    SetKey {
        endpoint: String,
        key: String,
    },
    ForgetKey(String),
    /// Opens the ChatGPT plan's usage page in the browser.
    OpenUsage,

    /// Lists the models of the default model's endpoint:
    /// [`AgentsEvent::Link`].
    TestConnection,
    /// Asks every endpoint for its models and writes them into the
    /// catalogue: [`AgentsEvent::Listing`].
    ListModels,
    /// The catalogue: [`AgentsReply::Catalogue`].
    Catalogue,
    AllowModel {
        model: String,
        allowed: bool,
    },
    /// The person's own prices of a model with none known, dollars
    /// per million tokens.
    PriceModel {
        model: String,
        input: f64,
        output: f64,
    },
    /// Asks a model, `endpoint::model`, for a word:
    /// [`AgentsEvent::Trial`].
    TryModel(String),

    /// The providers a key can be of: [`AgentsReply::Providers`].
    Providers,
    /// The setup of a place for models: [`AgentsReply::Setup`].
    Setup,
    /// Starts a setup afresh. `adding` adds a place to a home that
    /// has its model, and leaves the model as it is.
    BeginSetup {
        adding: bool,
    },
    /// Asks a provider, by the endpoint name it has in
    /// [`Provider`], or the address `url` when it is none, with
    /// `key`.
    Connect {
        provider: Option<String>,
        url: String,
        key: String,
    },
    SignIn(SignInTo),
    /// Looks for a model server on this computer.
    FindLocal,
    /// Tries the model `id` of the place that answered, and keeps it
    /// as the default once it answers.
    PickModel(String),
    EndSetup,

    /// The charter: [`AgentsReply::Charter`].
    Charter,
    /// Gives the home a charter of its own if it has none, and opens
    /// it in the system's editor.
    EditCharter,
    /// Takes the home's own charter away, back to the one built in.
    ResetCharter,

    /// The realm's limits: [`AgentsReply::Limits`].
    Limits,
    /// Sets one key of the realm's limits to `value`, written as in
    /// the file, or takes it out when `value` is none. A value the
    /// file cannot take fails with the problem.
    SetLimit {
        section: String,
        key: String,
        value: Option<String>,
    },

    /// The lasting permissions: [`AgentsReply::Grants`].
    Grants,
    Revoke {
        agent: String,
        permission: String,
    },

    /// The status: [`AgentsReply::Status`].
    Status,

    /// Sends a line, a message or a slash command, as the chief.
    Send {
        to: Recipient,
        text: String,
    },
    /// Stops what an agent does, or every agent with none.
    Veto(Option<String>),

    /// The sessions loaded: [`AgentsReply::Sessions`].
    Sessions,
    /// Opens the chief's session with `Recipient`, the open
    /// conversation of the focus and the one the status tells of,
    /// and selects its agent: [`AgentsReply::Session`].
    Open(Recipient),
    /// The last `most` messages of a session, after the message
    /// `after`: [`AgentsReply::Messages`].
    Messages {
        session: SessionName,
        after: Option<u64>,
        most: u32,
    },

    /// The window's focus: [`AgentsReply::Focus`].
    Focus,
    /// Selects a thing. A living agent selected is talked to, as
    /// `Open` does.
    Select(Selected),
    /// The sessions shown in the chat of the open conversation's
    /// agent, the open one first: [`AgentsReply::Related`].
    Related,
    /// Takes a session back to just before its message `entry`:
    /// [`AgentsEvent::Rewound`] once it is.
    Rewind {
        session: SessionName,
        entry: u64,
    },
    /// Gives a chief's command to the hamlet `hamlet`: in the open
    /// conversation when it is with that hamlet, to its mayor
    /// otherwise, with the answers told in the open conversation.
    Decree {
        hamlet: String,
        text: String,
    },
    /// What the agent of the open conversation keeps in its house:
    /// [`AgentsReply::Papers`].
    Papers,
    /// The prompt the agent of the open conversation is given now:
    /// [`AgentsReply::Prompt`].
    Prompt,
    /// Gives the agent of the open conversation a character file if
    /// it has none, and opens it in the system's editor.
    EditCharacter,
    /// The library's skills: [`AgentsReply::Skills`].
    Skills,
    /// The past sessions of the open conversation's agent with every
    /// word of `query`: [`AgentsReply::Pasts`].
    Pasts {
        query: String,
    },
    /// The messages of a past session, by its name:
    /// [`AgentsReply::Messages`].
    ReadPast(String),
    /// Deletes an archived session for good.
    DeletePast(String),
    /// The agents and the hamlets: [`AgentsReply::Roster`].
    Roster,
    /// A hamlet's record, by its name: [`AgentsReply::Record`].
    Record(String),
    /// A hamlet's own limits, by its name: [`AgentsReply::Bylaws`].
    Bylaws(String),
    /// Sets one key of the limits of the hamlet `hamlet` to `value`,
    /// written as in the file, or takes it out when `value` is none.
    /// A key of the realm's is refused; a value the file cannot take
    /// fails with the problem.
    SetBylaw {
        hamlet: String,
        section: String,
        key: String,
        value: Option<String>,
    },
    /// What waits for the chief: [`AgentsReply::Needs`].
    Needs,
    /// Answers something that waits, with a reason or a reply.
    Answer {
        need: u64,
        choice: Choice,
        text: String,
    },
    /// Picks an answer to one question of something that waits, or
    /// takes it back with none. An option picked shows the next
    /// question.
    Pick {
        need: u64,
        question: u32,
        pick: Option<Pick>,
    },
    /// Shows the question `to` of something that waits.
    Turn {
        need: u64,
        to: u32,
    },
    /// Sends the answers picked for something that waits once each
    /// question has one, and shows the first without one otherwise.
    Submit(u64),
    /// Has a living agent run `model`, `endpoint::model`, from now
    /// on: the mayor's is pinned, another's is chosen as its mayor
    /// would.
    RunModel {
        agent: String,
        model: String,
    },

    /// Adds `model`, listed by `endpoint`, to the catalogue, allowed:
    /// [`AgentsReply::Added`] with the name it goes by,
    /// `endpoint::id`.
    AddModel {
        endpoint: String,
        model: ListedModel,
    },
    /// Takes the skill `name`, its files, into the library, refused
    /// when the library has one of that name.
    TakeSkill {
        name: String,
        files: Vec<SkillFile>,
    },
    /// Founds a hamlet from `Template` and moves in its mayor and
    /// agents. Everything is checked before anything is written.
    FoundFrom(Template),
}

impl AgentsRequest {
    /// The capability it needs, on [`LOCAL`].
    pub fn capability(&self) -> Capability {
        match self {
            Self::SetKey { .. }
            | Self::ForgetKey(_)
            | Self::Connect { .. }
            | Self::SignIn(_) => Capability::AgentsKeys,
            Self::SetModel(_)
            | Self::SetReflectEvery(_)
            | Self::SetAllowNet(_)
            | Self::RemoveEndpoint(_)
            | Self::OpenUsage
            | Self::TestConnection
            | Self::ListModels
            | Self::AllowModel { .. }
            | Self::PriceModel { .. }
            | Self::TryModel(_)
            | Self::BeginSetup { .. }
            | Self::FindLocal
            | Self::PickModel(_)
            | Self::EndSetup
            | Self::EditCharter
            | Self::ResetCharter
            | Self::SetLimit { .. }
            | Self::SetBylaw { .. }
            | Self::AddModel { .. } => Capability::AgentsSettings,
            Self::Revoke { .. }
            | Self::Send { .. }
            | Self::Veto(_) => Capability::AgentsChief,
            Self::Watch
            | Self::Settings
            | Self::Catalogue
            | Self::Providers
            | Self::Setup
            | Self::Charter
            | Self::Limits
            | Self::Grants
            | Self::Status
            | Self::Sessions
            | Self::Open(_)
            | Self::Messages { .. } => Capability::AgentsRead,
            Self::Rewind { .. }
            | Self::Decree { .. }
            | Self::EditCharacter
            | Self::DeletePast(_)
            | Self::Answer { .. }
            | Self::Pick { .. }
            | Self::Turn { .. }
            | Self::Submit(_)
            | Self::RunModel { .. }
            | Self::TakeSkill { .. }
            | Self::FoundFrom(_) => Capability::AgentsChief,
            Self::Focus
            | Self::Select(_)
            | Self::Related
            | Self::Papers
            | Self::Prompt
            | Self::Skills
            | Self::Pasts { .. }
            | Self::ReadPast(_)
            | Self::Roster
            | Self::Record(_)
            | Self::Bylaws(_)
            | Self::Needs => Capability::AgentsRead,
        }
    }
}

/// What a request of the agents service came to.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub enum AgentsReply {
    Done,
    Watching,
    Settings(Settings),
    Catalogue(Vec<CatalogueModel>),
    Providers(Vec<Provider>),
    Setup(Setup),
    Charter(Charter),
    Limits(RealmLimits),
    Grants(Vec<Grant>),
    Status(Status),
    Sessions(Vec<Session>),
    Session(Session),
    Messages(Vec<SessionMessage>),
    Event(AgentsEvent),
    Focus(Focus),
    Related(Vec<Related>),
    Papers(Papers),
    Prompt(String),
    Skills(Vec<Skill>),
    Pasts(Pasts),
    Roster(Roster),
    Record(Record),
    Bylaws(Bylaws),
    Needs(Vec<Need>),
    /// The name a model added goes by, `endpoint::id`.
    Added(String),
}

/// A change, sent to each plugin that watches.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub enum AgentsEvent {
    Settings(Settings),
    Link(Link),
    Listing(Listing),
    /// The catalogue changed: ask for it again.
    Catalogue,
    Trial(Trial),
    Setup(Setup),
    Charter(Charter),
    Limits(RealmLimits),
    Grants(Vec<Grant>),
    Status(Status),
    /// Sessions came or went.
    Sessions,
    /// What a session said or did as it worked.
    Output {
        session: SessionName,
        output: Output,
    },
    Working {
        session: SessionName,
        working: bool,
    },
    Focus(Focus),
    /// Messages of a session that came or changed as it worked.
    /// `again` when it was read afresh, as after a rewind or a new
    /// session: what was had of it is gone, to be asked for again.
    Said {
        session: SessionName,
        messages: Vec<SessionMessage>,
        again: bool,
    },
    /// A session was taken back to before the message `text`.
    Rewound {
        session: SessionName,
        text: String,
    },
    Skills(Vec<Skill>),
    Roster(Roster),
    Needs(Vec<Need>),
}

/// The settings of the agents, with no key.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Settings {
    /// The model that runs where nobody chose one,
    /// `endpoint::model`.
    pub model: String,
    pub reflect_every: u32,
    pub allow_net: bool,
    pub endpoints: Vec<Endpoint>,
    /// The home has a model chosen: settings of its own, or a model
    /// named for this run.
    pub chosen: bool,
    /// Why the last change could not be saved.
    pub problem: Option<String>,
    pub link: Link,
    pub listing: Listing,
}

/// A place models are served, with no key.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Endpoint {
    pub name: String,
    pub url: String,
    pub has_key: bool,
    /// Served on the person's ChatGPT plan.
    pub chatgpt: bool,
}

/// The default model's endpoint as of its last test.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Link {
    Untested,
    Testing,
    /// The models it lists.
    Connected(Vec<String>),
    Failed(String),
}

/// What each endpoint answered when last asked for its models.
#[derive(
    Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize,
)]
pub struct Listing {
    /// Endpoints that have not answered yet.
    pub asking: u32,
    pub said: Vec<Listed>,
}

/// One endpoint's answer: how many models it has, or why it could
/// not be asked.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Listed {
    pub endpoint: String,
    pub models: Result<u32, String>,
}

/// The outcome of the last trial of a model.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Trial {
    Untried,
    Trying,
    Answered,
    Failed(String),
}

/// A model of the catalogue.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct CatalogueModel {
    /// `endpoint::id`.
    pub name: String,
    pub allowed: bool,
    /// Dollars per million tokens in.
    pub input: f64,
    pub output: f64,
    pub priced: Priced,
    /// Tokens it holds.
    pub context: Option<u64>,
    pub note: String,
}

/// Where a model's price comes from.
#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize,
)]
pub enum Priced {
    Unknown,
    /// The endpoint lists it.
    Listed,
    /// The home assigned it.
    Assigned,
    /// The person's own.
    Yours,
}

/// A provider a key can be of.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Provider {
    pub name: String,
    /// The name its endpoint is given.
    pub endpoint: String,
    pub url: String,
    /// The place a key for it is made.
    pub keys_at: String,
}

#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize,
)]
pub enum SignInTo {
    /// For a key to OpenRouter, which the service keeps.
    OpenRouter,
    /// For the use of the person's ChatGPT plan.
    ChatGpt,
}

/// The setup of a place for models.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Setup {
    /// A setup was begun and has not ended yet.
    pub begun: bool,
    pub adding: bool,
    pub stage: Stage,
    /// The name of whoever answered.
    pub from: String,
    /// Whoever answered is the ChatGPT plan.
    pub plan: bool,
    /// The models it answered with.
    pub models: Vec<String>,
    /// Why the last step did not work, with what to do, in plain
    /// words.
    pub problem: Option<String>,
}

#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize,
)]
pub enum Stage {
    Idle,
    /// A place is asked for its models.
    Asking,
    /// The person is signing in in the browser.
    SigningIn(SignInTo),
    /// A model server on this computer is looked for.
    Looking,
    /// A place answered with its models.
    Found,
    /// A model picked is asked for a word.
    Trying,
    /// The place and its model are kept.
    Done,
}

/// The words every agent's prompt opens with.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Charter {
    pub text: String,
    /// The home keeps a charter of its own.
    pub own: bool,
}

/// The limits that hold for every hamlet.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct RealmLimits {
    pub most_hamlets: u64,
    pub roads_between: bool,
    pub removal_mayors: u64,
    pub grows_over_days: u64,
    /// The share a vote passes with: yes of so many.
    pub pass: (u32, u32),
    pub least_voters: u64,
    pub sealed: bool,
    pub grounded_only: bool,
    pub ballot_minutes: u64,
    pub open_minutes: u64,
    pub retry_minutes: u64,
    /// Why the file cannot be read; the values are then the
    /// defaults.
    pub problem: Option<String>,
}

/// A permission the chief gave an agent for good.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Grant {
    /// The agent's number in its hamlet's record.
    pub agent: u64,
    /// The agent's name when it was given.
    pub name: String,
    pub permission: String,
    /// The chief who gave it.
    pub by: String,
}

/// The facts of the status line.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Status {
    /// The sandbox the agents' commands run in.
    pub sandbox: String,
    pub model: String,
    pub link: Link,
    /// What the open session used.
    pub usage: String,
    /// The first hamlet's spend against its budget.
    pub treasury: String,
    /// Things that wait for the chief.
    pub waiting: u32,
    /// The open session works.
    pub working: bool,
}

/// Who the chief talks to.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Recipient {
    /// An agent by its name.
    Agent(String),
    /// Whoever is mayor of a hamlet.
    Mayor(String),
}

/// The channel, chat and thread a session is found by.
#[derive(
    Clone, PartialEq, Eq, Hash, Debug, Serialize, Deserialize,
)]
pub struct SessionName {
    pub channel: String,
    pub chat: String,
    pub thread: String,
}

/// A session loaded.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Session {
    pub name: SessionName,
    /// The agent it talks to now.
    pub agent: Option<String>,
    pub working: bool,
    pub usage: String,
    /// The id of its last message.
    pub last: Option<u64>,
    /// The time the session in it began, in milliseconds since 1970.
    pub began: u64,
    /// Whoever it is with has something to answer.
    pub answering: bool,
}

/// A message of a session, with its id and its time in milliseconds
/// since 1970.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct SessionMessage {
    pub id: u64,
    pub at: u64,
    pub entry: Entry,
    /// Another agent's own work, done while it was asked.
    pub another: bool,
    /// An agent's reply to a letter, said back as a user.
    pub reply: bool,
}

/// One thing said or done in a session.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Entry {
    User {
        user: String,
        text: String,
    },
    Assistant(String),
    Reasoning(String),
    Tool {
        name: String,
        summary: String,
        /// The shell command, or the arguments when it runs none.
        command: String,
        exit: Option<i32>,
        /// The change it asks for, when it is an edit.
        change: Option<Change>,
    },
    Error(String),
    Notice(String),
    /// The chief's answers to an agent's questions.
    Answered(String),
    /// Words of another agent of the hamlet.
    Agent {
        name: String,
        text: String,
    },
}

/// What a session said or did as it worked.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Output {
    Text(String),
    Reasoning(String),
    Tool {
        name: String,
        summary: String,
    },
    ToolDone {
        name: String,
        exit: i32,
        failure: Option<String>,
    },
    Notice(String),
    /// Another agent it waits on did something.
    Agent(String),
    /// The final text of a turn.
    Done(String),
    Error(String),
}
