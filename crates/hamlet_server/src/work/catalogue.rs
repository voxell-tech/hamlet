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
    CreateScholar,
    Hire,
    EndHire,
    MergeHires,
    SetModel,
    SetTools,
    SetAllowance,
    Nickname,
    LayRoad,
    TakeUpRoad,
    AgreeRoad,
    FoundHamlet,
    SeatMayor,
    RemoveHamlet,
    ReadRecord,
    ReadScholar,
    EditScholar,
    Tag,
    Revert,
}

/// Who is offered a society tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Offered {
    /// Every agent but a worker.
    Everyone,
    /// Every hire: those under a lead.
    Hires,
    /// Every lead over its own tree, and the Hand over all.
    Leads,
    /// A hamlet's mayor, while there is another hamlet.
    Mayors,
    /// Every agent but a worker, in a chief's chat.
    Chats,
    /// The Hand alone.
    Hand,
}

/// Which acts of the tool a chief keeps for itself (H22).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reserve {
    /// None.
    Free,
    /// The Hand's, on what chiefs own.
    Hand,
    /// A lead's, when its hamlet's limits keep this.
    Limit(Limit),
}

/// What a hamlet's limits may keep for its chief.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Limit {
    Hire,
    Roads,
    End,
    Merge,
}

impl Limit {
    /// Whether `reserved` keeps it.
    pub fn kept(self, reserved: &Reserved) -> bool {
        match self {
            Self::Hire => reserved.hire,
            Self::Roads => reserved.roads,
            Self::End => reserved.end,
            Self::Merge => reserved.merge,
        }
    }
}

/// What is known of a society tool.
pub(crate) struct Facts {
    pub tool: Society,
    /// The name a model calls it by.
    pub name: &'static str,
    pub offered: Offered,
    pub reserve: Reserve,
    /// What its agent is doing while it runs, when it says more than
    /// that a tool runs.
    pub doing: Option<Doing>,
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
    }
}

impl Facts {
    const fn reserved(mut self, reserve: Reserve) -> Self {
        self.reserve = reserve;
        self
    }

    const fn doing(mut self, doing: Doing) -> Self {
        self.doing = Some(doing);
        self
    }
}

/// Every society tool, once.
pub(crate) const FACTS: [Facts; 28] = {
    use Offered::{Chats, Everyone, Hand, Hires, Leads, Mayors};
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
        facts(S::RequestRoad, "request_road", Hires)
            .doing(Doing::Await),
        facts(S::CreateScholar, "create_scholar", Leads)
            .doing(Doing::Think),
        facts(S::Hire, "hire", Leads)
            .reserved(Reserve::Limit(Limit::Hire))
            .doing(Doing::Think),
        facts(S::EndHire, "end_hire", Leads)
            .reserved(Reserve::Limit(Limit::End)),
        facts(S::MergeHires, "merge_hires", Leads)
            .reserved(Reserve::Limit(Limit::Merge)),
        facts(S::SetModel, "set_model", Leads),
        facts(S::SetTools, "set_tools", Leads),
        facts(S::SetAllowance, "set_allowance", Leads),
        facts(S::Nickname, "nickname", Leads),
        facts(S::LayRoad, "lay_road", Leads)
            .reserved(Reserve::Limit(Limit::Roads)),
        facts(S::TakeUpRoad, "take_up_road", Leads)
            .reserved(Reserve::Limit(Limit::Roads)),
        facts(S::AgreeRoad, "agree_road", Mayors).doing(Doing::Await),
        facts(S::FoundHamlet, "found_hamlet", Hand),
        facts(S::SeatMayor, "seat_mayor", Hand),
        facts(S::RemoveHamlet, "remove_hamlet", Hand)
            .reserved(Reserve::Hand),
        facts(S::ReadRecord, "read_record", Hand),
        facts(S::ReadScholar, "read_scholar", Hand),
        facts(S::EditScholar, "edit_scholar", Hand)
            .reserved(Reserve::Hand),
        facts(S::Tag, "tag", Hand).reserved(Reserve::Hand),
        facts(S::Revert, "revert", Hand).reserved(Reserve::Hand),
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

    /// What is known of it.
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
        let mut specs = crate::calls::specs()
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
