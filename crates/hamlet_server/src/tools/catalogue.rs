//! The tools of the society, offered to each agent by its role (H35).
//! A model names a tool with words; they are read into a [`Society`]
//! once, where the call arrives, and what is known of each tool is
//! said once, in [`FACTS`].

use hamlet_protocol::client::Doing;

use crate::society::limits::Reserved;

/// A tool of the society. A tool added by hand may not take the name
/// of one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Society {
    Tell,
    Ask,
    StartWorker,
    SubmitSkill,
    AskLibrarian,
    AskChief,
    ReadBudget,
    Object,
    RequestRoad,
    CreateProfile,
    AddAgent,
    EndAgent,
    MergeAgents,
    SetModel,
    SetTools,
    SetAllowance,
    RenameAgent,
    LayRoad,
    TakeUpRoad,
    AgreeRoad,
    FoundHamlet,
    SetLead,
    RemoveHamlet,
    ReadRecord,
    ReadProfile,
    EditProfile,
    Tag,
    Revert,
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
    Roads,
    End,
    Merge,
}

impl Limit {
    /// True when `reserved` keeps it.
    pub fn is_kept(self, reserved: &Reserved) -> bool {
        match self {
            Self::Add => reserved.add,
            Self::Roads => reserved.roads,
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
    /// that a tool runs.
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
pub(crate) const FACTS: [Facts; 28] = {
    use Offered::{
        Chats, Everyone, HamletAgents, Hand, Leads, Parents,
    };
    use Society as S;
    [
        facts(S::Tell, "tell", Everyone),
        facts(S::Ask, "ask", Everyone).doing(Doing::Await),
        facts(S::StartWorker, "start_worker", Everyone),
        facts(S::SubmitSkill, "submit_skill", Everyone)
            .doing(Doing::Await),
        facts(S::AskLibrarian, "ask_librarian", Everyone)
            .doing(Doing::Await),
        facts(S::AskChief, "ask_chief", Everyone),
        facts(S::ReadBudget, "read_budget", Everyone),
        facts(S::Object, "object", Chats),
        facts(S::RequestRoad, "request_road", HamletAgents)
            .doing(Doing::Await),
        facts(S::CreateProfile, "create_profile", Parents)
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
        facts(S::LayRoad, "lay_road", Parents)
            .reserved(Reserve::Limit(Limit::Roads)),
        facts(S::TakeUpRoad, "take_up_road", Parents)
            .reserved(Reserve::Limit(Limit::Roads)),
        facts(S::AgreeRoad, "agree_road", Leads).doing(Doing::Await),
        facts(S::FoundHamlet, "found_hamlet", Hand),
        facts(S::SetLead, "set_lead", Hand),
        facts(S::RemoveHamlet, "remove_hamlet", Hand)
            .reserved(Reserve::Hand),
        facts(S::ReadRecord, "read_record", Hand),
        facts(S::ReadProfile, "read_profile", Hand),
        facts(S::EditProfile, "edit_profile", Hand)
            .reserved(Reserve::Hand)
            .once(),
        facts(S::Tag, "tag", Hand).reserved(Reserve::Hand).once(),
        facts(S::Revert, "revert", Hand)
            .reserved(Reserve::Hand)
            .once(),
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
