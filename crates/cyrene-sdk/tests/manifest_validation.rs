use cyrene_sdk::{Capability, ExtensionManifest, Permissions, SdkError};

#[test]
fn reject_incompatible_host() {
    let manifest = ExtensionManifest {
        name: "legacy-ext".to_owned(),
        version: "1.0.0".to_owned(),
        description: String::new(),
        capabilities: vec![Capability::Tool],
        permissions: Permissions::default(),
        host_compat: "<0.1.0".to_owned(),
    };
    let err = manifest.check_host_compat("0.1.0").unwrap_err();
    assert!(matches!(err, SdkError::HostIncompatible { .. }));
}

#[test]
fn accept_compatible_host() {
    let manifest = ExtensionManifest {
        name: "good-ext".to_owned(),
        version: "1.0.0".to_owned(),
        description: String::new(),
        capabilities: vec![Capability::Channel],
        permissions: Permissions::default(),
        host_compat: "*".to_owned(),
    };
    assert!(manifest.check_host_compat("0.1.0").is_ok());
}
