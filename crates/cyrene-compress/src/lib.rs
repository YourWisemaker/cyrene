//! `cyrene-compress`: RTK-style tool-output compression for Cyrene (R20).
//!
//! Compresses tool/command output before it enters model context using four
//! strategies: smart filtering, grouping, truncation, and deduplication.
//! Preserves actionable information while reducing token count.

use std::collections::HashMap;
use std::path::PathBuf;

/// Configuration for the output compressor.
#[derive(Debug, Clone)]
pub struct CompressConfig {
    /// Maximum output length in characters after compression.
    pub max_chars: usize,
    /// Whether compression is bypassed for this session (R20.4).
    pub bypass: bool,
    /// Directory for retaining raw output on disk.
    pub retention_dir: Option<PathBuf>,
}

impl Default for CompressConfig {
    fn default() -> Self {
        Self {
            max_chars: 8000,
            bypass: false,
            retention_dir: None,
        }
    }
}

/// The result of compressing tool output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompressedOutput {
    /// The compressed text, ready for model context.
    pub text: String,
    /// Original character count before compression.
    pub original_chars: usize,
    /// Compressed character count.
    pub compressed_chars: usize,
    /// Path to the retained raw output file, if retention is enabled.
    pub raw_path: Option<PathBuf>,
}

impl CompressedOutput {
    /// Returns the compression ratio (0.0 = fully compressed, 1.0 = no compression).
    #[must_use]
    pub fn ratio(&self) -> f64 {
        if self.original_chars == 0 {
            return 1.0;
        }
        self.compressed_chars as f64 / self.original_chars as f64
    }
}

/// The output compressor: applies RTK-style strategies to reduce token usage.
pub struct OutputCompressor {
    config: CompressConfig,
}

impl OutputCompressor {
    /// Create a compressor with the given configuration.
    #[must_use]
    pub fn new(config: CompressConfig) -> Self {
        Self { config }
    }

    /// Create a compressor with default settings.
    #[must_use]
    pub fn default_compressor() -> Self {
        Self::new(CompressConfig::default())
    }

    /// Compress tool output. If bypass is enabled, returns the original unchanged.
    /// Retains raw output on disk if a retention directory is configured.
    pub fn compress(&self, raw: &str, tool_name: &str) -> CompressedOutput {
        let original_chars = raw.len();

        // Retain raw output on disk if configured.
        let raw_path = self.retain_raw(raw, tool_name);

        // If bypass is enabled, return as-is (R20.4).
        if self.config.bypass {
            return CompressedOutput {
                text: raw.to_owned(),
                original_chars,
                compressed_chars: original_chars,
                raw_path,
            };
        }

        // If already within limits, return as-is.
        if raw.len() <= self.config.max_chars {
            return CompressedOutput {
                text: raw.to_owned(),
                original_chars,
                compressed_chars: raw.len(),
                raw_path,
            };
        }

        // Apply compression strategies in order.
        let mut text = raw.to_owned();
        text = filter_noise(&text);
        text = deduplicate_lines(&text);
        text = group_similar(&text);
        text = truncate(&text, self.config.max_chars);

        let compressed_chars = text.len();
        CompressedOutput {
            text,
            original_chars,
            compressed_chars,
            raw_path,
        }
    }

    /// Retain raw output to disk, returning the path if successful.
    fn retain_raw(&self, raw: &str, tool_name: &str) -> Option<PathBuf> {
        let dir = self.config.retention_dir.as_ref()?;
        let _ = std::fs::create_dir_all(dir);
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let filename = format!("{}_{}.log", timestamp, tool_name.replace('/', "_"));
        let path = dir.join(filename);
        if std::fs::write(&path, raw).is_ok() {
            Some(path)
        } else {
            None
        }
    }
}

impl Default for OutputCompressor {
    fn default() -> Self {
        Self::default_compressor()
    }
}

// ─── Compression Strategies ──────────────────────────────────────────────────

/// Strategy 1: Filter noise — remove blank lines, trailing whitespace, common
/// boilerplate (progress bars, ANSI codes, timestamps in logs).
fn filter_noise(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut first = true;
    for line in text.lines() {
        // Avoid allocating a new String for lines without ANSI escape codes.
        if line.contains('\x1b') {
            let stripped = strip_ansi(line);
            let trimmed = stripped.trim_end();
            if !trimmed.is_empty() && !is_progress_line(trimmed) {
                if !first {
                    result.push('\n');
                }
                result.push_str(trimmed);
                first = false;
            }
        } else {
            let trimmed = line.trim_end();
            if !trimmed.is_empty() && !is_progress_line(trimmed) {
                if !first {
                    result.push('\n');
                }
                result.push_str(trimmed);
                first = false;
            }
        }
    }
    result
}

/// Strategy 2: Deduplicate consecutive identical or near-identical lines,
/// replacing runs with a count.
fn deduplicate_lines(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return String::new();
    }

    let mut result = String::with_capacity(text.len());
    let mut i = 0;
    while i < lines.len() {
        let current = lines[i];
        let mut count = 1;
        while i + count < lines.len() && lines[i + count] == current {
            count += 1;
        }
        if i > 0 {
            result.push('\n');
        }
        if count > 2 {
            result.push_str(current);
            result.push_str(&format!("  [×{count}]"));
        } else {
            for j in 0..count {
                if j > 0 {
                    result.push('\n');
                }
                result.push_str(current);
            }
        }
        i += count;
    }
    result
}

/// Strategy 3: Group similar lines (e.g. files by directory, errors by type).
fn group_similar(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() < 20 {
        return text.to_owned();
    }

    // Simple grouping: if many lines share a common prefix, collapse them.
    // Use &str slices as keys to avoid per-line String allocations.
    let mut prefix_counts: HashMap<&str, usize> = HashMap::new();
    for line in &lines {
        let prefix = char_prefix(line, 20);
        *prefix_counts.entry(prefix).or_insert(0) += 1;
    }

    // If no prefix appears more than 5 times, don't group.
    if prefix_counts.values().all(|&c| c <= 5) {
        return text.to_owned();
    }

    // Keep unique lines and collapse repeated-prefix groups.
    let mut result = String::with_capacity(text.len());
    let mut seen_prefixes: HashMap<&str, usize> = HashMap::new();
    let mut first = true;
    for line in &lines {
        let prefix = char_prefix(line, 20);
        let count = seen_prefixes.entry(prefix).or_insert(0);
        *count += 1;
        let line_str = if *count <= 3 || prefix_counts.get(prefix).copied().unwrap_or(0) <= 5 {
            Some((*line).to_owned())
        } else if *count == 4 {
            let total = prefix_counts.get(prefix).copied().unwrap_or(0);
            Some(format!("  ... ({} more similar lines)", total - 3))
        } else {
            None
        };
        if let Some(s) = line_str {
            if !first {
                result.push('\n');
            }
            result.push_str(&s);
            first = false;
        }
    }
    result
}

/// Returns a `&str` slice of up to `n` Unicode characters from `s`.
fn char_prefix(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

/// Strategy 4: Truncate to max_chars, keeping the head and tail.
fn truncate(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        return text.to_owned();
    }
    let head_size = max_chars * 3 / 4;
    let tail_size = max_chars - head_size - 50; // 50 chars for the separator
    let head = &text[..head_size];
    let tail = &text[text.len() - tail_size..];
    let omitted = text.len() - head_size - tail_size;
    format!("{head}\n\n[... {omitted} chars omitted ...]\n\n{tail}")
}

/// Strip ANSI escape codes from a line.
fn strip_ansi(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut in_escape = false;
    for ch in text.chars() {
        if ch == '\x1b' {
            in_escape = true;
        } else if in_escape {
            if ch.is_ascii_alphabetic() {
                in_escape = false;
            }
        } else {
            result.push(ch);
        }
    }
    result
}

/// Detect progress/spinner lines that add no information.
fn is_progress_line(line: &str) -> bool {
    // Common patterns: percentage bars, spinner chars, "Downloading..." repeated
    line.contains("━")
        || line.contains("█")
        || line.contains("▓")
        || (line.contains('%') && line.contains('/') && line.len() < 80)
        || line.starts_with("Enumerating objects:")
        || line.starts_with("Counting objects:")
        || line.starts_with("Compressing objects:")
        || line.starts_with("Delta compression")
}

/// Returns the stable identifier of this subsystem crate.
#[must_use]
pub fn subsystem() -> &'static str {
    "cyrene-compress"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subsystem_id_is_nonempty() {
        assert!(!subsystem().is_empty());
    }

    #[test]
    fn short_output_passes_through() {
        let c = OutputCompressor::default_compressor();
        let result = c.compress("hello world", "test");
        assert_eq!(result.text, "hello world");
        assert_eq!(result.ratio(), 1.0);
    }

    #[test]
    fn bypass_returns_original() {
        let config = CompressConfig {
            bypass: true,
            ..Default::default()
        };
        let c = OutputCompressor::new(config);
        let long = "x".repeat(20000);
        let result = c.compress(&long, "test");
        assert_eq!(result.text, long);
        assert_eq!(result.compressed_chars, result.original_chars);
    }

    #[test]
    fn deduplication_collapses_repeated_lines() {
        let input = "line A\nline A\nline A\nline A\nline A\nline B\n";
        let result = deduplicate_lines(input);
        assert!(result.contains("[×5]"));
        assert!(result.contains("line B"));
        assert!(result.lines().count() < input.lines().count());
    }

    #[test]
    fn ansi_codes_are_stripped() {
        let input = "\x1b[32mSuccess\x1b[0m: all tests passed";
        let result = strip_ansi(input);
        assert_eq!(result, "Success: all tests passed");
    }

    #[test]
    fn progress_lines_are_filtered() {
        let input = "Building...\nEnumerating objects: 5, done.\nCounting objects: 100%\nActual output here\n";
        let result = filter_noise(input);
        assert!(result.contains("Actual output here"));
        assert!(!result.contains("Enumerating objects"));
        assert!(!result.contains("Counting objects"));
    }

    #[test]
    fn truncation_keeps_head_and_tail() {
        let long = (0..1000)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let result = truncate(&long, 500);
        assert!(result.len() <= 600); // some slack for the separator
        assert!(result.contains("line 0"));
        assert!(result.contains("omitted"));
    }

    #[test]
    fn large_output_is_compressed() {
        let c = OutputCompressor::new(CompressConfig {
            max_chars: 200,
            ..Default::default()
        });
        let long = (0..500)
            .map(|i| format!("output line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let result = c.compress(&long, "test");
        assert!(result.compressed_chars < result.original_chars);
        assert!(result.compressed_chars <= 300); // within reasonable bounds
    }

    #[test]
    fn retention_writes_raw_to_disk() {
        let dir = tempfile::tempdir().unwrap();
        let config = CompressConfig {
            retention_dir: Some(dir.path().to_path_buf()),
            ..Default::default()
        };
        let c = OutputCompressor::new(config);
        let result = c.compress("raw output data", "git_status");
        assert!(result.raw_path.is_some());
        let path = result.raw_path.unwrap();
        assert!(path.exists());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "raw output data");
    }

    #[test]
    fn no_retention_dir_means_no_raw_path() {
        let c = OutputCompressor::default_compressor();
        let result = c.compress("data", "test");
        assert!(result.raw_path.is_none());
    }

    #[test]
    fn empty_input_handled() {
        let c = OutputCompressor::default_compressor();
        let result = c.compress("", "test");
        assert_eq!(result.text, "");
        assert_eq!(result.original_chars, 0);
    }
}
