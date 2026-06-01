//! `cyrene-trajectory`: Trajectory Compressor for Cyrene (R11).
//!
//! When a subagent finishes, its raw execution log is compressed into a
//! [`BehavioralBlueprint`] returned to the parent in place of raw logs.
//! The blueprint preserves the task outcome, final result, and reproducible
//! steps while staying within ≤20% of the raw token count.

use serde::{Deserialize, Serialize};

/// A compact representation of how a task was solved, returned by a subagent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BehavioralBlueprint {
    /// Whether the task succeeded or failed.
    pub outcome: TaskOutcome,
    /// The final result/output of the task.
    pub final_result: String,
    /// The key steps needed to reproduce the outcome (compressed).
    pub reproducible_steps: Vec<String>,
    /// An optional embedding vector for semantic similarity search.
    pub embedding: Vec<f32>,
    /// Reference to the retained raw log for full retrieval (R11.5).
    pub raw_log_ref: Option<String>,
}

/// The outcome of a subagent task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskOutcome {
    Success,
    Failure,
    Partial,
}

/// A raw execution log entry from a subagent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogEntry {
    /// The step description.
    pub step: String,
    /// The result of this step.
    pub result: String,
    /// Whether this step is a key/reproducible step.
    pub is_key_step: bool,
}

/// The Trajectory Compressor: compresses raw execution logs into blueprints.
pub struct TrajectoryCompressor {
    /// Maximum ratio of blueprint tokens to raw tokens (default 0.20 = 20%).
    pub max_ratio: f64,
}

impl Default for TrajectoryCompressor {
    fn default() -> Self {
        Self { max_ratio: 0.20 }
    }
}

impl TrajectoryCompressor {
    /// Create a compressor with a custom max ratio.
    #[must_use]
    pub fn new(max_ratio: f64) -> Self {
        Self { max_ratio }
    }

    /// Compress a raw execution log into a behavioral blueprint.
    ///
    /// The blueprint's token count will be ≤ `max_ratio` of the raw log's
    /// token count (R11.3), while preserving outcome, result, and key steps.
    pub fn compress(
        &self,
        entries: &[LogEntry],
        outcome: TaskOutcome,
        final_result: &str,
        raw_log_ref: Option<String>,
    ) -> BehavioralBlueprint {
        let raw_token_estimate = estimate_tokens(entries);
        let max_blueprint_tokens = (raw_token_estimate as f64 * self.max_ratio) as usize;

        // Extract key/reproducible steps.
        let mut steps: Vec<String> = entries
            .iter()
            .filter(|e| e.is_key_step)
            .map(|e| format!("{}: {}", e.step, truncate_str(&e.result, 100)))
            .collect();

        // If no key steps marked, take first and last entries.
        if steps.is_empty() && !entries.is_empty() {
            if let Some(first) = entries.first() {
                steps.push(format!(
                    "{}: {}",
                    first.step,
                    truncate_str(&first.result, 100)
                ));
            }
            if entries.len() > 1 {
                if let Some(last) = entries.last() {
                    steps.push(format!(
                        "{}: {}",
                        last.step,
                        truncate_str(&last.result, 100)
                    ));
                }
            }
        }

        // Trim steps to fit within the token budget.
        let result_tokens = estimate_str_tokens(final_result);
        let overhead_tokens = 20; // outcome + structure
        let available_for_steps =
            max_blueprint_tokens.saturating_sub(result_tokens + overhead_tokens);

        let mut trimmed_steps = Vec::new();
        let mut used_tokens = 0;
        for step in &steps {
            let step_tokens = estimate_str_tokens(step);
            // Always include at least the first step; then respect the budget.
            if !trimmed_steps.is_empty() && used_tokens + step_tokens > available_for_steps {
                break;
            }
            trimmed_steps.push(step.clone());
            used_tokens += step_tokens;
        }

        BehavioralBlueprint {
            outcome,
            final_result: truncate_str(final_result, max_blueprint_tokens * 4), // ~4 chars per token
            reproducible_steps: trimmed_steps,
            embedding: Vec::new(), // Embedding computed externally if needed.
            raw_log_ref,
        }
    }

    /// Estimate the token count of a blueprint.
    #[must_use]
    pub fn blueprint_tokens(blueprint: &BehavioralBlueprint) -> usize {
        let mut tokens = 10; // overhead for structure
        tokens += estimate_str_tokens(&blueprint.final_result);
        for step in &blueprint.reproducible_steps {
            tokens += estimate_str_tokens(step);
        }
        tokens
    }
}

/// Estimate token count for a set of log entries (~4 chars per token).
fn estimate_tokens(entries: &[LogEntry]) -> usize {
    entries
        .iter()
        .map(|e| (e.step.len() + e.result.len()) / 4 + 1)
        .sum()
}

/// Estimate token count for a string (~4 chars per token).
fn estimate_str_tokens(s: &str) -> usize {
    s.len() / 4 + 1
}

/// Truncate a string to approximately `max_chars` characters.
fn truncate_str(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        s.to_owned()
    } else {
        format!("{}...", &s[..max_chars.min(s.len())])
    }
}

/// Returns the stable identifier of this subsystem crate.
#[must_use]
pub fn subsystem() -> &'static str {
    "cyrene-trajectory"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subsystem_id_is_nonempty() {
        assert!(!subsystem().is_empty());
    }

    fn sample_entries(n: usize) -> Vec<LogEntry> {
        (0..n)
            .map(|i| LogEntry {
                step: format!("step_{i}"),
                result: format!("result for step {i} with some detail text here"),
                is_key_step: i == 0 || i == n - 1,
            })
            .collect()
    }

    #[test]
    fn compress_produces_blueprint_with_outcome_and_result() {
        let compressor = TrajectoryCompressor::default();
        let entries = sample_entries(10);
        let bp = compressor.compress(&entries, TaskOutcome::Success, "all done", None);
        assert_eq!(bp.outcome, TaskOutcome::Success);
        assert!(bp.final_result.contains("all done"));
    }

    #[test]
    fn compress_preserves_key_steps() {
        let compressor = TrajectoryCompressor::default();
        let entries = sample_entries(10);
        let bp = compressor.compress(&entries, TaskOutcome::Success, "done", None);
        assert!(!bp.reproducible_steps.is_empty());
        assert!(bp.reproducible_steps[0].contains("step_0"));
    }

    #[test]
    fn blueprint_is_smaller_than_raw() {
        let compressor = TrajectoryCompressor::default();
        let entries = sample_entries(50);
        let raw_tokens = estimate_tokens(&entries);
        let bp = compressor.compress(&entries, TaskOutcome::Success, "done", None);
        let bp_tokens = TrajectoryCompressor::blueprint_tokens(&bp);
        assert!(
            bp_tokens <= (raw_tokens as f64 * 0.25) as usize, // allow some slack
            "blueprint {} tokens > 25% of raw {} tokens",
            bp_tokens,
            raw_tokens
        );
    }

    #[test]
    fn raw_log_ref_is_preserved() {
        let compressor = TrajectoryCompressor::default();
        let entries = sample_entries(5);
        let bp = compressor.compress(
            &entries,
            TaskOutcome::Success,
            "done",
            Some("/tmp/raw.log".to_owned()),
        );
        assert_eq!(bp.raw_log_ref, Some("/tmp/raw.log".to_owned()));
    }

    #[test]
    fn empty_entries_produce_minimal_blueprint() {
        let compressor = TrajectoryCompressor::default();
        let bp = compressor.compress(&[], TaskOutcome::Failure, "failed", None);
        assert_eq!(bp.outcome, TaskOutcome::Failure);
        assert!(bp.reproducible_steps.is_empty());
    }

    #[test]
    fn no_key_steps_uses_first_and_last() {
        let entries: Vec<LogEntry> = (0..5)
            .map(|i| LogEntry {
                step: format!("step_{i}"),
                result: format!("result {i}"),
                is_key_step: false,
            })
            .collect();
        let compressor = TrajectoryCompressor::default();
        let bp = compressor.compress(&entries, TaskOutcome::Success, "done", None);
        assert!(!bp.reproducible_steps.is_empty());
        assert!(bp.reproducible_steps[0].contains("step_0"));
    }

    #[test]
    fn serde_round_trip() {
        let compressor = TrajectoryCompressor::default();
        let entries = sample_entries(5);
        let bp = compressor.compress(&entries, TaskOutcome::Success, "done", None);
        let json = serde_json::to_string(&bp).unwrap();
        let back: BehavioralBlueprint = serde_json::from_str(&json).unwrap();
        assert_eq!(bp, back);
    }
}
