use super::{download, install, platform, release};

const RELEASE_JSON: &str = r#"{
  "tag_name": "v0.2.0",
  "html_url": "https://github.com/seashyne/mybot-cli/releases/tag/v0.2.0",
  "published_at": "2026-11-22T00:00:00Z",
  "assets": [
    {
      "name": "mybot-x86_64-unknown-linux-gnu",
      "browser_download_url": "https://example.com/mybot-x86_64-unknown-linux-gnu",
      "size": 1234
    },
    {
      "name": "sha256sums.txt",
      "browser_download_url": "https://example.com/sha256sums.txt",
      "size": 64
    }
  ]
}"#;

#[test]
fn parses_a_release_response() {
    let release = release::from_json(RELEASE_JSON).unwrap();
    assert_eq!(release.tag, "v0.2.0");
    assert_eq!(release.version, semver::Version::new(0, 2, 0));
    assert!(release.asset("mybot-x86_64-unknown-linux-gnu").is_some());
    assert!(release.asset("sha256sums.txt").is_some());
    assert!(release.asset("missing").is_none());
}

#[test]
fn parses_tags_with_and_without_a_v_prefix() {
    assert_eq!(release::parse_tag("v1.2.3").unwrap().to_string(), "1.2.3");
    assert_eq!(release::parse_tag("1.2.3").unwrap().to_string(), "1.2.3");
    assert!(release::parse_tag("nightly").is_err());
}

#[test]
fn maps_targets_to_release_assets() {
    assert_eq!(
        platform::asset_name_for("x86_64-unknown-linux-gnu").unwrap(),
        "mybot-x86_64-unknown-linux-gnu"
    );
    assert_eq!(
        platform::asset_name_for("aarch64-apple-darwin").unwrap(),
        "mybot-aarch64-apple-darwin"
    );
    assert_eq!(
        platform::asset_name_for("x86_64-pc-windows-msvc").unwrap(),
        "mybot-x86_64-pc-windows-msvc.exe"
    );
    assert!(platform::asset_name_for("riscv64gc-unknown-linux-gnu").is_err());
}

#[test]
fn finds_the_checksum_for_an_asset() {
    let first = "a".repeat(64);
    let second = "b".repeat(64);
    let sums = format!(
        "{first}  mybot-x86_64-unknown-linux-gnu\n{second} *mybot-aarch64-apple-darwin\n"
    );

    assert_eq!(
        download::expected_hash(&sums, "mybot-x86_64-unknown-linux-gnu"),
        Some(first)
    );
    assert_eq!(
        download::expected_hash(&sums, "mybot-aarch64-apple-darwin"),
        Some(second)
    );
    assert!(download::expected_hash(&sums, "mybot-unknown").is_none());
}

#[test]
fn verifies_sha256_digests() {
    let expected = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
    assert!(download::verify(b"hello", expected).is_ok());
    assert!(download::verify(b"hello!", expected).is_err());
}

#[cfg(unix)]
mod unix {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    use super::*;

    fn script(version: &str) -> Vec<u8> {
        format!("#!/bin/sh\necho 'mybot {version} (test)'\n").into_bytes()
    }

    fn executable(path: &std::path::Path, body: &[u8]) {
        std::fs::write(path, body).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn replaces_the_target_binary() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("mybot");
        executable(&target, &script("0.1.0"));

        install::replace(&script("9.9.9"), &target, "9.9.9").unwrap();

        let output = Command::new(&target).arg("--version").output().unwrap();
        assert!(String::from_utf8_lossy(&output.stdout).contains("9.9.9"));
        let leftovers = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_string_lossy().contains(".new"))
            .count();
        assert_eq!(leftovers, 0);
    }

    #[test]
    fn rejects_a_binary_with_the_wrong_version() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("mybot");
        executable(&target, &script("0.1.0"));

        let error = install::replace(&script("0.0.1"), &target, "9.9.9").unwrap_err();
        assert!(error.to_string().contains("unexpected version"));

        let output = Command::new(&target).arg("--version").output().unwrap();
        assert!(String::from_utf8_lossy(&output.stdout).contains("0.1.0"));
    }
}
