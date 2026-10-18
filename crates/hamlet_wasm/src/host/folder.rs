//! The files a plugin keeps in its own folder, each call needing a
//! grant.

use hamlet_sdk::capability::Capability;

use super::State;
use crate::files::Files;

impl State {
    pub(super) fn list_files(
        &mut self,
    ) -> Result<Vec<String>, String> {
        self.require(Capability::FilesRead)?;
        Ok(self.folder()?.list())
    }

    pub(super) fn read_file(
        &mut self,
        path: &str,
    ) -> Result<Vec<u8>, String> {
        self.require(Capability::FilesRead)?;
        self.folder()?.read(path)
    }

    pub(super) fn write_file(
        &mut self,
        path: &str,
        data: &[u8],
    ) -> Result<(), String> {
        self.require(Capability::FilesWrite)?;
        self.folder()?.write(path, data)
    }

    pub(super) fn delete_file(
        &mut self,
        path: &str,
    ) -> Result<(), String> {
        self.require(Capability::FilesWrite)?;
        self.folder()?.delete(path)
    }

    fn folder(&self) -> Result<&Files, String> {
        self.files.as_ref().ok_or("there is no folder".into())
    }
}
