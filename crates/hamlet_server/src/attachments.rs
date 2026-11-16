//! What a chief's line carries besides its words: the images and
//! files the realm keeps, the captions that say where each is saved,
//! and the inbox of the lodge it is saved in (C33, S29).

pub(crate) mod captions;
pub(crate) mod files;
pub(crate) mod images;
pub(crate) mod inbox;

pub(crate) fn plugin(app: &mut bevy_app::App) {
    app.add_plugins((images::plugin, files::plugin));
}
