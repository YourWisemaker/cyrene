//! The [`SkillBundle`]: a group of related skills proposed together (R15).
//!
//! When the Skill_Engine notices several related skills emerging from a
//! recurring workflow, it proposes them as a single bundle the user confirms or
//! declines as a unit (R15.1). The bundle carries a name and purpose so the
//! engine can present "what this is and why" before saving (R15.2).

use serde::{Deserialize, Serialize};

use crate::skill::Skill;

/// A named group of related skills proposed for all-or-nothing confirmation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillBundle {
    /// A human-readable name for the bundle.
    pub name: String,
    /// Why these skills belong together (shown before saving, R15.2).
    pub purpose: String,
    /// The skills contained in the bundle.
    pub skills: Vec<Skill>,
}

impl SkillBundle {
    /// Creates a bundle from a name, purpose, and its skills.
    pub fn new(name: impl Into<String>, purpose: impl Into<String>, skills: Vec<Skill>) -> Self {
        Self {
            name: name.into(),
            purpose: purpose.into(),
            skills,
        }
    }

    /// The number of skills in the bundle.
    #[must_use]
    pub fn len(&self) -> usize {
        self.skills.len()
    }

    /// Whether the bundle contains no skills.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }

    /// Renders a human-readable summary of the bundle's contents and purpose,
    /// suitable for presenting to the user before they confirm (R15.2).
    #[must_use]
    pub fn presentation(&self) -> String {
        let mut out = format!(
            "Skill bundle: {}\nPurpose: {}\nContains:\n",
            self.name, self.purpose
        );
        for skill in &self.skills {
            out.push_str(&format!("  - {} — {}\n", skill.name, skill.description));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundle() -> SkillBundle {
        SkillBundle::new(
            "Release workflow",
            "Skills for cutting a release",
            vec![
                Skill::new("Tag", "create a git tag", "devops", "git tag vX"),
                Skill::new("Publish", "publish the crate", "devops", "cargo publish"),
            ],
        )
    }

    #[test]
    fn len_and_empty() {
        let b = bundle();
        assert_eq!(b.len(), 2);
        assert!(!b.is_empty());
        assert!(SkillBundle::new("e", "p", vec![]).is_empty());
    }

    #[test]
    fn presentation_lists_purpose_and_contents() {
        let p = bundle().presentation();
        assert!(p.contains("Release workflow"));
        assert!(p.contains("Purpose: Skills for cutting a release"));
        assert!(p.contains("Tag"));
        assert!(p.contains("Publish"));
    }

    #[test]
    fn serde_round_trip() {
        let b = bundle();
        let json = serde_json::to_string(&b).unwrap();
        let back: SkillBundle = serde_json::from_str(&json).unwrap();
        assert_eq!(b, back);
    }
}
