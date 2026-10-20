//! Refusals on platforms without no-follow house file operations.

use super::*;

impl Service {
    pub(crate) fn prepare_gifts(&self) -> Result<()> {
        anyhow::bail!(
            "House files require a supported sandbox platform."
        )
    }

    fn gift_begin(
        &mut self,
        _owner: u64,
        _id: &str,
        _to: &Address,
        _files: &[GiveFilesFiles],
    ) -> Result<std::result::Result<Progress, Refusal>> {
        Ok(Err(Refusal::Unsupported("gifts on this platform")))
    }

    fn gift_part(
        &mut self,
        _owner: u64,
        _part: &GivePart,
    ) -> Result<std::result::Result<Progress, Refusal>> {
        Ok(Err(Refusal::Unsupported("gifts on this platform")))
    }

    pub(crate) fn gift_skill(
        &mut self,
        _to: &Address,
        _skill: &str,
        _from: Option<&Address>,
    ) -> Result<std::result::Result<String, Refusal>> {
        Ok(Err(Refusal::Unsupported("gifts on this platform")))
    }
}
