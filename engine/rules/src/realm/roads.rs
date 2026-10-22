//! Roads between hamlets (H14): one between each two mayors, and
//! those both mayors agreed to.

use anyhow::{Result, bail};
use hamlet_model::log;
use serde_json::{Value, json};

use crate::hamlet::{MAYOR, Road};
use crate::realm::{Realm, office};
use crate::record::text;

impl Realm {
    /// Every road that crosses hamlets: one between each two
    /// mayors, and those both mayors agreed to.
    pub fn roads(&self) -> Vec<Road> {
        let events = self.events();
        let (live, _) = self.places_in(&events);
        let everyone = self.everyone_in(&live);
        let home_of = |address: &str| match address
            .strip_prefix(&format!("{MAYOR}@"))
        {
            Some(hamlet) => live
                .iter()
                .any(|p| p.name == hamlet)
                .then(|| hamlet.to_string()),
            None => everyone
                .iter()
                .find(|(r, _)| r.name == address)
                .map(|(_, hamlet)| hamlet.clone()),
        };
        let mut roads = Vec::<Road>::new();
        for (i, a) in live.iter().enumerate() {
            for b in &live[i + 1..] {
                roads.push(Road::new(
                    &office(&a.name),
                    &office(&b.name),
                ));
            }
        }
        // Each agreed road with the hamlets whose mayors agreed.
        let mut agreed = Vec::<(Road, Vec<String>)>::new();
        for e in &events {
            match e["type"].as_str() {
                Some("road_agreed") => {
                    let (a, b) = (text(e, "a"), text(e, "b"));
                    let by = text(e, "hamlet");
                    match agreed
                        .iter_mut()
                        .find(|(r, _)| r.joins(&a, &b))
                    {
                        Some((_, hamlets)) => hamlets.push(by),
                        None => {
                            agreed.push((Road::new(&a, &b), vec![by]))
                        }
                    }
                }
                Some("agent_moved") => {
                    // Agreements about an agent are about where it
                    // lived then.
                    let agent = text(e, "agent");
                    agreed.retain(|(r, _)| !r.has(&agent));
                }
                Some("message") => {
                    let (from, to) = (text(e, "from"), text(e, "to"));
                    let ts = e["ts"].as_u64().unwrap_or_default();
                    let road = roads
                        .iter_mut()
                        .chain(agreed.iter_mut().map(|(r, _)| r))
                        .find(|r| r.joins(&from, &to));
                    if let Some(road) = road {
                        road.carry(&from, ts);
                    }
                }
                _ => {}
            }
        }
        for (road, hamlets) in agreed {
            let (Some(a), Some(b)) =
                (home_of(&road.ends.0), home_of(&road.ends.1))
            else {
                continue;
            };
            if a != b && hamlets.contains(&a) && hamlets.contains(&b)
            {
                roads.push(road);
            }
        }
        roads
    }

    pub fn has_road(&self, a: &str, b: &str) -> bool {
        a != b && self.roads().iter().any(|r| r.joins(a, b))
    }

    /// Notes a message carried between hamlets. Whoever asks pays
    /// (H14): `payer` is the sender's hamlet.
    pub fn carried(
        &self,
        from: &str,
        to: &str,
        payer: &str,
    ) -> Result<()> {
        self.append(json!({
            "v": 1,
            "type": "message",
            "ts": log::now_millis(),
            "from": from,
            "to": to,
            "payer": payer,
        }))
    }

    /// Messages between hamlets that `hamlet` has paid for.
    pub fn paid(&self, hamlet: &str) -> usize {
        self.events()
            .iter()
            .filter(|e| {
                e["type"] == "message" && e["payer"] == hamlet
            })
            .count()
    }

    /// The hamlets whose mayors have agreed to a road between `a`
    /// and `b` since either last moved.
    pub(super) fn road_agreements(
        events: &[Value],
        a: &str,
        b: &str,
    ) -> Vec<String> {
        let probe = Road::new(a, b);
        let mut hamlets = Vec::new();
        for e in events {
            match e["type"].as_str() {
                Some("road_agreed")
                    if probe.joins(&text(e, "a"), &text(e, "b")) =>
                {
                    hamlets.push(text(e, "hamlet"))
                }
                Some("agent_moved")
                    if probe.has(&text(e, "agent")) =>
                {
                    hamlets.clear()
                }
                _ => {}
            }
        }
        hamlets
    }

    /// The mayor of `hamlet` agrees to a road between `a` and `b`,
    /// agents of different hamlets. True once both mayors have agreed
    /// and the road exists (H14).
    pub fn agree_road(
        &self,
        hamlet: &str,
        a: &str,
        b: &str,
        reason: &str,
    ) -> Result<bool> {
        if !self.rules.roads_between_hamlets {
            bail!(
                "the chief's limits allow no roads between hamlets"
            );
        }
        let home = |name: &str| {
            if name.contains('@') || name == MAYOR {
                bail!(
                    "name two agents; mayors already have roads to \
                     each other"
                );
            }
            self.hamlet_of(name).ok_or_else(|| {
                anyhow::anyhow!("no agent named `{name}`")
            })
        };
        let (at_a, at_b) = (home(a)?, home(b)?);
        if at_a == at_b {
            bail!(
                "`{a}` and `{b}` live in the same hamlet; its mayor \
                 builds that road with build_road"
            );
        }
        if hamlet != at_a && hamlet != at_b {
            bail!("neither `{a}` nor `{b}` lives in your hamlet");
        }
        if self.has_road(a, b) {
            bail!("`{a}` and `{b}` already have a road");
        }
        let agreed = Realm::road_agreements(&self.events(), a, b);
        if agreed.iter().any(|h| h == hamlet) {
            bail!(
                "you have agreed already; the road waits for the \
                 other mayor"
            );
        }
        self.append(json!({
            "v": 1,
            "type": "road_agreed",
            "ts": log::now_millis(),
            "a": a,
            "b": b,
            "hamlet": hamlet,
            "reason": reason.trim(),
        }))?;
        Ok(self.has_road(a, b))
    }
}
