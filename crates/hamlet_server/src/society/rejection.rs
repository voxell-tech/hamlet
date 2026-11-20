//! The reasons the society rejects a change, in words a chief or an
//! agent reads.

use serde::{Deserialize, Serialize};

/// An expected outcome that leaves the realm as it was.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(tag = "reason", rename_all = "kebab-case")]
pub(crate) enum Rejection {
    #[error(
        "`{name}` is not a name: use one line of 1 to 40 characters \
         with no colon"
    )]
    BadName { name: String },
    #[error("no hamlet has the id `{id}`")]
    NoHamlet { id: String },
    #[error("the hamlet `{id}` was removed; restore it first")]
    HamletRemoved { id: String },
    #[error("the hamlet `{id}` is still living; remove it first")]
    HamletLiving { id: String },
    #[error("the hamlet `{id}` was erased")]
    HamletErased { id: String },
    #[error("the hamlet `{id}` holds its most agents, {most}")]
    HamletFull { id: String, most: i64 },
    #[error("a hamlet's tier is 50, 100, 200 or 500 agents")]
    BadTier,
    #[error("no profile has the address `{address}`")]
    NoProfile { address: String },
    #[error("`{name}` is another chief's profile")]
    ProfileNotOwn { name: String },
    #[error("the profile `{name}` no longer lives in the city")]
    ProfileGone { name: String },
    #[error("the profile `{name}` is still living; remove it first")]
    ProfileLiving { name: String },
    #[error("the profile `{name}` was erased")]
    ProfileErased { name: String },
    #[error(
        "`{name}` is a blank agent's own, not a profile of the city"
    )]
    Apart { name: String },
    #[error("the profile `{name}` lives in the city already")]
    Housed { name: String },
    #[error("the profile `{name}` was turned away already")]
    TurnedAway { name: String },
    #[error(
        "the profile `{name}` waits at the harbor; welcome it into \
         the city first"
    )]
    AtHarbor { name: String },
    #[error(
        "the house of `{name}` would stand on another house or the \
         city's middle"
    )]
    Crowded { name: String },
    #[error("`{name}` still has agents that may come back: {agents}")]
    ProfileHasAgents { name: String, agents: String },
    #[error("`{name}` has no version `{version}`")]
    NoVersion { name: String, version: String },
    #[error("`{name}` has no branch yet; try again in a moment")]
    NoBranch { name: String },
    #[error(
        "`{version}` is not a version's name: use at most 32 \
         letters, digits, dots and hyphens"
    )]
    BadVersion { version: String },
    #[error("the version `{version}` exists already; choose another")]
    VersionTaken { version: String },
    #[error("a character may have at most 4000 characters")]
    LongCharacter,
    #[error("say what it is for")]
    NoPurpose,
    #[error("only a chief of the hamlet `{hamlet}` may do that")]
    NotChief { hamlet: String },
    #[error("only a chief of the realm may do that")]
    NotRealmChief,
    #[error(
        "only a chief chooses your own model, effort and fast tier"
    )]
    OwnSteering,
    #[error("only `{owner}`'s chief may do that")]
    NotOwner { owner: String },
    #[error("only a parent of `{name}` may do that")]
    NotParent { name: String },
    #[error("only a lead may do that")]
    NotLead,
    #[error("a worker cannot do that")]
    Worker,
    #[error("`{name}` has left: {how}")]
    Left { name: String, how: String },
    #[error("`{name}` has not left")]
    NotLeft { name: String },
    #[error("`{name}` was erased")]
    Erased { name: String },
    #[error("a lead cannot be ended; set another lead first")]
    EndingLead,
    #[error(
        "`{name}` is on a chief's task; a parent may end or merge \
         it once it has answered and its protected minutes have \
         passed"
    )]
    OnChiefsTask { name: String },
    #[error("`{name}` is already the lead")]
    AlreadyLead { name: String },
    #[error(
        "the lead cannot be moved; give the office to another agent"
    )]
    MovingLead,
    #[error("`{name}` is already under `{parent}`")]
    AlreadyUnder { name: String, parent: String },
    #[error(
        "`{name}` cannot go under `{parent}`, which is itself or \
         under it"
    )]
    UnderItself { name: String, parent: String },
    #[error("`{name}` is in another hamlet")]
    OtherHamlet { name: String },
    #[error("no agent has the address `{address}`")]
    NoOne { address: String },
    #[error("`{name}` is not an agent of a hamlet")]
    NotHamletAgent { name: String },
    #[error("`{address}` is not an address")]
    BadAddress { address: String },
    #[error("a channel needs two different agents")]
    SameEnds,
    #[error("`{a}` and `{b}` already have a channel")]
    ChannelExists { a: String, b: String },
    #[error("`{a}` and `{b}` have no channel to close")]
    NoChannel { a: String, b: String },
    #[error("the hamlet `{hamlet}` has its most channels, {most}")]
    TooManyChannels { hamlet: String, most: i64 },
    #[error("the chief allows no channels between hamlets")]
    NoChannelsBetween,
    #[error(
        "both ends of an agreed channel must be in different hamlets"
    )]
    SameHamlet,
    #[error("`{name}` has no such tool: {tool}")]
    NoTool { name: String, tool: String },
    #[error("a worker's tools are a part of its starter's")]
    WorkerTools,
    #[error("say what the worker is to do")]
    NoBrief,
    #[error(
        "a chief chose `{name}`'s model; only a chief may change it"
    )]
    ChiefChose { name: String },
    #[error(
        "the hamlet `{id}` holds {count} agents, more than {tier}"
    )]
    TierTooSmall { id: String, count: i64, tier: i64 },
    #[error("a channel joins agents of hamlets only")]
    ChannelEnds,
    #[error("that line is no longer queued")]
    NotQueued,
    #[error("a turn is running here; stop it, then {then}")]
    TurnRunning { then: String },
    #[error(
        "the last message was answered, so there is nothing to \
         carry on"
    )]
    Answered,
    #[error("this server does not do that yet: {what}")]
    Unsupported { what: String },
    #[error("only a chief of `{name}`'s hamlet may reach it")]
    NotYours { name: String },
    #[error("nothing waits here with that id")]
    NoWait,
    #[error("no model is named `{model}`")]
    NoModel { model: String },
    #[error(
        "no model is set up yet: choose one in Settings, under \
         Models"
    )]
    NoModelSetUp,
    #[error(
        "the model `{model}` takes no images; choose another model \
         or leave the images out"
    )]
    NoImages { model: String },
    #[error(
        "the model `{model}` takes at most {most} images in a line"
    )]
    TooManyImages { model: String, most: u32 },
    #[error("no image `{image}` was sent first")]
    NoImage { image: String },
    #[error("a command takes no images or files")]
    CommandAttachments,
    #[error("no skill is named `{name}`")]
    NoSkill { name: String },
    #[error("there is no file `{path}` here")]
    NoFile { path: String },
    #[error("that merge was split already")]
    AlreadySplit,
    #[error("This connection is closing.")]
    ConnectionClosing,
    #[error("A file is at most 256 MiB.")]
    FileTooBig,
    #[error("Too many files are being sent at once.")]
    TooManyFilesSent,
    #[error("No file of yours is being sent under that upload.")]
    NoFileUpload,
    #[error("A part's data is base64.")]
    NotBase64,
    #[error("The file continues at byte {received}.")]
    FileContinuesAt { received: u64 },
    #[error("The file is longer than declared.")]
    FileLongerThanDeclared,
    #[error("A line takes at most 10000 files.")]
    TooManyFilesInLine,
    #[error("The file `{name}` was not sent first.")]
    FileNotSent { name: String },
    #[error("The file `{name}` is not the size it was sent as.")]
    FileSizeDiffers { name: String },
    #[error("A line takes at most 1 GiB of files.")]
    FilesTooBigInLine,
    #[error("An image has bytes.")]
    ImageEmpty,
    #[error("An image is at most 20 MiB.")]
    ImageTooBig,
    #[error("Too many images are being sent at once.")]
    TooManyImagesSent,
    #[error("No image of yours is being sent under that upload.")]
    NoImageUpload,
    #[error("The image continues at byte {received}.")]
    ImageContinuesAt { received: u64 },
    #[error("The image is longer than declared.")]
    ImageLongerThanDeclared,
    #[error(
        "An attachment's place, {at}, is not a byte offset on a \
         character boundary of the line, which is {bytes} bytes."
    )]
    BadAttachmentPlace { at: u64, bytes: usize },
    #[error("{why}")]
    ImageUnusable { why: String },
    #[error("`{name}` is not a name of a file.")]
    NotFileName { name: String },
    #[error("`{path}` cannot be read: {message}")]
    FileUnreadable { path: String, message: String },
    #[error("`{path}` is a link, and links are not sent.")]
    FileIsLink { path: String },
    #[error("`{path}` has no name to send.")]
    NoNameToSend { path: String },
    #[error("That is too many folders and files to look through.")]
    TooManyToLookThrough,
    #[error("`{name}` is more than {most}.")]
    FileOverSendLimit { name: String, most: String },
    #[error("That is more than {most} to send at once.")]
    SendOverBytes { most: String },
    #[error("That is more than {most} files to send at once.")]
    SendOverFiles { most: usize },
    #[error("There are no files in those paths.")]
    NoFilesInPaths,
    #[error("{error}")]
    WorkspaceNotOpened { error: String },
    #[error("Use /dream <profile#id|address>")]
    DreamUsage,
    #[error("That sign-in is not waiting for an answer.")]
    SignInNotWaiting,
    #[error("No sign-in of yours is open.")]
    NoSignIn,
    #[error(
        "`{name}` names no endpoint. A model is named \
         `endpoint::model`."
    )]
    NoEndpointNamed { name: String },
    #[error(
        "`{name}` is served by the endpoint `{endpoint}`, which the \
         settings do not have."
    )]
    NoSuchEndpoint { name: String, endpoint: String },
    #[error("A cost is a number of zero or more.")]
    BadCost,
    #[error("An image limit is a number above zero.")]
    BadImageLimit,
    #[error(
        "The provider of `{model}` lists its price and image \
         limits, so they cannot be changed."
    )]
    ModelListed { model: String },
    #[error("Use {usage}")]
    CommandUsage { usage: String },
    #[error(
        "There is no command /{name}; the client lists the commands."
    )]
    NoCommand { name: String },
    #[error("Say this to a hamlet's lead or one of its agents.")]
    SayToLeadOrAgent,
    #[error("Say what you need a skill for.")]
    NoSkillRequest,
    #[error(
        "`skill` is its folder under skills/, in letters, digits, \
         hyphens and underscores."
    )]
    BadSkillFolder,
    #[error(
        "The library has a skill named `{name}` already. Use it, or \
         offer yours under another name if it does something else."
    )]
    SkillInLibrary { name: String },
    #[error("Another offer of that skill is being judged.")]
    SkillBeingJudged,
    #[error(
        "skills/{name}/SKILL.md must open with a front matter \
         `name:` and `description:`."
    )]
    SkillFrontMatter { name: String },
    #[error("`{name}` holds more than the library takes, 4 MB.")]
    SkillTooBig { name: String },
    #[error("{why}")]
    WaitUnanswerable { why: String },
    #[error("The arguments of {tool} do not fit: {error}.")]
    ArgumentsUnfit { tool: String, error: String },
    #[error("{error}")]
    LookFailed { error: String },
    #[error("Only the current session of a chief's chat goes back.")]
    RewindNotCurrent,
    #[error("A name is one line of at most {most} letters.")]
    BadAgentName { most: usize },
    #[error("Say which hamlet to add an agent to.")]
    NoHamletToAddTo,
    #[error(
        "Give `profile` to add an agent of one in the city, or \
         `name` for a blank agent, not both."
    )]
    AddBoth,
    #[error("Reflect after zero or more turns.")]
    BadReflectEvery,
    #[error("A blank profile needs a character to start from.")]
    NoCharacter,
    #[error("{why}")]
    BadEndpoint { why: String },
    /// A change to the list of MCP servers that was not made.
    #[error("{why}")]
    BadMcp { why: String },
    #[error("{why}")]
    BadLimits { why: String },
    #[error("Those limits do not fit their form.")]
    LimitsUnfit,
    #[error("Those settings do not fit their form.")]
    SettingsUnfit,
    #[error("The mode is `manual` or `auto`.")]
    NoMode,
    #[error("There is no effort `{level}`.")]
    NoEffort { level: String },
    #[error("You cannot stop yourself.")]
    StopSelf,
    #[error("That is you.")]
    SendToSelf,
    #[error("Say what to send.")]
    NothingToSend,
    #[error(
        "You have no channel to {other}. Propose one to your parent \
         with propose_channel."
    )]
    NoChannelTo { other: String },
    #[error("That is not one of your lines in this conversation.")]
    NotYourLine,
    #[error("Choose a remote workspace in the settings first.")]
    NoWorkspaceChosen,
    #[error("Moving the worktrees failed: {error}")]
    MoveFailed { error: String },
    #[error("Every conversation of it was brought in before.")]
    BroughtInBefore,
    #[error("The conversation was not sent first.")]
    ConversationNotSent,
    #[error("A conversation brought in is at most {most} MiB.")]
    ConversationTooBig { most: u64 },
    #[error(
        "A project's conversations come to at most {most} MiB \
         together; bring in fewer at a time."
    )]
    ProjectTooBig { most: u64 },
    #[error("That file is not a conversation Hamlet can bring in.")]
    NotAConversation,
}
