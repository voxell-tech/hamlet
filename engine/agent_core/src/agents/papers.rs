//! The notes, the character and the skills an agent keeps in its
//! house.

use std::path::PathBuf;

use hamlet_home::Home;

/// What an agent keeps in its house.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct Papers {
    pub notes: String,
    pub character: String,
    /// Each skill of its own by its name, with what it is for.
    pub skills: Vec<(String, String)>,
}

impl Papers {
    /// What `house` holds now. A paper not yet written is empty.
    pub fn read(house: &Home) -> Papers {
        let read = |file: PathBuf| {
            std::fs::read_to_string(file)
                .unwrap_or_default()
                .trim()
                .to_string()
        };
        Papers {
            notes: read(house.notes()),
            character: read(house.character()),
            skills: house
                .skill_index()
                .into_iter()
                .map(|skill| (skill.name, skill.description))
                .collect(),
        }
    }
}

/// The file of the character of `house`, made empty if there was
/// none, for the chief to write in.
pub fn character_file(house: &Home) -> std::io::Result<PathBuf> {
    let file = house.character();
    if !file.exists() {
        std::fs::write(&file, "")?;
    }
    Ok(file)
}
