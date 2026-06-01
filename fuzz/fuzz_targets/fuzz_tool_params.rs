#![no_main]
use libfuzzer_sys::fuzz_target;
use cyrene_core::ToolCall;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        if let Ok(args) = serde_json::from_str::<serde_json::Value>(s) {
            let _call = ToolCall::new("test", args);
        }
    }
});
