//! The Hand's acts on a hamlet.

use hamlet_store::ids::{Actor, HamletId};

use super::args::{
    Called, EditLimits, EraseHamlet, MoveHamlet, NameHamlet,
    RemoveHamlet, ResetBudget, RestoreHamlet, SetFolder, SetTier,
};
use super::{Call, unsupported};
use crate::core::Core;
use crate::realm::change::{Done, checked, reject};
use crate::society::limits::{self, Limit};
use crate::society::rejection::Rejection;
use crate::society::{authority, hamlets, spend};

impl Core<'_, '_> {
    pub(super) fn hamlet_tool(
        &mut self,
        call: &Call<'_>,
        called: Called,
    ) -> Done<String> {
        let by = Actor::Agent(call.conversation.agent);
        let named = match &called {
            Called::EditLimits(args) => {
                return self.edit_limits(call, args);
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
            | Called::ResetBudget(ResetBudget { hamlet }) => {
                hamlet.clone()
            }
            _ => reject!(unsupported(call)),
        };
        let id = HamletId(named.clone());
        let db = self.realm.store().connection();
        if hamlets::find(db, &id)?.is_none() {
            reject!(Rejection::NoHamlet { id: named });
        }
        if !authority::may_govern(db, by, &id)? {
            reject!(Rejection::NotChief { hamlet: named });
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
                                "`{id}` holds at most {tier} agents."
                            )
                        }))
                })
            }
            Called::SetFolder(SetFolder { folder, .. }) => {
                let folder = (!folder.is_empty()).then_some(folder);
                if let Some(folder) = &folder
                    && let Err(rejection) =
                        hamlets::check_folder(folder)
                {
                    reject!(rejection);
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
            Called::ResetBudget(_) => {
                self.reset_budget(call, Some(id))
            }
            Called::RemoveHamlet(RemoveHamlet { reason, .. }) => self
                .carry(call, |cx, _| {
                    Ok(hamlets::remove(cx, &id, &reason)?.map(|()| {
                        format!(
                            "`{id}` was removed and its agents \
                             ended."
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

    fn edit_limits(
        &mut self,
        call: &Call<'_>,
        args: &EditLimits,
    ) -> Done<String> {
        let limit = checked!(Limit::read(
            &args.section,
            &args.key,
            args.value.clone(),
        ));
        let hamlet = (!args.hamlet.is_empty())
            .then(|| HamletId(args.hamlet.clone()));
        // The chief who allowed it is the one this server serves.
        let chief = self.realm.chief();
        self.carry(call, |cx, _| {
            Ok(limits::set_one(cx, chief, hamlet.as_ref(), limit)?
                .map(|()| limits::saved(hamlet.as_ref())))
        })
    }
}
