//! The [`SkillEngine`]: the closed-loop skill creation pipeline (R14, R15).
//!
//! The engine drives the lifecycle of a candidate skill:
//!
//! 1. **Generate** a candidate `SKILL.md` for a task not covered by an existing
//!    skill (R14.1). Callers hand the engine a [`Skill`] candidate.
//! 2. **Test** it in a [`SandboxTester`] (R14.2). On failure the candidate is
//!    discarded and the failure is logged (R14.3).
//! 3. **Confirm** with the user via a [`Confirmer`] before saving (R14.4).
//! 4. **Save** to the [`SkillLibrary`] on confirmation, making it reusable
//!    (R14.5).
//!
//! Improvements to an existing skill update the stored definition and log the
//! change (R14.6). Recurring related skills are proposed as a [`SkillBundle`],
//! presented for confirmation, and saved all-or-nothing (R15).
//!
//! The sandbox, ledger, and user-confirmation dependencies are **traits**, so
//! the loop is fully unit-testable with fakes and the engine stays decoupled
//! from the concrete safety/ledger/channel crates.

use crate::bundle::SkillBundle;
use crate::error::SkillError;
use crate::library::SkillLibrary;
use crate::skill::{Skill, SkillId};

/// The outcome of testing a candidate skill in a sandbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestOutcome {
    /// The candidate behaved correctly in the sandbox.
    Passed,
    /// The candidate failed; the string explains why (logged on discard).
    Failed(String),
}

/// Tests a candidate skill in an isolated sandbox before it can be saved
/// (R14.2). Backed in production by the `cyrene-safety` Sandbox.
pub trait SandboxTester {
    /// Runs the candidate skill in a sandbox and reports the outcome.
    fn test(&self, skill: &Skill) -> TestOutcome;
}

/// Asks the user to confirm a save (R14.4, R15.2). Backed in production by a
/// Channel prompt.
pub trait Confirmer {
    /// Returns `true` if the user confirms saving the given skill.
    fn confirm_skill(&self, skill: &Skill) -> bool;

    /// Returns `true` if the user confirms saving the given bundle (R15.2).
    /// Defaults to confirming each skill individually.
    fn confirm_bundle(&self, bundle: &SkillBundle) -> bool {
        bundle.skills.iter().all(|s| self.confirm_skill(s))
    }
}

/// Records skill lifecycle events in the Receipt_Ledger (R14.3, R14.6).
/// Backed in production by the `cyrene-ledger` Receipt_Ledger.
pub trait SkillLedger {
    /// Records that a candidate was discarded after a failed sandbox test.
    fn record_discard(&self, skill: &Skill, reason: &str);

    /// Records that a skill was saved to the library.
    fn record_save(&self, id: &SkillId, version: u32);

    /// Records that an existing skill was updated (improved) in place.
    fn record_update(&self, id: &SkillId, old_version: u32, new_version: u32);
}

/// The result of running a candidate through the full create→test→confirm→save
/// loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillDecision {
    /// The skill was tested, confirmed, and saved (carries its id + version).
    Saved(SkillId, u32),
    /// The user declined to save the tested candidate (not an error).
    Declined,
    /// The candidate failed its sandbox test and was discarded + logged.
    Discarded(String),
}

/// The closed-loop skill creation engine.
pub struct SkillEngine<T, C, L> {
    library: SkillLibrary,
    tester: T,
    confirmer: C,
    ledger: L,
}

impl<T, C, L> SkillEngine<T, C, L>
where
    T: SandboxTester,
    C: Confirmer,
    L: SkillLedger,
{
    /// Creates an engine over a library and its sandbox/confirm/ledger deps.
    pub fn new(library: SkillLibrary, tester: T, confirmer: C, ledger: L) -> Self {
        Self {
            library,
            tester,
            confirmer,
            ledger,
        }
    }

    /// Borrows the underlying skill library.
    #[must_use]
    pub fn library(&self) -> &SkillLibrary {
        &self.library
    }

    /// Returns `true` if a task is already covered by a stored skill, so the
    /// engine only proposes genuinely new capabilities (R14.1).
    #[must_use]
    pub fn is_covered(&self, candidate: &Skill) -> bool {
        self.library.contains(&candidate.id())
    }

    /// Runs the full lifecycle for a single candidate skill (R14.1–14.5):
    /// mandatory sandbox test → discard+log on failure → user confirmation →
    /// save on confirm.
    ///
    /// # Errors
    /// Returns [`SkillError`] only on a storage failure while saving; a failed
    /// test or a decline are normal [`SkillDecision`] outcomes, not errors.
    pub fn process_candidate(&self, candidate: Skill) -> Result<SkillDecision, SkillError> {
        // Step 2: mandatory sandbox test (R14.2).
        match self.tester.test(&candidate) {
            TestOutcome::Failed(reason) => {
                // Step 3a: discard + log on failure (R14.3).
                self.ledger.record_discard(&candidate, &reason);
                Ok(SkillDecision::Discarded(reason))
            }
            TestOutcome::Passed => {
                // Step 4: confirm before save (R14.4).
                if !self.confirmer.confirm_skill(&candidate) {
                    return Ok(SkillDecision::Declined);
                }
                // Step 5: save and make reusable (R14.5).
                let id = self.library.save(&candidate)?;
                self.ledger.record_save(&id, candidate.version);
                Ok(SkillDecision::Saved(id, candidate.version))
            }
        }
    }

    /// Updates an existing skill with an improved definition, bumping its
    /// version and logging the change (R14.6).
    ///
    /// The improved skill must share the existing skill's id (same name). The
    /// new version is the stored version + 1 regardless of the candidate's
    /// declared version, so updates are monotonic.
    ///
    /// # Errors
    /// Returns [`SkillError::NotFound`] if no skill with that id exists, or
    /// [`SkillError::Storage`] on a save failure.
    pub fn improve_skill(&self, mut improved: Skill) -> Result<SkillDecision, SkillError> {
        let id = improved.id();
        let existing = self.library.get(&id)?;
        let old_version = existing.version;
        improved.version = old_version + 1;
        self.library.save(&improved)?;
        self.ledger
            .record_update(&id, old_version, improved.version);
        Ok(SkillDecision::Saved(id, improved.version))
    }

    /// Proposes a [`SkillBundle`] for confirmation and, if confirmed, saves
    /// every skill in it all-or-nothing (R15.2–15.4).
    ///
    /// Each skill in the bundle is still sandbox-tested first (R15 defers skill
    /// validation to R14); if any skill fails its test the whole bundle is
    /// discarded and the failure logged.
    ///
    /// # Errors
    /// Returns [`SkillError::Storage`] on a save failure.
    pub fn process_bundle(&self, bundle: SkillBundle) -> Result<SkillDecision, SkillError> {
        // Validate every skill in the bundle in the sandbox first (R15 → R14.2).
        for skill in &bundle.skills {
            if let TestOutcome::Failed(reason) = self.tester.test(skill) {
                self.ledger.record_discard(skill, &reason);
                return Ok(SkillDecision::Discarded(format!(
                    "bundle skill `{}` failed: {reason}",
                    skill.name
                )));
            }
        }

        // Present contents/purpose and ask for one confirmation (R15.2).
        if !self.confirmer.confirm_bundle(&bundle) {
            // Decline: discard the whole bundle, save nothing (R15.4).
            return Ok(SkillDecision::Declined);
        }

        // Confirm: save every skill (R15.3).
        let mut last = None;
        for skill in &bundle.skills {
            let id = self.library.save(skill)?;
            self.ledger.record_save(&id, skill.version);
            last = Some((id, skill.version));
        }
        match last {
            Some((id, v)) => Ok(SkillDecision::Saved(id, v)),
            None => Ok(SkillDecision::Declined), // empty bundle: nothing saved
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// A tester whose verdict is fixed at construction.
    struct FixedTester(TestOutcome);
    impl SandboxTester for FixedTester {
        fn test(&self, _skill: &Skill) -> TestOutcome {
            self.0.clone()
        }
    }

    /// A confirmer that always answers the same way.
    struct FixedConfirmer(bool);
    impl Confirmer for FixedConfirmer {
        fn confirm_skill(&self, _skill: &Skill) -> bool {
            self.0
        }
    }

    /// A ledger that records event kinds for assertions.
    #[derive(Default)]
    struct RecordingLedger {
        events: RefCell<Vec<String>>,
    }
    impl SkillLedger for RecordingLedger {
        fn record_discard(&self, skill: &Skill, reason: &str) {
            self.events
                .borrow_mut()
                .push(format!("discard:{}:{reason}", skill.name));
        }
        fn record_save(&self, id: &SkillId, version: u32) {
            self.events
                .borrow_mut()
                .push(format!("save:{id}:{version}"));
        }
        fn record_update(&self, id: &SkillId, old: u32, new: u32) {
            self.events
                .borrow_mut()
                .push(format!("update:{id}:{old}->{new}"));
        }
    }

    fn library() -> (tempfile::TempDir, SkillLibrary) {
        let dir = tempfile::tempdir().unwrap();
        let lib = SkillLibrary::open(dir.path()).unwrap();
        (dir, lib)
    }

    fn candidate() -> Skill {
        Skill::new(
            "Lint",
            "lint the code",
            "software-development",
            "run clippy",
        )
    }

    #[test]
    fn passing_and_confirmed_candidate_is_saved() {
        let (_dir, lib) = library();
        let engine = SkillEngine::new(
            lib,
            FixedTester(TestOutcome::Passed),
            FixedConfirmer(true),
            RecordingLedger::default(),
        );
        let decision = engine.process_candidate(candidate()).unwrap();
        match decision {
            SkillDecision::Saved(id, v) => {
                assert_eq!(id.as_str(), "lint");
                assert_eq!(v, 1);
            }
            other => panic!("expected Saved, got {other:?}"),
        }
        assert!(engine.library().contains(&SkillId::from_name("Lint")));
    }

    #[test]
    fn failing_candidate_is_discarded_and_logged() {
        let (_dir, lib) = library();
        let ledger = RecordingLedger::default();
        let engine = SkillEngine::new(
            lib,
            FixedTester(TestOutcome::Failed("compile error".to_owned())),
            FixedConfirmer(true),
            ledger,
        );
        let decision = engine.process_candidate(candidate()).unwrap();
        assert_eq!(
            decision,
            SkillDecision::Discarded("compile error".to_owned())
        );
        // Nothing was saved.
        assert!(!engine.library().contains(&SkillId::from_name("Lint")));
    }

    #[test]
    fn declined_candidate_is_not_saved() {
        let (_dir, lib) = library();
        let engine = SkillEngine::new(
            lib,
            FixedTester(TestOutcome::Passed),
            FixedConfirmer(false),
            RecordingLedger::default(),
        );
        let decision = engine.process_candidate(candidate()).unwrap();
        assert_eq!(decision, SkillDecision::Declined);
        assert!(!engine.library().contains(&SkillId::from_name("Lint")));
    }

    #[test]
    fn improve_existing_skill_bumps_version_and_logs() {
        let (_dir, lib) = library();
        lib.save(&candidate()).unwrap();
        let engine = SkillEngine::new(
            lib,
            FixedTester(TestOutcome::Passed),
            FixedConfirmer(true),
            RecordingLedger::default(),
        );
        let improved = Skill::new(
            "Lint",
            "lint better",
            "software-development",
            "run clippy -D warnings",
        );
        let decision = engine.improve_skill(improved).unwrap();
        assert_eq!(
            decision,
            SkillDecision::Saved(SkillId::from_name("Lint"), 2)
        );
        assert_eq!(
            engine
                .library()
                .get(&SkillId::from_name("Lint"))
                .unwrap()
                .version,
            2
        );
    }

    #[test]
    fn improve_missing_skill_errors() {
        let (_dir, lib) = library();
        let engine = SkillEngine::new(
            lib,
            FixedTester(TestOutcome::Passed),
            FixedConfirmer(true),
            RecordingLedger::default(),
        );
        let err = engine.improve_skill(candidate()).unwrap_err();
        assert!(matches!(err, SkillError::NotFound(_)));
    }

    #[test]
    fn is_covered_reflects_library_state() {
        let (_dir, lib) = library();
        let engine = SkillEngine::new(
            lib,
            FixedTester(TestOutcome::Passed),
            FixedConfirmer(true),
            RecordingLedger::default(),
        );
        assert!(!engine.is_covered(&candidate()));
        engine.process_candidate(candidate()).unwrap();
        assert!(engine.is_covered(&candidate()));
    }

    fn release_bundle() -> SkillBundle {
        SkillBundle::new(
            "Release",
            "Skills for cutting a release",
            vec![
                Skill::new("Tag", "git tag", "devops", "git tag vX"),
                Skill::new("Publish", "publish crate", "devops", "cargo publish"),
            ],
        )
    }

    #[test]
    fn confirmed_bundle_saves_every_skill() {
        let (_dir, lib) = library();
        let engine = SkillEngine::new(
            lib,
            FixedTester(TestOutcome::Passed),
            FixedConfirmer(true),
            RecordingLedger::default(),
        );
        let decision = engine.process_bundle(release_bundle()).unwrap();
        assert!(matches!(decision, SkillDecision::Saved(_, _)));
        assert!(engine.library().contains(&SkillId::from_name("Tag")));
        assert!(engine.library().contains(&SkillId::from_name("Publish")));
        assert_eq!(engine.library().count().unwrap(), 2);
    }

    #[test]
    fn declined_bundle_saves_nothing() {
        let (_dir, lib) = library();
        let engine = SkillEngine::new(
            lib,
            FixedTester(TestOutcome::Passed),
            FixedConfirmer(false),
            RecordingLedger::default(),
        );
        let decision = engine.process_bundle(release_bundle()).unwrap();
        assert_eq!(decision, SkillDecision::Declined);
        assert_eq!(engine.library().count().unwrap(), 0);
    }

    #[test]
    fn bundle_with_a_failing_skill_is_discarded_entirely() {
        let (_dir, lib) = library();
        let engine = SkillEngine::new(
            lib,
            FixedTester(TestOutcome::Failed("bad skill".to_owned())),
            FixedConfirmer(true),
            RecordingLedger::default(),
        );
        let decision = engine.process_bundle(release_bundle()).unwrap();
        assert!(matches!(decision, SkillDecision::Discarded(_)));
        // All-or-nothing: nothing saved when any skill fails.
        assert_eq!(engine.library().count().unwrap(), 0);
    }
}
