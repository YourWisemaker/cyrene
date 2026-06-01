//! Property tests for the Trajectory Compressor (Property 9, R11.3/11.4).
//!
//! Property 9 (Trajectory bound): a `Behavioral_Blueprint` is always ≤20% of
//! the token count of the raw log it represents, while preserving outcome,
//! result, and reproduction steps.
//!
//! The ≤20% bound is asserted only for logs *above the threshold* (raw logs
//! large enough that compression is meaningful). For tiny logs, the minimum
//! per-step / result floor can exceed 20%, which the task wording explicitly
//! scopes out ("for logs above the threshold").

use cyrene_trajectory::{LogEntry, TaskOutcome, TrajectoryCompressor};
use proptest::prelude::*;

/// Raw-token threshold above which the ≤20% bound is enforced.
const RAW_TOKEN_THRESHOLD: usize = 500;

/// Re-estimate raw tokens the same way the compressor does (~4 chars/token).
fn estimate_raw_tokens(entries: &[LogEntry]) -> usize {
    entries
        .iter()
        .map(|e| (e.step.len() + e.result.len()) / 4 + 1)
        .sum()
}

prop_compose! {
    /// Generate a realistic raw execution log with a mix of key and non-key
    /// steps and reasonably long result text so logs can exceed the threshold.
    fn log_entries(min: usize, max: usize)(
        steps in prop::collection::vec(
            (
                "[a-z_]{4,20}",
                "[a-zA-Z0-9 .,:/_-]{20,200}",
                any::<bool>(),
            ),
            min..max,
        )
    ) -> Vec<LogEntry> {
        steps
            .into_iter()
            .map(|(step, result, is_key_step)| LogEntry { step, result, is_key_step })
            .collect()
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// Property 9a: For logs above the threshold, the blueprint token count is
    /// ≤20% of the raw log token count.
    ///
    /// **Validates: Requirements 11.3**
    #[test]
    fn prop9a_blueprint_within_twenty_percent(
        entries in log_entries(1, 80),
        final_result in "[a-zA-Z0-9 .,:/_-]{0,120}",
        outcome_idx in 0usize..3,
    ) {
        let outcome = match outcome_idx {
            0 => TaskOutcome::Success,
            1 => TaskOutcome::Failure,
            _ => TaskOutcome::Partial,
        };

        let raw_tokens = estimate_raw_tokens(&entries);
        prop_assume!(raw_tokens > RAW_TOKEN_THRESHOLD);

        let compressor = TrajectoryCompressor::default();
        let bp = compressor.compress(&entries, outcome, &final_result, None);
        let bp_tokens = TrajectoryCompressor::blueprint_tokens(&bp);

        prop_assert!(
            bp_tokens <= (raw_tokens as f64 * 0.20).ceil() as usize,
            "blueprint {} tokens > 20% of raw {} tokens",
            bp_tokens,
            raw_tokens,
        );
    }

    /// Property 9b: Compression always preserves the outcome exactly, and for
    /// logs above the threshold (where the token budget is large enough) the
    /// final result is retained in full.
    ///
    /// For tiny logs the result may be truncated to fit the ≤20% budget; that
    /// case is covered by `prop9a` (the bound holds) and is intentionally
    /// excluded here since the task scopes result preservation to logs above
    /// the threshold.
    ///
    /// **Validates: Requirements 11.4**
    #[test]
    fn prop9b_preserves_outcome_and_result(
        entries in log_entries(1, 80),
        final_result in "[a-zA-Z0-9 ]{1,40}",
        outcome_idx in 0usize..3,
    ) {
        let outcome = match outcome_idx {
            0 => TaskOutcome::Success,
            1 => TaskOutcome::Failure,
            _ => TaskOutcome::Partial,
        };

        let compressor = TrajectoryCompressor::default();
        let bp = compressor.compress(&entries, outcome, &final_result, None);

        // Outcome is preserved exactly for any input.
        prop_assert_eq!(bp.outcome, outcome);

        // For logs above the threshold the budget is large enough that the
        // result is retained in full (modulo trailing-whitespace trimming).
        let raw_tokens = estimate_raw_tokens(&entries);
        if raw_tokens > RAW_TOKEN_THRESHOLD {
            prop_assert!(
                bp.final_result.contains(final_result.trim_end()),
                "final result not preserved above threshold: {:?} -> {:?}",
                final_result,
                bp.final_result,
            );
        }
    }

    /// Property 9c: Whenever the raw log has at least one step, the blueprint
    /// preserves at least one reproduction step.
    ///
    /// **Validates: Requirements 11.4**
    #[test]
    fn prop9c_preserves_at_least_one_step(
        entries in log_entries(1, 80),
        final_result in "[a-zA-Z0-9 ]{0,40}",
    ) {
        let compressor = TrajectoryCompressor::default();
        let bp = compressor.compress(&entries, TaskOutcome::Success, &final_result, None);

        prop_assert!(
            !bp.reproducible_steps.is_empty(),
            "non-empty log produced no reproduction steps",
        );
    }

    /// Property 9d: The blueprint is never larger (in tokens) than the raw log.
    ///
    /// Compression must never inflate the representation.
    ///
    /// **Validates: Requirements 11.3**
    #[test]
    fn prop9d_never_inflates(
        entries in log_entries(1, 80),
        final_result in "[a-zA-Z0-9 .,:/_-]{0,120}",
    ) {
        let raw_tokens = estimate_raw_tokens(&entries);
        prop_assume!(raw_tokens > RAW_TOKEN_THRESHOLD);

        let compressor = TrajectoryCompressor::default();
        let bp = compressor.compress(&entries, TaskOutcome::Success, &final_result, None);
        let bp_tokens = TrajectoryCompressor::blueprint_tokens(&bp);

        prop_assert!(
            bp_tokens <= raw_tokens,
            "blueprint {} tokens > raw {} tokens",
            bp_tokens,
            raw_tokens,
        );
    }
}
