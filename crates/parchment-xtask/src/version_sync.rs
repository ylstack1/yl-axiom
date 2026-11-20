//! Single-source version resolution and `version-sync` quality gate.

use std::collections::BTreeMap;

use crate::quality::Problem;

pub fn parse_manifest_root_version(text: &str) -> Option<String> {
    let idx = text.find("\".\"")?;
    let after_key = &text[idx + 3..];
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

fn is_valid_semver(v: &str) -> bool {
    let (core, pre) = match v.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (v, None),
    };
    let mut parts = core.split('.');
    let Some(a) = parts.next() else { return false };
    let Some(b) = parts.next() else { return false };
    let Some(c) = parts.next() else { return false };
    if parts.next().is_some()
        || [a, b, c]
            .iter()
            .any(|p| p.is_empty() || !p.bytes().all(|ch| ch.is_ascii_digit()))
    {
        return false;
    }
    pre.is_none_or(|p| {
        !p.is_empty()
            && p.bytes()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'.' | b'-'))
    })
}

fn parse_workspace_package_version(cargo_toml: &str) -> Option<String> {
    let mut in_workspace_pkg = false;
    for raw_line in cargo_toml.lines() {
        let line = raw_line.trim();
        if line.starts_with('[') {
            in_workspace_pkg = line == "[workspace.package]";
            continue;
        }
        if in_workspace_pkg
            && let Some(rest) = line.strip_prefix("version")
            && let Some(after_eq) = rest.trim_start().strip_prefix('=')
        {
            let val = after_eq.trim();
            if let Some(quoted) = val.strip_prefix('"')
                && let Some(end) = quoted.find('"')
            {
                return Some(quoted[..end].to_string());
            }
        }
    }
    None
}

pub fn check_version_sync(files: &BTreeMap<String, Vec<u8>>, out: &mut Vec<Problem>) {
    let Some(manifest_bytes) = files.get(".release-please-manifest.json") else {
        return;
    };
    let manifest_text = String::from_utf8_lossy(manifest_bytes);
    let Some(manifest_ver) = parse_manifest_root_version(&manifest_text) else {
        out.push(Problem {
            rule: "version-sync",
            path: ".release-please-manifest.json".into(),
            line: 1,
            message: "Missing root '.' version entry.".into(),
        });
        return;
    };
    if !is_valid_semver(&manifest_ver) {
        out.push(Problem {
            rule: "version-sync",
            path: ".release-please-manifest.json".into(),
            line: 1,
            message: format!("Manifest version '{manifest_ver}' is not valid semver."),
        });
        return;
    }
    if let Some(cargo_bytes) = files.get("Cargo.toml") {
        let cargo_text = String::from_utf8_lossy(cargo_bytes);
        match parse_workspace_package_version(&cargo_text) {
            Some(v) if v == manifest_ver => {}
            Some(v) => out.push(Problem {
                rule: "version-sync",
                path: "Cargo.toml".into(),
                line: 1,
                message: format!(
                    "[workspace.package].version ('{v}') does not match .release-please-manifest.json ('{manifest_ver}')."
                ),
            }),
            None => out.push(Problem {
                rule: "version-sync",
                path: "Cargo.toml".into(),
                line: 1,
                message: "Missing [workspace.package] version field.".into(),
            }),
        }
    }
    if let Some(verify_bytes) = files.get("crates/parchment-xtask/src/verify.rs") {
        let verify_text = String::from_utf8_lossy(verify_bytes);
        for (idx, line) in verify_text.lines().enumerate() {
            if line.contains("const APP_VERSION") {
                out.push(Problem {
                    rule: "version-sync",
                    path: "crates/parchment-xtask/src/verify.rs".into(),
                    line: idx + 1,
                    message:
                        "Do not hardcode const APP_VERSION; read .release-please-manifest.json."
                            .into(),
                });
            }
        }
    }
    if let Some(mcp_bytes) = files.get("crates/parchment-harness/src/mcp.rs") {
        let mcp_text = String::from_utf8_lossy(mcp_bytes);
        for (idx, line) in mcp_text.lines().enumerate() {
            if line.contains("\"version\": \"3.") {
                out.push(Problem {
                    rule: "version-sync",
                    path: "crates/parchment-harness/src/mcp.rs".into(),
                    line: idx + 1,
                    message:
                        "Do not hardcode MCP clientInfo version; use parchment_model::APP_VERSION."
                            .into(),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quality::check_files;

    #[test]
    fn enforces_version_sync_between_manifest_cargo_and_sources() {
        let mut files = BTreeMap::new();
        files.insert("AGENTS.md".into(), b"Run cargo xtask affected.\n".to_vec());
        files.insert(
            "TODO.md".into(),
            b"## Current status\n- Source: a\n- Last verified: b\n- Next action: c\n\n## Open\n- [ ] 1.1 First: ok\n".to_vec(),
        );
        files.insert(
            ".release-please-manifest.json".into(),
            b"{\n  \".\": \"3.1.0\"\n}\n".to_vec(),
        );
        files.insert(
            "Cargo.toml".into(),
            b"[workspace.package]\nversion = \"3.0.0\"\n".to_vec(),
        );
        files.insert(
            "crates/parchment-xtask/src/verify.rs".into(),
            b"const APP_VERSION: &str = \"3.0.0\";\n".to_vec(),
        );
        files.insert(
            "crates/parchment-harness/src/mcp.rs".into(),
            br#"let x = json!({ "version": "3.0.0" });"#.to_vec(),
        );
        let probs = check_files(&files);
        let vs: Vec<_> = probs.iter().filter(|p| p.rule == "version-sync").collect();
        assert_eq!(
            vs.len(),
            3,
            "expected Cargo.toml + verify.rs + mcp.rs mismatch: {vs:?}"
        );

        files.insert(
            "Cargo.toml".into(),
            b"[workspace.package]\nversion = \"3.1.0\"\n".to_vec(),
        );
        files.insert(
            "crates/parchment-xtask/src/verify.rs".into(),
            b"// reads manifest\n".to_vec(),
        );
        files.insert(
            "crates/parchment-harness/src/mcp.rs".into(),
            b"let v = parchment_model::APP_VERSION;\n".to_vec(),
        );
        let probs_ok = check_files(&files);
        assert!(
            probs_ok.iter().all(|p| p.rule != "version-sync"),
            "unexpected version-sync problems: {probs_ok:?}"
        );
    }
}
