//! SHA-256 verified update staging (`OtaStagedFile`) and GitHub Releases update checker.
//!
//! Contracts:
//! - Verifies exact byte size + SHA-256 digest before staging is considered valid.
//! - Blocks installing an update while any generation run is active or queued.
//! - Uses a zero-dependency FIPS 180-4 SHA-256 implementation so binary size stays minimal.

use serde_json::Value;
use std::fs::{self, File};
use std::io::{BufReader, Read as _};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct UpdateReleaseInfo {
    pub tag_name: String,
    pub release_url: String,
    pub asset_name: String,
    pub expected_sha256_hex: Option<String>,
    pub expected_size_bytes: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct OtaStagedFile {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub sha256_hex: String,
}

const MAX_OTA_STAGED_BYTES: u64 = 256 * 1024 * 1024;

impl OtaStagedFile {
    /// Verifies exact file size and SHA-256 digest of a downloaded update artifact.
    pub fn verify_staged(
        path: &Path,
        expected_size: Option<u64>,
        expected_sha256_hex: &str,
    ) -> Result<Self, String> {
        let meta_size = fs::metadata(path)
            .map_err(|e| format!("Could not read staged update: {e}"))?
            .len();
        if meta_size > MAX_OTA_STAGED_BYTES {
            return Err(format!(
                "Staged update ({meta_size} bytes) exceeds maximum allowed size ({MAX_OTA_STAGED_BYTES} bytes)."
            ));
        }
        if let Some(exp) = expected_size
            && meta_size != exp
        {
            return Err(format!(
                "Update size mismatch: expected {exp} bytes, got {meta_size} bytes."
            ));
        }
        let file = File::open(path).map_err(|e| format!("Could not read staged update: {e}"))?;
        let mut reader = BufReader::with_capacity(64 * 1024, file);
        let mut hasher = Sha256Hasher::new();
        let mut buf = [0u8; 64 * 1024];
        let mut actual_size: u64 = 0;
        loop {
            let n = reader
                .read(&mut buf)
                .map_err(|e| format!("Could not read staged update: {e}"))?;
            if n == 0 {
                break;
            }
            actual_size = actual_size.saturating_add(n as u64);
            if actual_size > MAX_OTA_STAGED_BYTES {
                return Err(format!(
                    "Staged update ({actual_size} bytes) exceeds maximum allowed size ({MAX_OTA_STAGED_BYTES} bytes)."
                ));
            }
            hasher.update(&buf[..n]);
        }
        if let Some(exp) = expected_size
            && actual_size != exp
        {
            return Err(format!(
                "Update size mismatch: expected {exp} bytes, got {actual_size} bytes."
            ));
        }
        let actual_sha = hasher.finalize_hex();
        let want = expected_sha256_hex.trim().to_ascii_lowercase();
        if want.len() != 64 || actual_sha != want {
            return Err(format!(
                "Update checksum mismatch: expected {want}, got {actual_sha}."
            ));
        }
        Ok(Self {
            path: path.to_path_buf(),
            size_bytes: actual_size,
            sha256_hex: actual_sha,
        })
    }

    /// Guarded install preflight: blocks installing while any chat run is active or queued.
    pub fn ensure_safe_to_install(&self, active_or_queued_runs: usize) -> Result<(), String> {
        if active_or_queued_runs > 0 {
            return Err("Finish or stop active replies before installing the update.".to_string());
        }
        if !self.path.is_file() {
            return Err("Staged update file no longer exists.".to_string());
        }
        Ok(())
    }
}

/// Checks GitHub Releases API (`repos/<owner>/<repo>/releases/latest`) for a newer Windows installer.
pub async fn check_latest_github_release(
    http: &reqwest::Client,
    repo: &str,
    current_version: &str,
) -> Result<Option<UpdateReleaseInfo>, String> {
    let clean_repo = repo.trim().trim_matches('/');
    if clean_repo.is_empty() || !clean_repo.contains('/') {
        return Err("Update repository must be in 'owner/repo' format.".into());
    }
    let url = format!("https://api.github.com/repos/{clean_repo}/releases/latest");
    let resp = http
        .get(&url)
        .header("User-Agent", format!("YL-Axiom/{current_version}"))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("Update check failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Update server returned HTTP {}.", resp.status()));
    }
    let body: Value = resp
        .json()
        .await
        .map_err(|e| format!("Invalid release JSON: {e}"))?;
    let tag = body
        .get("tag_name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if tag.is_empty() || !is_newer_semver(tag, current_version) {
        return Ok(None);
    }
    let release_url = body
        .get("html_url")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let assets = body
        .get("assets")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let exe_asset = assets.iter().find(|a| {
        a.get("name")
            .and_then(Value::as_str)
            .is_some_and(|n| n.to_ascii_lowercase().ends_with(".exe"))
    });
    let Some(asset) = exe_asset else {
        return Ok(None);
    };
    let asset_name = asset
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let expected_size = asset.get("size").and_then(Value::as_u64);
    let expected_sha = asset
        .get("digest")
        .and_then(Value::as_str)
        .and_then(|d| d.strip_prefix("sha256:"))
        .map(str::to_string);

    Ok(Some(UpdateReleaseInfo {
        tag_name: tag.to_string(),
        release_url,
        asset_name,
        expected_sha256_hex: expected_sha,
        expected_size_bytes: expected_size,
    }))
}

/// Returns true iff `candidate` is strictly newer than `current` per SemVer 2.0
/// (including numeric vs lexicographic prerelease identifier comparison, e.g.
/// `3.1.0-rc.10 > 3.1.0-rc.2` and `3.1.0 > 3.1.0-rc.10`).
pub fn is_newer_semver(candidate: &str, current: &str) -> bool {
    fn parse_semver(raw: &str) -> Option<(u64, u64, u64, Vec<&str>)> {
        let s = raw.trim().trim_start_matches(['v', 'V']);
        let s = s.split('+').next().unwrap_or(s);
        let (core, pre) = match s.split_once('-') {
            Some((c, p)) if !p.is_empty() => (c, p.split('.').collect()),
            Some((c, _)) => (c, Vec::new()),
            None => (s, Vec::new()),
        };
        let mut parts = core.split('.');
        let major = parts.next()?.parse::<u64>().ok()?;
        let minor = parts.next().unwrap_or("0").parse::<u64>().ok()?;
        let patch = parts.next().unwrap_or("0").parse::<u64>().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some((major, minor, patch, pre))
    }
    let (Some((cmaj, cmin, cpat, cpre)), Some((maj, min, pat, pre))) =
        (parse_semver(candidate), parse_semver(current))
    else {
        return false;
    };
    match (cmaj, cmin, cpat).cmp(&(maj, min, pat)) {
        std::cmp::Ordering::Greater => return true,
        std::cmp::Ordering::Less => return false,
        std::cmp::Ordering::Equal => {}
    }
    match (cpre.is_empty(), pre.is_empty()) {
        (true, true) => false,
        (true, false) => true,
        (false, true) => false,
        (false, false) => {
            for (a, b) in cpre.iter().zip(pre.iter()) {
                let ord = match (a.parse::<u64>(), b.parse::<u64>()) {
                    (Ok(na), Ok(nb)) => na.cmp(&nb),
                    (Ok(_), Err(_)) => std::cmp::Ordering::Less,
                    (Err(_), Ok(_)) => std::cmp::Ordering::Greater,
                    (Err(_), Err(_)) => a.cmp(b),
                };
                if ord != std::cmp::Ordering::Equal {
                    return ord == std::cmp::Ordering::Greater;
                }
            }
            cpre.len() > pre.len()
        }
    }
}

/// Streaming FIPS 180-4 SHA-256 hasher (zero heap allocation during `update`).
pub struct Sha256Hasher {
    h: [u32; 8],
    buf: [u8; 64],
    buf_len: usize,
    total_bytes: u64,
}

impl Sha256Hasher {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    pub fn new() -> Self {
        Self {
            h: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
            buf: [0u8; 64],
            buf_len: 0,
            total_bytes: 0,
        }
    }

    fn compress(h: &mut [u32; 8], block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for (i, chunk) in block.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(*chunk);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = *h;
        for (k_val, w_val) in Self::K.iter().zip(w.iter()) {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(*k_val)
                .wrapping_add(*w_val);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (slot, delta) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *slot = slot.wrapping_add(delta);
        }
    }

    pub fn update(&mut self, mut data: &[u8]) {
        self.total_bytes = self.total_bytes.saturating_add(data.len() as u64);
        if self.buf_len > 0 {
            let need = 64 - self.buf_len;
            let take = need.min(data.len());
            self.buf[self.buf_len..self.buf_len + take].copy_from_slice(&data[..take]);
            self.buf_len += take;
            data = &data[take..];
            if self.buf_len == 64 {
                let block = self.buf;
                Self::compress(&mut self.h, &block);
                self.buf_len = 0;
            }
        }
        let (full_blocks, rem) = data.as_chunks::<64>();
        for block in full_blocks {
            Self::compress(&mut self.h, block);
        }
        if !rem.is_empty() {
            self.buf[..rem.len()].copy_from_slice(rem);
            self.buf_len = rem.len();
        }
    }

    pub fn finalize_hex(mut self) -> String {
        let mut tail = [0u8; 64];
        tail[..self.buf_len].copy_from_slice(&self.buf[..self.buf_len]);
        tail[self.buf_len] = 0x80;
        if self.buf_len >= 56 {
            Self::compress(&mut self.h, &tail);
            tail.fill(0);
        }
        let bit_len = self.total_bytes.wrapping_mul(8);
        tail[56..64].copy_from_slice(&bit_len.to_be_bytes());
        Self::compress(&mut self.h, &tail);

        use std::fmt::Write as _;
        let mut out = String::with_capacity(64);
        for word in self.h {
            let _ = write!(&mut out, "{word:08x}");
        }
        out
    }
}

impl Default for Sha256Hasher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
/// Zero-copy streaming FIPS 180-4 SHA-256 hex digest (zero extra heap buffer beyond 64-char output).
pub fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256Hasher::new();
    hasher.update(data);
    hasher.finalize_hex()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_nist_vectors_boundaries_and_ota_blocks_active_runs() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // Exact 56-byte NIST two-block boundary vector:
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );

        let tmp = std::env::temp_dir().join("parchment_ota_test_artifact.bin");
        fs::write(&tmp, b"parchment-release-payload").unwrap();
        let digest = sha256_hex(b"parchment-release-payload");
        let staged = OtaStagedFile::verify_staged(&tmp, Some(25), &digest).unwrap();
        assert!(staged.ensure_safe_to_install(1).is_err());
        assert!(staged.ensure_safe_to_install(0).is_ok());
        assert!(OtaStagedFile::verify_staged(&tmp, Some(25), &"0".repeat(64)).is_err());
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn semver_comparison_handles_prereleases_and_rejects_downgrades() {
        assert!(is_newer_semver("v3.0.1", "3.0.0"));
        assert!(is_newer_semver("v3.1.0", "3.0.9"));
        assert!(is_newer_semver("3.0.0", "3.0.0-beta.1"));
        assert!(is_newer_semver("3.1.0-rc.10", "3.1.0-rc.2"));
        assert!(is_newer_semver("3.1.0-beta.1", "3.1.0-alpha.9"));
        assert!(is_newer_semver("3.1.0-rc.1.1", "3.1.0-rc.1"));
        assert!(!is_newer_semver("3.1.0-rc.2", "3.1.0-rc.10"));
        assert!(!is_newer_semver("3.1.0-rc.1", "3.1.0"));
        assert!(!is_newer_semver("v3.0.0", "3.0.0"));
        assert!(!is_newer_semver("v2.9.9", "3.0.0"));
        assert!(!is_newer_semver("not-a-version", "3.0.0"));
    }

    #[test]
    fn chunked_sha256_matches_oneshot_across_multi_chunk_file() {
        let payload: Vec<u8> = (0..(150 * 1024)).map(|i| (i % 251) as u8).collect();
        let expected = sha256_hex(&payload);
        let tmp = std::env::temp_dir().join("parchment_ota_chunked_test.bin");
        fs::write(&tmp, &payload).unwrap();
        let staged =
            OtaStagedFile::verify_staged(&tmp, Some(payload.len() as u64), &expected).unwrap();
        assert_eq!(staged.sha256_hex, expected);
        assert_eq!(staged.size_bytes, payload.len() as u64);
        let _ = fs::remove_file(&tmp);
    }
}
