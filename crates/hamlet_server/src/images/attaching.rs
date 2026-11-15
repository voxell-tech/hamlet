//! The images and files a chief's line carries to an agent (C33,
//! S29): each at its place in the words, saved in the agent's own
//! folders, with the captions that say where.

use anyhow::Result;
use hamlet_protocol::client::{AttachedFile, AttachedImage};
use hamlet_store::ids::AgentId;

use super::kept::Images;
use super::{Limits, SEEN};
use crate::attached::{Attached, Caption, is_place};
use crate::core::{Core, UnpushedLodge};
use crate::files::Files;
use crate::inbox::INBOX;
use crate::society::refusal::Refusal;
use crate::talk::models;
use crate::workspace::lodges::LODGE;

impl Core<'_, '_> {
    /// The images `images` and the files `files` of the line `text`
    /// to `agent`, each at its place in the words, with the captions
    /// that tell where they are saved (C33). The images go to the
    /// model too, if it takes them (S30).
    pub(crate) fn attach(
        &mut self,
        agent: AgentId,
        text: &str,
        images: &[AttachedImage],
        files: &[AttachedFile],
    ) -> Result<std::result::Result<Attached, Refusal>> {
        let places = images
            .iter()
            .filter_map(|image| image.at)
            .chain(files.iter().filter_map(|file| file.at));
        for at in places {
            if !is_place(text, at) {
                return Ok(Err(Refusal::BadAttachmentPlace {
                    at,
                    bytes: text.len(),
                }));
            }
        }
        if let Err(refusal) = self.check_files(files) {
            return Ok(Err(refusal));
        }
        let mut images = images.to_vec();
        images.sort_by_key(|image| image.at.unwrap_or(u64::MAX));
        let mut files = files.to_vec();
        files.sort_by_key(|file| file.at.unwrap_or(u64::MAX));
        let mut attached = match self.attach_images(agent, &images)? {
            Ok(attached) => attached,
            Err(refusal) => return Ok(Err(refusal)),
        };
        if files.is_empty() {
            return Ok(Ok(attached));
        }
        let kept = Files::new(self.realm.layout().files());
        let mut inbox = self.open_inbox(agent, INBOX)?;
        for file in &files {
            let Some(bytes) = kept.read(&file.id)? else {
                return Ok(Err(Refusal::FileNotSent {
                    name: file.name.clone(),
                }));
            };
            if let Err(refusal) =
                inbox.put_at(&file.name, &bytes, file.at)?
            {
                return Ok(Err(refusal));
            }
        }
        attached
            .captions
            .extend(inbox.placed_captions("of this message"));
        attached.files = files;
        Ok(Ok(attached))
    }

    /// The images `images` of a line to `agent`, if its model takes
    /// them (S30), with the captions that tell where they are saved:
    /// in the agent's own folder of images.
    fn attach_images(
        &mut self,
        agent: AgentId,
        images: &[AttachedImage],
    ) -> Result<std::result::Result<Attached, Refusal>> {
        if images.is_empty() {
            return Ok(Ok(Attached::default()));
        }
        let db = self.realm.store().connection();
        let model = models::choose(
            db,
            &self.env.settings,
            &self.env.overrides,
            agent,
        )?
        .model;
        let named = format!("{}::{}", model.provider, model.id);
        let Some(takes) = self.providers.images(db, &model)? else {
            return Ok(Err(Refusal::NoImages { model: named }));
        };
        let most = Limits::of(&takes).per_line;
        if images.len() > most as usize {
            return Ok(Err(Refusal::TooManyImages {
                model: named,
                most,
            }));
        }
        let layout = self.realm.layout().clone();
        let lodge = crate::calls::place::of(
            self.realm.store().connection(),
            &layout,
            agent,
        )?
        .place
        .lodge;
        let remote = self.remote.workspace.is_some();
        if remote {
            self.remote.unpushed.write(UnpushedLodge(agent));
        }
        let kept = Images::new(layout.images());
        let mut attached = Attached::default();
        for (index, image) in images.iter().enumerate() {
            let Some(stored) = kept.find(&image.id)? else {
                return Ok(Err(Refusal::NoImage {
                    image: image.id.to_string(),
                }));
            };
            let name = kept.hand_over(&lodge, &image.id, &stored)?;
            let saved = if remote {
                format!("{LODGE}/{SEEN}/{name}")
            } else {
                lodge.join(SEEN).join(&name).display().to_string()
            };
            attached.captions.push(Caption {
                at: image.at,
                words: format!(
                    "[Image {} of this message, saved at {saved}]",
                    index + 1
                ),
            });
            attached.images.push(image.clone());
        }
        Ok(Ok(attached))
    }

    /// Keeps the images `images` longer.
    pub(crate) fn keep_images(&mut self, images: &[AttachedImage]) {
        let kept = Images::new(self.realm.layout().images());
        for image in images {
            if let Ok(Some(stored)) = kept.find(&image.id) {
                let _ = kept.touch(&image.id, &stored);
            }
        }
    }
}
