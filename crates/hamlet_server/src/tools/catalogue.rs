//! The tools of the society, offered to each agent by its role (H35).

use hamlet_protocol::client::Doing;

use crate::society::limits::Reserved;

/// A tool of the society.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Society {
    Notify,
    Request,
    StartWorker,
    SubmitSkill,
    RequestLibrarian,
    RequestChief,
    RequestMcpServer,
    ReadBudget,
    Object,
    ProposeFolder,
    CreateProfile,
    MakeProfile,
    AddAgent,
    EndAgent,
    MergeAgents,
    SetModel,
    SetTools,
    SetAllowance,
    RenameAgent,
    OpenChannel,
    CloseChannel,
    AgreeChannel,
    FoundHamlet,
    SetLead,
    ReparentAgent,
    RemoveHamlet,
    RestoreHamlet,
    EraseHamlet,
    NameHamlet,
    MoveHamlet,
    SetTier,
    SetFolder,
    ResetBudget,
    EditLimits,
    ReadRecord,
    ReadProfile,
    EditProfile,
    Tag,
    Revert,
    Versions,
    Fork,
    Purge,
    Welcome,
    TurnAway,
    NameProfile,
    MoveHouse,
    RemoveProfile,
    RestoreProfile,
    EraseProfile,
    Pin,
    RestoreAgent,
    EraseAgent,
    SetEffort,
    SetFast,
    Dream,
    Reflect,
    Stop,
    Resume,
    EditMemory,
    ReadPrompt,
    ReadPapers,
    EditSettings,
    EditCharter,
    ReadCharter,
    ReadModels,
    Unshelve,
}

/// The agents offered a society tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Offered {
    /// Every agent but a worker.
    Everyone,
    /// Every agent of a hamlet: those under a parent.
    HamletAgents,
    /// Every parent over its own tree, and the Hand over all.
    Parents,
    /// A hamlet's lead, while there is another hamlet.
    Leads,
    /// Every agent but a worker, in a chief's chat.
    Chats,
    /// The Hand alone.
    Hand,
}

/// The acts of the tool a chief keeps for itself (H22).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reserve {
    /// None.
    Free,
    /// The Hand's, on what chiefs own.
    Hand,
    /// A parent's, when its hamlet's limits keep this.
    Limit(Limit),
}

/// An act a hamlet's limits may keep for its chief.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Limit {
    Add,
    Channels,
    End,
    Merge,
}

impl Limit {
    /// True when `reserved` keeps it.
    pub fn is_kept(self, reserved: &Reserved) -> bool {
        match self {
            Self::Add => reserved.add,
            Self::Channels => reserved.channels,
            Self::End => reserved.end,
            Self::Merge => reserved.merge,
        }
    }
}

/// The facts of a society tool.
pub(crate) struct Facts {
    pub tool: Society,
    /// The name a model calls it by.
    pub name: &'static str,
    pub offered: Offered,
    pub reserve: Reserve,
    /// Its agent's doing while it runs, when that says more than
    /// that it thinks.
    pub doing: Option<Doing>,
    /// True when a call cut off by a crash may run again when called
    /// again: its change is kept with its answer, or what it began
    /// is found by its call. A write of a profile's repository is
    /// kept by no transaction, so it is answered as cut off instead.
    pub replays: bool,
}

const fn facts(
    tool: Society,
    name: &'static str,
    offered: Offered,
) -> Facts {
    Facts {
        tool,
        name,
        offered,
        reserve: Reserve::Free,
        doing: None,
        replays: true,
    }
}

impl Facts {
    const fn reserved(mut self, reserve: Reserve) -> Self {
        self.reserve = reserve;
        self
    }

    /// It writes a profile's repository, which no transaction keeps.
    const fn once(mut self) -> Self {
        self.replays = false;
        self
    }

    const fn doing(mut self, doing: Doing) -> Self {
        self.doing = Some(doing);
        self
    }
}

/// Every society tool, once.
pub(crate) const FACTS: [Facts; 66] = {
    use Offered::{
        Chats, Everyone, HamletAgents, Hand, Leads, Parents,
    };
    use Society as S;
    [
        facts(S::Notify, "notify", Everyone),
        facts(S::Request, "request", Everyone).doing(Doing::Await),
        facts(S::StartWorker, "start_worker", Everyone),
        facts(S::SubmitSkill, "submit_skill", Everyone)
            .doing(Doing::Await),
        facts(S::RequestLibrarian, "request_librarian", Everyone)
            .doing(Doing::Await),
        facts(S::RequestChief, "request_chief", Everyone),
        facts(S::RequestMcpServer, "request_mcp_server", Everyone),
        facts(S::ReadBudget, "read_budget", Everyone),
        facts(S::Object, "object", Chats),
        facts(S::ProposeFolder, "propose_folder", HamletAgents)
            .doing(Doing::Await),
        facts(S::CreateProfile, "create_profile", Parents)
            .doing(Doing::Think),
        facts(S::MakeProfile, "make_profile", Parents)
            .doing(Doing::Think),
        facts(S::AddAgent, "add_agent", Parents)
            .reserved(Reserve::Limit(Limit::Add))
            .doing(Doing::Think),
        facts(S::EndAgent, "end_agent", Parents)
            .reserved(Reserve::Limit(Limit::End)),
        facts(S::MergeAgents, "merge_agents", Parents)
            .reserved(Reserve::Limit(Limit::Merge)),
        facts(S::SetModel, "set_model", Parents),
        facts(S::SetTools, "set_tools", Parents),
        facts(S::SetAllowance, "set_allowance", Parents),
        facts(S::RenameAgent, "rename_agent", Parents),
        facts(S::ReparentAgent, "reparent_agent", Parents),
        facts(S::OpenChannel, "open_channel", Parents)
            .reserved(Reserve::Limit(Limit::Channels)),
        facts(S::CloseChannel, "close_channel", Parents)
            .reserved(Reserve::Limit(Limit::Channels)),
        facts(S::AgreeChannel, "agree_channel", Leads)
            .doing(Doing::Await),
        facts(S::FoundHamlet, "found_hamlet", Hand),
        facts(S::SetLead, "set_lead", Hand),
        facts(S::RemoveHamlet, "remove_hamlet", Hand)
            .reserved(Reserve::Hand),
        facts(S::RestoreHamlet, "restore_hamlet", Hand),
        facts(S::EraseHamlet, "erase_hamlet", Hand)
            .reserved(Reserve::Hand),
        facts(S::NameHamlet, "name_hamlet", Hand),
        facts(S::MoveHamlet, "move_hamlet", Hand),
        facts(S::SetTier, "set_tier", Hand).reserved(Reserve::Hand),
        facts(S::SetFolder, "set_folder", Hand)
            .reserved(Reserve::Hand),
        facts(S::ResetBudget, "reset_budget", Hand)
            .reserved(Reserve::Hand),
        facts(S::EditLimits, "edit_limits", Hand)
            .reserved(Reserve::Hand)
            .once(),
        facts(S::ReadRecord, "read_record", Hand),
        facts(S::ReadProfile, "read_profile", Hand),
        facts(S::EditProfile, "edit_profile", Hand)
            .reserved(Reserve::Hand)
            .once(),
        facts(S::Tag, "tag", Hand).reserved(Reserve::Hand).once(),
        facts(S::Revert, "revert", Hand)
            .reserved(Reserve::Hand)
            .once(),
        facts(S::Versions, "versions", Hand),
        facts(S::Fork, "fork", Hand),
        facts(S::Purge, "purge", Hand)
            .reserved(Reserve::Hand)
            .once(),
        facts(S::Welcome, "welcome", Hand),
        facts(S::TurnAway, "turn_away", Hand),
        facts(S::NameProfile, "name_profile", Hand),
        facts(S::MoveHouse, "move_house", Hand),
        facts(S::RemoveProfile, "remove_profile", Hand)
            .reserved(Reserve::Hand),
        facts(S::RestoreProfile, "restore_profile", Hand),
        facts(S::EraseProfile, "erase_profile", Hand)
            .reserved(Reserve::Hand),
        facts(S::Pin, "pin", Hand),
        facts(S::RestoreAgent, "restore_agent", Hand),
        facts(S::EraseAgent, "erase_agent", Hand)
            .reserved(Reserve::Hand),
        facts(S::SetEffort, "set_effort", Hand),
        facts(S::SetFast, "set_fast", Hand),
        facts(S::Dream, "dream", Hand).once(),
        facts(S::Reflect, "reflect", Hand),
        facts(S::Stop, "stop", Hand),
        facts(S::Resume, "resume", Hand),
        facts(S::EditMemory, "edit_memory", Hand).once(),
        facts(S::ReadPrompt, "read_prompt", Hand),
        facts(S::ReadPapers, "read_papers", Hand),
        facts(S::EditSettings, "edit_settings", Hand)
            .reserved(Reserve::Hand)
            .once(),
        facts(S::EditCharter, "edit_charter", Hand)
            .reserved(Reserve::Hand)
            .once(),
        facts(S::ReadCharter, "read_charter", Hand),
        facts(S::ReadModels, "read_models", Hand),
        facts(S::Unshelve, "unshelve", Hand).once(),
    ]
};

impl Society {
    /// The society tool a model calls `name`, if it is one.
    pub fn named(name: &str) -> Option<Self> {
        FACTS
            .iter()
            .find(|facts| facts.name == name)
            .map(|f| f.tool)
    }

    /// Its [`Facts`].
    pub fn facts(self) -> &'static Facts {
        FACTS
            .iter()
            .find(|facts| facts.tool == self)
            .expect("every society tool has its facts")
    }

    /// The name a model calls it by.
    pub fn name(self) -> &'static str {
        self.facts().name
    }

    /// The names of the tools `offered` to whom.
    pub fn offered(offered: Offered) -> impl Iterator<Item = String> {
        FACTS
            .iter()
            .filter(move |facts| facts.offered == offered)
            .map(|facts| facts.name.to_owned())
    }

    /// Every society tool's name.
    pub fn names() -> impl Iterator<Item = &'static str> {
        FACTS.iter().map(|facts| facts.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_specs_offer_every_society_tool_and_no_other() {
        let mut specs = crate::calls::specs::specs()
            .into_iter()
            .map(|spec| spec.name)
            .collect::<Vec<_>>();
        let mut named =
            Society::names().map(String::from).collect::<Vec<_>>();
        specs.sort();
        named.sort();
        assert_eq!(specs, named);
    }

    #[test]
    fn every_society_tool_has_one_name_and_its_facts() {
        for (index, facts) in FACTS.iter().enumerate() {
            assert_eq!(Society::named(facts.name), Some(facts.tool));
            assert!(
                FACTS[index + 1..].iter().all(|other| {
                    other.name != facts.name
                        && other.tool != facts.tool
                }),
                "{} is listed twice",
                facts.name
            );
        }
    }
}
