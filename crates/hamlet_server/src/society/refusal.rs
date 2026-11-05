//! Why the society refuses a change, in words a chief or an agent
//! reads.

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
pub(crate) enum Refusal {
    #[error(
        "`{name}` is not a name: use at most 24 lowercase letters \
         and hyphens"
    )]
    BadName { name: String },
    #[error("the name `{name}` is reserved")]
    ReservedName { name: String },
    #[error("the name `{name}` belongs to a living scholar")]
    NameTaken { name: String },
    #[error(
        "`{id}` is not a hamlet's name: use at most 32 lowercase \
         letters, digits and hyphens"
    )]
    BadHamletId { id: String },
    #[error("the hamlet name `{id}` was used before; choose another")]
    HamletUsed { id: String },
    #[error("no hamlet is named `{id}`")]
    NoHamlet { id: String },
    #[error("the hamlet `{id}` was removed; restore it first")]
    HamletRemoved { id: String },
    #[error("the hamlet `{id}` is still living; remove it first")]
    HamletLiving { id: String },
    #[error("the hamlet `{id}` was erased")]
    HamletErased { id: String },
    #[error("the hamlet `{id}` holds its most hires, {most}")]
    HamletFull { id: String, most: i64 },
    #[error("a hamlet's tier is 50, 100, 200 or 500 hires")]
    BadTier,
    #[error("a hamlet's shown name may have at most 32 letters")]
    LongTitle,
    #[error("no scholar is named `{name}`")]
    NoScholar { name: String },
    #[error("`{name}` is another chief's scholar")]
    ScholarNotOwn { name: String },
    #[error("the scholar `{name}` no longer lives in the city")]
    ScholarGone { name: String },
    #[error("the scholar `{name}` is still living; remove it first")]
    ScholarLiving { name: String },
    #[error("the scholar `{name}` was erased")]
    ScholarErased { name: String },
    #[error("`{name}` still has hires that may come back: {hires}")]
    ScholarHasHires { name: String, hires: String },
    #[error("`{name}` has no version `{version}`")]
    NoVersion { name: String, version: String },
    #[error(
        "`{version}` is not a version's name: use at most 32 \
         letters, digits, dots and hyphens"
    )]
    BadVersion { version: String },
    #[error("a character may have at most 4000 characters")]
    LongCharacter,
    #[error("say what it is for")]
    NoPurpose,
    #[error("give a reason")]
    NoReason,
    #[error("only a chief of the hamlet `{hamlet}` may do that")]
    NotChief { hamlet: String },
    #[error("only `{owner}`'s chief may do that")]
    NotOwner { owner: String },
    #[error("only a lead of `{name}` may do that")]
    NotLead { name: String },
    #[error("only a mayor may do that")]
    NotMayor,
    #[error("a worker cannot do that")]
    Worker,
    #[error("`{name}` has left: {how}")]
    Left { name: String, how: String },
    #[error("`{name}` has not left")]
    NotLeft { name: String },
    #[error("`{name}` was erased")]
    Erased { name: String },
    #[error("a hamlet's tree is at most {most} deep under its mayor")]
    TooDeep { most: i64 },
    #[error("a mayor cannot be ended; seat another mayor first")]
    EndingMayor,
    #[error(
        "`{name}` is on a chief's task; a lead may end or merge it \
         once it has answered and its protected minutes have passed"
    )]
    OnChiefsTask { name: String },
    #[error("`{name}` is already the mayor")]
    AlreadyMayor { name: String },
    #[error("`{name}` is in another hamlet")]
    OtherHamlet { name: String },
    #[error("nothing is named `{address}`")]
    NoOne { address: String },
    #[error("`{address}` could be any of {hires}; name one")]
    Ambiguous { address: String, hires: String },
    #[error("`{address}` is not an address")]
    BadAddress { address: String },
    #[error("a road needs two different hires")]
    SameEnds,
    #[error("`{a}` and `{b}` already have a road")]
    RoadExists { a: String, b: String },
    #[error("`{a}` and `{b}` have no road to take up")]
    NoRoad { a: String, b: String },
    #[error("the hamlet `{hamlet}` has its most roads, {most}")]
    TooManyRoads { hamlet: String, most: i64 },
    #[error("the chief allows no roads between hamlets")]
    NoRoadsBetween,
    #[error(
        "both ends of an agreed road must be in different hamlets"
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
    #[error("the chief does not allow the model `{model}` here")]
    ModelNotAllowed { model: String },
    #[error(
        "the hamlet `{id}` holds {count} hires, more than {tier}"
    )]
    TierTooSmall { id: String, count: i64, tier: i64 },
    #[error("a road joins hires only")]
    RoadEnds,
    #[error("there is no past session {number} here")]
    NoSession { number: i64 },
    #[error("a turn is running here; stop it, then {then}")]
    TurnRunning { then: String },
    #[error(
        "the last message was answered, so there is nothing to \
         carry on"
    )]
    Answered,
    #[error("this server does not do that yet: {what}")]
    Unsupported { what: String },
    #[error("{words}")]
    Usage { words: String },
    #[error("only a chief of `{name}`'s hamlet may reach it")]
    NotYours { name: String },
    #[error("nothing waits here with that id")]
    NoWait,
    #[error("no model is named `{model}`")]
    NoModel { model: String },
    #[error("no skill is named `{name}`")]
    NoSkill { name: String },
    #[error("there is no file `{path}` here")]
    NoFile { path: String },
    #[error("that merge was split already")]
    AlreadySplit,
}
