//! Integration tests for loading config from a real file on disk and resolving
//! secrets from a `.env` file (task 3.1, R2.5 + R22 secret hygiene).

use std::io::Write;

use cyrene_config::{Config, ConfigError, SecretResolver};

const VALID: &str = r#"
[providers.openai.coding]
model = "gpt-4o"
tier  = "Premium"
api_key_env = "CYRENE_IT_OPENAI_KEY"

[channels.cli.default]

[autonomy]
command_allowlist = ["git", "cargo"]
"#;

/// Writes `contents` to a uniquely-named temp file and returns its path.
fn write_temp(name: &str, contents: &str) -> std::path::PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("cyrene-config-it-{}-{name}", std::process::id()));
    let mut file = std::fs::File::create(&path).expect("create temp config");
    file.write_all(contents.as_bytes())
        .expect("write temp config");
    path
}

#[test]
fn loads_valid_config_from_explicit_path() {
    let path = write_temp("valid.toml", VALID);
    let cfg = Config::load_from_path(&path).expect("valid config loads");

    assert!(cfg.provider("openai", "coding").is_some());
    assert!(cfg.channel("cli", "default").is_some());
    assert!(cfg.autonomy.is_command_allowed("cargo build"));

    std::fs::remove_file(&path).ok();
}

#[test]
fn missing_file_is_io_error() {
    let path = std::env::temp_dir().join("cyrene-config-it-does-not-exist.toml");
    let _ = std::fs::remove_file(&path);
    match Config::load_from_path(&path) {
        Err(ConfigError::Io { .. }) => {}
        other => panic!("expected Io error, got {other:?}"),
    }
}

#[test]
fn missing_required_section_is_reported() {
    // Providers present but no channels -> MissingSection("channels").
    let path = write_temp(
        "no-channels.toml",
        "[providers.ollama.local]\nmodel = \"llama3.1\"\n",
    );
    match Config::load_from_path(&path) {
        Err(ConfigError::MissingSection("channels")) => {}
        other => panic!("expected MissingSection(channels), got {other:?}"),
    }
    std::fs::remove_file(&path).ok();
}

#[test]
fn secret_resolved_from_dotenv_file_not_toml() {
    // The TOML references the key by NAME; the value comes from a .env file.
    let env_path = write_temp("secret.env", "CYRENE_IT_DOTENV_KEY=sk-from-dotenv\n");
    let resolver = SecretResolver::with_dotenv_path(&env_path);
    assert_eq!(
        resolver.require("CYRENE_IT_DOTENV_KEY").unwrap(),
        "sk-from-dotenv"
    );

    // And the config that references it never contains the value itself.
    let cfg = Config::parse(VALID, "v.toml").unwrap();
    let serialized = toml::to_string(&cfg).unwrap();
    assert!(!serialized.contains("sk-"));
    assert!(serialized.contains("CYRENE_IT_OPENAI_KEY"));

    std::env::remove_var("CYRENE_IT_DOTENV_KEY");
    std::fs::remove_file(&env_path).ok();
}
