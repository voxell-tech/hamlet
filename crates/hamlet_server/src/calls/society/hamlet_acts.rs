//! The Hand's acts on a hamlet: its name, its place, its removal,
//! restoring and erasure, its tier, folder, mode, budget and limits.
//! A reserved act waits for a chief before it gets here.

use hamlet_protocol::client::LimitsEdit;
use hamlet_store::ids::{Actor, HamletId};

use super::Call;
use super::args::{
    Called, EditLimits, EraseHamlet, MoveHamlet, NameHamlet,
    RemoveHamlet, ResetBudget, RestoreHamlet, SetFolder, SetMode,
    SetTier,
};
use crate::clients::documents::{LimitsOwner, with_limit};
use crate::core::Core;
use crate::realm::change::{Done, refuse};
use crate::society::hamlets::{self, Mode};
use crate::society::refusal::Refusal;
use crate::society::{authority, spend};

impl Core<'_, '_> {
    /// Carries out the Hand's call `called` on a hamlet.
    pub(super) fn hamlet_tool(
        &mut self,
        call: &Call<'_>,
        called: Called,
    ) -> Done<String> {
        let by = Actor::Agent(call.conversation.agent);
        let named = match &called {
            Called::EditLimits(args) => {
                return self.edit_limits(call, by, args);
            }
            Called::ResetBudget(ResetBudget { hamlet })
                if hamlet.is_empty() =>
            {
                return self.reset_budget(call, None);
            }
            Called::RemoveHamlet(RemoveHamlet { hamlet, .. })
            | Called::RestoreHamlet(RestoreHamlet {
                hamlet, ..
            })
            | Called::EraseHamlet(EraseHamlet { hamlet })
            | Called::NameHamlet(NameHamlet { hamlet, .. })
            | Called::MoveHamlet(MoveHamlet { hamlet, .. })
            | Called::SetTier(SetTier { hamlet, .. })
            | Called::SetFolder(SetFolder { hamlet, .. })
            | Called::SetMode(SetMode { hamlet, .. })
            | Called::ResetBudget(ResetBudget { hamlet }) => {
                hamlet.clone()
            }
            _ => refuse!(Refusal::Unsupported {
                what: format!("the tool {}", call.tool),
            }),
        };
        let id = HamletId(named.clone());
        let db = self.realm.store().connection();
        if hamlets::find(db, &id)?.is_none() {
            refuse!(Refusal::NoHamlet { id: named });
        }
        if !authority::may_govern(db, by, &id)? {
            refuse!(Refusal::NotChief { hamlet: named });
        }
        match called {
            Called::NameHamlet(NameHamlet { name, .. }) => self
                .carry(call, |cx, _| {
                    Ok(hamlets::rename(cx, &id, &name)?.map(|()| {
                        format!("`{id}` is named {name} now.")
                    }))
                }),
            Called::MoveHamlet(MoveHamlet { x, z, .. }) => self
                .carry(call, |cx, _| {
                    Ok(hamlets::place(cx, &id, x, z)?.map(|()| {
                        format!("`{id}` is set down at {x}, {z}.")
                    }))
                }),
            Called::SetTier(SetTier { tier, .. }) => {
                self.carry(call, |cx, _| {
                    Ok(hamlets::set_tier(cx, &id, i64::from(tier))?
                        .map(|()| {
                            format!(
                                "`{id}` holds at most {tier} hires."
                            )
                        }))
                })
            }
            Called::SetFolder(SetFolder { folder, .. }) => {
                let folder = (!folder.is_empty()).then_some(folder);
                if let Some(folder) = &folder
                    && let Err(refusal) =
                        hamlets::check_folder(folder)
                {
                    refuse!(refusal);
                }
                self.carry(call, |cx, _| {
                    Ok(hamlets::set_folder(
                        cx,
                        &id,
                        folder.as_deref(),
                    )?
                    .map(|()| match &folder {
                        Some(folder) => {
                            format!("`{id}` works on {folder}.")
                        }
                        None => {
                            format!("`{id}` works on no folder.")
                        }
                    }))
                })
            }
            Called::SetMode(SetMode { mode, .. }) => {
                let mode = match mode.as_str() {
                    "manual" => Mode::Manual,
                    "auto" => Mode::Auto,
                    _ => refuse!(Refusal::NoMode),
                };
                self.carry(call, |cx, _| {
                    Ok(hamlets::set_mode(cx, &id, mode)?.map(|()| {
                        format!("`{id}` is in {mode} mode.")
                    }))
                })
            }
            Called::ResetBudget(_) => {
                self.reset_budget(call, Some(id))
            }
            Called::RemoveHamlet(RemoveHamlet { reason, .. }) => self
                .carry(call, |cx, _| {
                    Ok(hamlets::remove(cx, &id, &reason)?.map(|()| {
                        format!(
                            "`{id}` was removed and its hires ended."
                        )
                    }))
                }),
            Called::RestoreHamlet(RestoreHamlet {
                reason, ..
            }) => self.carry(call, |cx, _| {
                Ok(hamlets::restore(cx, &id, &reason)?
                    .map(|()| format!("`{id}` is restored.")))
            }),
            _ => self.carry(call, |cx, _| {
                Ok(hamlets::erase(cx, &id)?
                    .map(|()| format!("`{id}` was erased for good.")))
            }),
        }
    }

    /// Starts the budget period of `hamlet`, or the realm's, again.
    fn reset_budget(
        &mut self,
        call: &Call<'_>,
        hamlet: Option<HamletId>,
    ) -> Done<String> {
        // The chief who allowed it is the one this server serves.
        let chief = self.realm.chief();
        self.carry(call, |cx, _| {
            spend::reset(cx.db, hamlet.as_ref(), chief, cx.at)?;
            Ok(
                Ok("The budget's period starts again from now."
                    .into()),
            )
        })
    }

    /// Sets one limit of a hamlet, or of the realm.
    fn edit_limits(
        &mut self,
        call: &Call<'_>,
        by: Actor,
        args: &EditLimits,
    ) -> Done<String> {
        let set = LimitsEdit {
            section: args.section.clone(),
            key: args.key.clone(),
            value: args.value.clone(),
        };
        self.begin(call)?;
        let hamlet =
            (!args.hamlet.is_empty()).then(|| args.hamlet.clone());
        self.set_limits(by, LimitsOwner::of(hamlet), |current| {
            Ok(with_limit(current, &set))
        })
    }
}
