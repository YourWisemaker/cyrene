//! The [`Skill`] data model and the `SKILL.md` serialization format (R14.1).
//!
//! A [`Skill`] is the unit of reusable capability Cyrene accumulates. Each
//! skill is persisted as a `SKILL.md` file: a YAML-style front-matter block
//! (delimited by `---`) carrying structured metadata, followed by a Markdown
//! body holding the natural-language instructions and any required code. The
//! same format is used for both engine-created and bundled skills so they are
//! interchangeable (R32.3).

use serde::{Deserialize, Serialize};

use crate::error::SkillError;

/// A stable identifier for a skill, derived from its name so a skill keeps the
/// same identity across updates (R14.6).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SkillId(pub String);

impl SkillId {
    /// Derives an id from a human-readable name by slugifying it.
    #[must_use]
    pub fn from_name(name: &str) -> Self {
        let slug: String = name
            .trim()
            .to_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        // Collapse runs of '-' and trim leading/trailing separators.
        let mut out = String::with_capacity(slug.len());
        let mut prev_dash = false;
        for c in slug.chars() {
            if c == '-' {
                if !prev_dash {
                    out.push('-');
                }
                prev_dash = true;
            } else {
                out.push(c);
                prev_dash = false;
            }
        }
        Self(out.trim_matches('-').to_owned())
    }

    /// Returns the id as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for SkillId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A reusable skill: instructions plus optional code, with metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Skill {
    /// Human-readable name (the front-matter `name`).
    pub name: String,
    /// One-line description shown when browsing the library (R32.4).
    pub description: String,
    /// The category this skill belongs to (e.g. `"devops"`, R32.2).
    pub category: String,
    /// Free-form tags for search/discovery.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Monotonic version, bumped on each improvement-update (R14.6).
    #[serde(default = "default_version")]
    pub version: u32,
    /// The natural-language instructions (the Markdown body).
    pub instructions: String,
    /// Optional code block required to perform the skill.
    #[serde(default)]
    pub code: Option<String>,
}

fn default_version() -> u32 {
    1
}

impl Skill {
    /// Creates a new version-1 skill.
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        category: impl Into<String>,
        instructions: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            category: category.into(),
            tags: Vec::new(),
            version: 1,
            instructions: instructions.into(),
            code: None,
        }
    }

    /// Sets the optional code body, returning `self` for chaining.
    #[must_use]
    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    /// Sets tags, returning `self` for chaining.
    #[must_use]
    pub fn with_tags<I, S>(mut self, tags: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.tags = tags.into_iter().map(Into::into).collect();
        self
    }

    /// The id derived from this skill's name.
    #[must_use]
    pub fn id(&self) -> SkillId {
        SkillId::from_name(&self.name)
    }

    /// Renders this skill to the `SKILL.md` on-disk format: YAML-ish
    /// front-matter delimited by `---`, then the Markdown instructions and an
    /// optional fenced code block.
    #[must_use]
    pub fn to_skill_md(&self) -> String {
        let mut out = String::new();
        out.push_str("---\n");
        out.push_str(&format!("name: {}\n", yaml_escape(&self.name)));
        out.push_str(&format!(
            "description: {}\n",
            yaml_escape(&self.description)
        ));
        out.push_str(&format!("category: {}\n", yaml_escape(&self.category)));
        out.push_str(&format!("version: {}\n", self.version));
        if !self.tags.is_empty() {
            let joined = self
                .tags
                .iter()
                .map(|t| yaml_escape(t))
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&format!("tags: [{joined}]\n"));
        }
        out.push_str("---\n\n");
        out.push_str(self.instructions.trim_end());
        out.push('\n');
        if let Some(code) = &self.code {
            out.push_str("\n```\n");
            out.push_str(code.trim_end());
            out.push_str("\n```\n");
        }
        out
    }

    /// Parses a [`Skill`] from the `SKILL.md` on-disk format.
    ///
    /// # Errors
    /// Returns [`SkillError::Parse`] if the front-matter block is missing or a
    /// required field (`name`, `description`, `category`) is absent.
    pub fn from_skill_md(text: &str) -> Result<Self, SkillError> {
        let rest = text
            .strip_prefix("---\n")
            .or_else(|| text.strip_prefix("---\r\n"))
            .ok_or_else(|| SkillError::Parse("missing front-matter opening `---`".to_owned()))?;

        let end = rest
            .find("\n---")
            .ok_or_else(|| SkillError::Parse("missing front-matter closing `---`".to_owned()))?;
        let front = &rest[..end];
        // Body starts after the closing `---` line.
        let after = &rest[end + 1..];
        let body = after
            .strip_prefix("---")
            .map(|b| b.trim_start_matches(['\r', '\n']))
            .unwrap_or("");

        let mut name = None;
        let mut description = None;
        let mut category = None;
        let mut version = 1u32;
        let mut tags = Vec::new();

        for line in front.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let key = key.trim();
            let value = value.trim();
            match key {
                "name" => name = Some(yaml_unescape(value)),
                "description" => description = Some(yaml_unescape(value)),
                "category" => category = Some(yaml_unescape(value)),
                "version" => version = value.parse().unwrap_or(1),
                "tags" => {
                    let inner = value.trim_start_matches('[').trim_end_matches(']');
                    tags = inner
                        .split(',')
                        .map(|t| yaml_unescape(t.trim()))
                        .filter(|t| !t.is_empty())
                        .collect();
                }
                _ => {}
            }
        }

        // Split the body into instructions and an optional fenced code block.
        let (instructions, code) = split_body(body);

        Ok(Self {
            name: name.ok_or_else(|| SkillError::Parse("missing `name`".to_owned()))?,
            description: description
                .ok_or_else(|| SkillError::Parse("missing `description`".to_owned()))?,
            category: category.ok_or_else(|| SkillError::Parse("missing `category`".to_owned()))?,
            tags,
            version,
            instructions,
            code,
        })
    }
}

/// Splits a Markdown body into leading instructions and a trailing fenced code
/// block, if one is present.
fn split_body(body: &str) -> (String, Option<String>) {
    if let Some(fence_start) = body.find("```") {
        let before = &body[..fence_start];
        let after = &body[fence_start + 3..];
        // Skip an optional language tag on the opening fence line.
        let after = match after.find('\n') {
            Some(nl) => &after[nl + 1..],
            None => "",
        };
        if let Some(fence_end) = after.find("```") {
            let code = after[..fence_end].trim_end_matches(['\r', '\n']).to_owned();
            return (before.trim().to_owned(), Some(code));
        }
    }
    (body.trim().to_owned(), None)
}

/// Escapes a scalar for the minimal YAML front-matter (quotes values that
/// contain structural characters).
fn yaml_escape(s: &str) -> String {
    if s.contains([':', '[', ']', ',', '"', '\n', '#']) {
        format!("\"{}\"", s.replace('"', "\\\""))
    } else {
        s.to_owned()
    }
}

/// Reverses [`yaml_escape`] for a scalar value.
fn yaml_unescape(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        s[1..s.len() - 1].replace("\\\"", "\"")
    } else {
        s.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_slugifies_name() {
        assert_eq!(
            SkillId::from_name("Deploy to AWS!").as_str(),
            "deploy-to-aws"
        );
        assert_eq!(
            SkillId::from_name("  Hello   World  ").as_str(),
            "hello-world"
        );
    }

    #[test]
    fn skill_md_round_trip_with_code() {
        let skill = Skill::new(
            "Format Rust",
            "Run rustfmt on the workspace",
            "software-development",
            "Run the formatter across all crates.",
        )
        .with_code("cargo fmt --all")
        .with_tags(["rust", "formatting"]);

        let md = skill.to_skill_md();
        let back = Skill::from_skill_md(&md).unwrap();
        assert_eq!(back, skill);
    }

    #[test]
    fn skill_md_round_trip_without_code() {
        let skill = Skill::new(
            "Greet",
            "Say hello",
            "communication",
            "Greet the user warmly.",
        );
        let md = skill.to_skill_md();
        let back = Skill::from_skill_md(&md).unwrap();
        assert_eq!(back, skill);
        assert!(back.code.is_none());
    }

    #[test]
    fn parse_rejects_missing_front_matter() {
        let err = Skill::from_skill_md("no front matter here").unwrap_err();
        assert!(matches!(err, SkillError::Parse(_)));
    }

    #[test]
    fn parse_rejects_missing_required_field() {
        let md = "---\nname: X\ncategory: c\n---\nbody";
        let err = Skill::from_skill_md(md).unwrap_err();
        assert!(matches!(err, SkillError::Parse(_)));
    }

    #[test]
    fn escapes_values_with_structural_chars() {
        let skill = Skill::new(
            "Tricky: name",
            "has, commas, and [brackets]",
            "misc",
            "do the thing",
        );
        let md = skill.to_skill_md();
        let back = Skill::from_skill_md(&md).unwrap();
        assert_eq!(back, skill);
    }
}
