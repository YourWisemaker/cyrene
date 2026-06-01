//! Property tests for tool-output compression (Property 11, R20.2/20.3).
//!
//! Property 11: Compressed tool output retains the information needed to act,
//! and adds no more than the configured per-output overhead budget.

use cyrene_compress::{CompressConfig, OutputCompressor};
use proptest::prelude::*;
use std::time::Instant;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// Property 11a: Compression preserves actionable information.
    ///
    /// For any tool output containing "key lines" (error messages, file paths,
    /// status indicators), the compressed output retains at least the first
    /// occurrence of each key line (unless the output is extremely long and
    /// truncation is necessary, in which case the head is preserved).
    ///
    /// **Validates: Requirements 20.2**
    #[test]
    fn prop11a_compression_preserves_key_info(
        prefix_lines in prop::collection::vec("[a-z ]{5,40}", 0..20),
        key_line in "(ERROR|FAIL|error:|warning:)[a-zA-Z0-9 :._/-]{5,60}",
        suffix_lines in prop::collection::vec("[a-z ]{5,40}", 0..20),
    ) {
        let mut lines = prefix_lines;
        lines.push(key_line.clone());
        lines.extend(suffix_lines);
        let input = lines.join("\n");

        let config = CompressConfig { max_chars: 4000, ..Default::default() };
        let compressor = OutputCompressor::new(config);
        let result = compressor.compress(&input, "test");

        // The key line (or its beginning) should be preserved in the output
        // since it's actionable information and the output is within limits.
        if input.len() <= 4000 {
            prop_assert!(
                result.text.contains(&key_line),
                "Key line was lost during compression: {:?}",
                key_line
            );
        } else {
            // For very long outputs, at least the first part should be preserved
            // (head truncation keeps the beginning).
            let key_start = &key_line[..key_line.len().min(20)];
            prop_assert!(
                result.text.contains(key_start) || result.text.contains("omitted"),
                "Neither key line nor truncation marker found"
            );
        }
    }

    /// Property 11b: Compression overhead stays within budget.
    ///
    /// For any tool output, compression completes within 10ms (the per-output
    /// overhead budget from R20.3). This is a timing property — we measure
    /// wall-clock time and assert it stays under the budget.
    ///
    /// **Validates: Requirements 20.3**
    #[test]
    fn prop11b_compression_overhead_within_budget(
        output in "[a-zA-Z0-9 .,\n:/-]{100,50000}",
    ) {
        let compressor = OutputCompressor::default();
        let start = Instant::now();
        let _result = compressor.compress(&output, "bench_tool");
        let elapsed = start.elapsed();

        // R20.3: no more than 10ms per output in release; allow 50ms in debug.
        let budget_ms = if cfg!(debug_assertions) { 50 } else { 10 };
        prop_assert!(
            elapsed.as_millis() <= budget_ms,
            "Compression took {}ms, exceeding the {}ms budget",
            elapsed.as_millis(),
            budget_ms
        );
    }

    /// Property 11c: Compressed output is never larger than the original.
    ///
    /// Compression should never inflate the output.
    #[test]
    fn prop11c_compression_never_inflates(
        output in "[a-zA-Z0-9 .,\n:/-]{1,20000}",
    ) {
        let compressor = OutputCompressor::default();
        let result = compressor.compress(&output, "test");
        prop_assert!(
            result.compressed_chars <= result.original_chars,
            "Compression inflated: {} -> {}",
            result.original_chars,
            result.compressed_chars
        );
    }
}
