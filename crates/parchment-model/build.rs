#![allow(clippy::print_stdout)]

use std::env;
use std::error::Error;
use std::fs;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-env-changed=PARCHMENT_APP_VERSION");
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let rp_manifest = manifest_dir
        .join("..")
        .join("..")
        .join(".release-please-manifest.json");
    println!("cargo:rerun-if-changed={}", rp_manifest.display());

    let resolved = resolve_app_version(&rp_manifest)?;
    println!("cargo:rustc-env=PARCHMENT_APP_VERSION={resolved}");
    Ok(())
}

fn resolve_app_version(manifest_path: &PathBuf) -> Result<String, Box<dyn Error>> {
    if let Ok(raw) = env::var("PARCHMENT_APP_VERSION") {
        let trimmed = raw.trim().trim_start_matches(['v', 'V']);
        if !trimmed.is_empty() {
            validate_semver(trimmed)?;
            return Ok(trimmed.to_string());
        }
    }
    let text = fs::read_to_string(manifest_path)?;
    let parsed = parse_root_manifest_version(&text)
        .ok_or("Could not find root '.' version in .release-please-manifest.json")?;
    validate_semver(&parsed)?;
    Ok(parsed)
}

fn parse_root_manifest_version(json: &str) -> Option<String> {
    let idx = json.find("\".\"")?;
    let after_key = &json[idx + 3..];
    let colon = after_key.find(':')?;
    let after_colon = after_key[colon + 1..].trim_start();
    let rest = after_colon.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(
        rest[..end]
            .trim()
            .trim_start_matches(['v', 'V'])
            .to_string(),
    )
}

fn validate_semver(v: &str) -> Result<(), Box<dyn Error>> {
    let (core, pre) = match v.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (v, None),
    };
    let mut parts = core.split('.');
    let major = parts.next().ok_or("Missing semver major")?;
    let minor = parts.next().ok_or("Missing semver minor")?;
    let patch = parts.next().ok_or("Missing semver patch")?;
    if parts.next().is_some()
        || [major, minor, patch]
            .iter()
            .any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(format!("Invalid semver core: {v}").into());
    }
    if let Some(p) = pre
        && (p.is_empty()
            || !p
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-')))
    {
        return Err(format!("Invalid semver prerelease: {v}").into());
    }
    Ok(())
}
