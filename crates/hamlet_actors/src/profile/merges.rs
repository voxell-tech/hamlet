//! One hire's branch merged into another's and that merge taken out
//! again, a branch's work read out in parts for a merge from another
//! profile, and a branch dropped.

use anyhow::{Result, bail};
use hamlet_messages::profile::{
    Bundle, DropBranch, MergeBranch, Unmerge,
};

use super::Profile;
use crate::repo::{Call, Reply, Takes};

macro_rules! unbuilt {
    ($($message:ty),+) => {$(
        impl Takes<$message> for Profile {
            async fn take(
                _call: Call<Self, $message>,
            ) -> Result<Reply<$message>> {
                bail!("not implemented")
            }
        }
    )+};
}

unbuilt!(MergeBranch, Bundle, Unmerge, DropBranch);
