#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &str| {
    let _ = cyrene_config::Config::parse(data, "fuzz.toml");
});
