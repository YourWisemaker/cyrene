#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _v: Result<serde_json::Value, _> = serde_json::from_str(s);
    }
});
