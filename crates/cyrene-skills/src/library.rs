//! The [`SkillLibrary`]: filesystem-backed storage of saved skills.
//!
//! Each saved [`Skill`] lives as a `<id>/SKILL.md` file under a root directory,
//! so the on-disk layout is human-browsable and a skill keeps a stable location
//! across improvement-updates (R14.6). The library is the durable store the
//! Skill_Engine writes to once a candidate is tested and confirmed (R14.5).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::error::SkillError;
use crate::skill::{Skill, SkillId};

/// A summary of a saved skill, returned when browsing the library (R32.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillSummary {
    /// The skill's stable id.
    pub id: SkillId,
    /// The human-readable name.
    pub name: String,
    /// The category.
    pub category: String,
    /// The one-line description.
    pub description: String,
    /// The current stored version.
    pub version: u32,
}

/// A filesystem-backed store of skills, one `SKILL.md` per skill id.
#[derive(Debug, Clone)]
pub struct SkillLibrary {
    root: PathBuf,
}

impl SkillLibrary {
    /// Opens (or creates) a library rooted at `root`.
    ///
    /// # Errors
    /// Returns [`SkillError::Storage`] if the root cannot be created.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, SkillError> {
        let root = root.as_ref().to_path_buf();
        std::fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    /// The directory holding a given skill's `SKILL.md`.
    fn skill_dir(&self, id: &SkillId) -> PathBuf {
        self.root.join(id.as_str())
    }

    /// The path of a given skill's `SKILL.md` file.
    fn skill_path(&self, id: &SkillId) -> PathBuf {
        self.skill_dir(id).join("SKILL.md")
    }

    /// Returns `true` if a skill with the given id is stored.
    #[must_use]
    pub fn contains(&self, id: &SkillId) -> bool {
        self.skill_path(id).is_file()
    }

    /// Saves (or overwrites) a skill, writing its `SKILL.md`.
    ///
    /// # Errors
    /// Returns [`SkillError::Storage`] on a filesystem failure.
    pub fn save(&self, skill: &Skill) -> Result<SkillId, SkillError> {
        let id = skill.id();
        let dir = self.skill_dir(&id);
        std::fs::create_dir_all(&dir)?;
        std::fs::write(self.skill_path(&id), skill.to_skill_md())?;
        Ok(id)
    }

    /// Loads a skill by id.
    ///
    /// # Errors
    /// Returns [`SkillError::NotFound`] if no such skill exists, or
    /// [`SkillError::Parse`] if its `SKILL.md` is malformed.
    pub fn get(&self, id: &SkillId) -> Result<Skill, SkillError> {
        let path = self.skill_path(id);
        if !path.is_file() {
            return Err(SkillError::NotFound(id.to_string()));
        }
        let text = std::fs::read_to_string(path)?;
        Skill::from_skill_md(&text)
    }

    /// Lists all stored skills as summaries, ordered by id (R32.4).
    ///
    /// Malformed entries are skipped rather than failing the whole listing.
    ///
    /// # Errors
    /// Returns [`SkillError::Storage`] if the root cannot be read.
    pub fn list(&self) -> Result<Vec<SkillSummary>, SkillError> {
        let mut summaries: BTreeMap<String, SkillSummary> = BTreeMap::new();
        for entry in std::fs::read_dir(&self.root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let md = entry.path().join("SKILL.md");
            if !md.is_file() {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&md) else {
                continue;
            };
            if let Ok(skill) = Skill::from_skill_md(&text) {
                let id = skill.id();
                summaries.insert(
                    id.0.clone(),
                    SkillSummary {
                        id,
                        name: skill.name,
                        category: skill.category,
                        description: skill.description,
                        version: skill.version,
                    },
                );
            }
        }
        Ok(summaries.into_values().collect())
    }

    /// Returns the number of stored skills.
    ///
    /// # Errors
    /// Returns [`SkillError::Storage`] if the root cannot be read.
    pub fn count(&self) -> Result<usize, SkillError> {
        Ok(self.list()?.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_library() -> (tempfile::TempDir, SkillLibrary) {
        let dir = tempfile::tempdir().unwrap();
        let lib = SkillLibrary::open(dir.path()).unwrap();
        (dir, lib)
    }

    #[test]
    fn save_get_round_trip() {
        let (_dir, lib) = temp_library();
        let skill = Skill::new("Build", "build the project", "devops", "run cargo build");
        let id = lib.save(&skill).unwrap();
        assert!(lib.contains(&id));
        let back = lib.get(&id).unwrap();
        assert_eq!(back, skill);
    }

    #[test]
    fn get_missing_returns_not_found() {
        let (_dir, lib) = temp_library();
        let err = lib.get(&SkillId::from_name("ghost")).unwrap_err();
        assert!(matches!(err, SkillError::NotFound(_)));
    }

    #[test]
    fn list_returns_summaries() {
        let (_dir, lib) = temp_library();
        lib.save(&Skill::new("A", "first", "cat1", "do a")).unwrap();
        lib.save(&Skill::new("B", "second", "cat2", "do b"))
            .unwrap();
        let list = lib.list().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "A");
        assert_eq!(list[1].name, "B");
    }

    #[test]
    fn save_same_id_overwrites_in_place() {
        let (_dir, lib) = temp_library();
        let mut skill = Skill::new("Deploy", "v1", "devops", "do v1");
        lib.save(&skill).unwrap();
        skill.version = 2;
        skill.instructions = "do v2".to_owned();
        lib.save(&skill).unwrap();
        assert_eq!(lib.count().unwrap(), 1);
        assert_eq!(lib.get(&skill.id()).unwrap().version, 2);
    }
}
