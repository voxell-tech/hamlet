//! The company the chief's mascot keeps: the agent the chat is with
//! while it or its house is selected, a hamlet while its common is,
//! the library while it is, and home otherwise. A message the chief
//! sends while the mascot is at home brings it to that agent.

use glam::{Quat, Vec2, Vec3};
use hamlet_engine_bevy::Reply;
use hamlet_engine_bevy::api::agents::{
    AgentsEvent, AgentsReply, Entry, Focus, Recipient, Selected,
    SessionName,
};
use hamlet_engine_bevy::prelude::*;

use crate::bridge::Bridge;
use crate::folk::chief::chance;
use crate::folk::chief::wickstep::{Company, With};
use crate::folk::keeper::Robots;
use crate::places::library::Library;
use crate::society::{AgentId, Society};
use crate::world::land::Land;
use crate::world::layout::plot;

/// The spot of one who comes to the library, from its middle: at the
/// foot of the steps, to the side the librarian is not on.
const VISITOR: Vec3 = Vec3::new(-1.7, 0.0, 7.3);

/// The front of a house its agent is found at while it has no robot,
/// out from the house's middle.
const HOUSE_FRONT: f32 = 3.2;

/// The company the chief's mascot keeps, and what of the window's
/// focus it follows.
#[derive(Resource, Default)]
pub struct Keeping {
    pub with: Option<Company>,
    selected: Option<Selected>,
    talking: Option<Recipient>,
    chief: String,
    open: Option<SessionName>,
    /// The last message of the chief's seen in the open session.
    said: Option<u64>,
}

impl Keeping {
    /// Follows `focus`: the company changes with what is selected
    /// and with whom the chat is with.
    fn focus(
        &mut self,
        focus: &Focus,
        agent: Option<AgentId>,
        bridge: &Bridge,
    ) {
        self.chief.clone_from(&focus.chief);
        if self.open.as_ref() != Some(&focus.open.name) {
            self.open = Some(focus.open.name.clone());
            self.said = None;
        }
        let selected = Some(focus.selected.clone());
        let talking = Some(focus.with.clone());
        if self.selected == selected && self.talking == talking {
            return;
        }
        self.selected = selected;
        self.talking = talking;
        self.with = match &focus.selected {
            Selected::Library => Some(Company::Library),
            Selected::Agent(_) => agent.map(Company::Agent),
            Selected::Hamlet(hamlet) => {
                bridge.town(hamlet).map(Company::Common)
            }
            Selected::Hearth | Selected::Nothing => None,
        };
    }
}

/// The agent of the society `with` is drawn as.
fn talking_to(
    with: &Recipient,
    bridge: &Bridge,
    society: &Society,
) -> Option<AgentId> {
    match with {
        Recipient::Agent(name) => bridge.find(name),
        Recipient::Mayor(hamlet) => {
            let town = bridge.town(hamlet)?;
            society.town(town).map(|town| town.leader)
        }
    }
}

/// Follows the window's focus and the chief's messages for the
/// company the mascot keeps.
pub fn follow_company(
    mut replies: MessageReader<Replied>,
    mut events: MessageReader<AgentsEventReceived>,
    bridge: Res<Bridge>,
    society: Res<Society>,
    mut keeping: ResMut<Keeping>,
) {
    for replied in replies.read() {
        if let Ok(Reply::Agents(AgentsReply::Focus(focus))) =
            &replied.reply
        {
            let agent = talking_to(&focus.with, &bridge, &society);
            keeping.focus(focus, agent, &bridge);
        }
    }
    for event in events.read() {
        match &event.event {
            AgentsEvent::Focus(focus) => {
                let agent =
                    talking_to(&focus.with, &bridge, &society);
                keeping.focus(focus, agent, &bridge);
            }
            AgentsEvent::Said {
                session,
                messages,
                again: false,
            } if keeping.open.as_ref() == Some(session) => {
                let chief = keeping.chief.clone();
                let sent = messages
                    .iter()
                    .filter(|message| {
                        matches!(&message.entry, Entry::User { user, .. } if *user == chief)
                    })
                    .map(|message| message.id)
                    .filter(|id| keeping.said.is_none_or(|said| *id > said))
                    .max();
                let Some(sent) = sent else {
                    continue;
                };
                keeping.said = Some(sent);
                if keeping.with.is_none()
                    && let Some(agent) =
                        keeping.talking.as_ref().and_then(|with| {
                            talking_to(with, &bridge, &society)
                        })
                {
                    keeping.with = Some(Company::Agent(agent));
                }
            }
            _ => {}
        }
    }
}

/// Where the mascot stands by `company` at clock time `now`, `stroll`
/// the reach and the pause of its walk about a common.
pub fn whereabouts(
    company: Company,
    now: f32,
    stroll: (f32, f32),
    (land, society): (&Land, &Society),
    robots: &Robots,
    library: &Library,
) -> Option<With> {
    match company {
        Company::Agent(agent) => {
            // Its robot, or the front of its house while it has none.
            let at =
                robots.at(agent).or_else(|| {
                    let agent = society.agent(agent)?;
                    if agent.archived {
                        return None;
                    }
                    let town = society.town(agent.town)?;
                    let spot = plot(land, town.site, agent.plot);
                    Some(land.on_ground(
                        spot.at + spot.facing() * HOUSE_FRONT,
                    ))
                })?;
            Some(With {
                company,
                at,
                gaze: None,
            })
        }
        Company::Common(town) => {
            // It stands at one spot about the common and then
            // another, as one who looks the place over, each a short
            // walk on from the last so that it goes round the well.
            let middle = land.sites[society.town(town)?.site].at;
            let (reach, pause) = stroll;
            let turn = (now / pause).floor();
            let seed = (town.0 % 97) as f32;
            let angle = seed + turn * 0.9 + chance(turn * 1.3) * 0.2;
            let spot = middle + Vec2::from_angle(angle) * reach;
            Some(With {
                company,
                at: Vec3::new(spot.x, 0.0, spot.y),
                gaze: Some(middle),
            })
        }
        Company::Library => {
            // At the foot of its steps. Its door looks toward the
            // first town.
            let at = library.at()?;
            let first = land
                .sites
                .iter()
                .find(|site| site.buildable)
                .map_or(Vec2::ZERO, |site| site.at);
            let toward =
                (first - Vec2::new(at.x, at.z)).normalize_or(Vec2::Y);
            let turn =
                Quat::from_rotation_y(toward.x.atan2(toward.y));
            Some(With {
                company,
                at: at + turn * VISITOR,
                gaze: None,
            })
        }
    }
}
