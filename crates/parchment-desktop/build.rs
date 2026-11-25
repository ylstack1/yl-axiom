#![allow(clippy::print_stdout)]

use std::env;
use std::error::Error;
use std::fs;
use std::path::PathBuf;

const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;
const RT_VERSION: u16 = 16;
const GROUP_ICON_ID: u16 = 1;
const VERSION_INFO_ID: u16 = 1;
const LANG_EN_US: u16 = 0x0409;
fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-env-changed=PARCHMENT_APP_VERSION");
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let rp_manifest = manifest_dir
        .join("..")
        .join("..")
        .join(".release-please-manifest.json");
    println!("cargo:rerun-if-changed={}", rp_manifest.display());

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return Ok(());
    }

    let ico_path = manifest_dir
        .join("..")
        .join("..")
        .join("installer")
        .join("parchment-icon.ico");
    println!("cargo:rerun-if-changed={}", ico_path.display());

    let app_version = resolve_app_version(&rp_manifest)?;
    let ico = fs::read(&ico_path)?;
    let res = build_windows_resources(&ico, &app_version)?;

    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    let res_path = out_dir.join("parchment-icon.res");
    fs::write(&res_path, res)?;
    println!("cargo:rustc-link-arg-bins={}", res_path.display());
    Ok(())
}

/// Builds a standard 32-bit Windows `.res` resource file containing `RT_ICON`,
/// `RT_GROUP_ICON` ID `1`, and `RT_VERSION` (`VS_VERSION_INFO`) ID `1`.
fn build_windows_resources(ico: &[u8], app_version: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    if ico.len() < 6 {
        return Err("parchment-icon.ico is truncated".into());
    }
    let reserved = u16::from_le_bytes([ico[0], ico[1]]);
    let ico_type = u16::from_le_bytes([ico[2], ico[3]]);
    let count = u16::from_le_bytes([ico[4], ico[5]]);
    if reserved != 0 || ico_type != 1 || count == 0 {
        return Err("parchment-icon.ico has an invalid ICO header".into());
    }
    let dir_end = 6usize
        .checked_add(usize::from(count) * 16)
        .ok_or("ICO directory overflow")?;
    if dir_end > ico.len() {
        return Err("parchment-icon.ico directory exceeds file size".into());
    }

    let mut out = Vec::with_capacity(ico.len() + 512);
    // Standard 32-byte leading marker record for 32-bit RES files.
    write_res_record(&mut out, 0, 0, 0, 0, &[]);

    let mut grp = Vec::with_capacity(6 + usize::from(count) * 14);
    grp.extend_from_slice(&0u16.to_le_bytes());
    grp.extend_from_slice(&1u16.to_le_bytes());
    grp.extend_from_slice(&count.to_le_bytes());

    for i in 0..usize::from(count) {
        let off = 6 + i * 16;
        let width = ico[off];
        let height = ico[off + 1];
        let color_count = ico[off + 2];
        let entry_reserved = ico[off + 3];
        let raw_planes = u16::from_le_bytes([ico[off + 4], ico[off + 5]]);
        let planes = if raw_planes == 0 { 1 } else { raw_planes };
        let bpp = u16::from_le_bytes([ico[off + 6], ico[off + 7]]);
        let size = u32::from_le_bytes([ico[off + 8], ico[off + 9], ico[off + 10], ico[off + 11]]);
        let img_off =
            u32::from_le_bytes([ico[off + 12], ico[off + 13], ico[off + 14], ico[off + 15]]);

        let start = img_off as usize;
        let end = start
            .checked_add(size as usize)
            .ok_or("ICO entry offset overflow")?;
        if end > ico.len() {
            return Err("ICO entry image data out of bounds".into());
        }

        let icon_id = (i + 1) as u16;
        // MOVEABLE (0x0010) | DISCARDABLE (0x1000)
        write_res_record(
            &mut out,
            RT_ICON,
            icon_id,
            0x1010,
            LANG_EN_US,
            &ico[start..end],
        );

        grp.push(width);
        grp.push(height);
        grp.push(color_count);
        grp.push(entry_reserved);
        grp.extend_from_slice(&planes.to_le_bytes());
        grp.extend_from_slice(&bpp.to_le_bytes());
        grp.extend_from_slice(&size.to_le_bytes());
        grp.extend_from_slice(&icon_id.to_le_bytes());
    }

    // MOVEABLE (0x0010) | PURE (0x0020) | DISCARDABLE (0x1000)
    write_res_record(
        &mut out,
        RT_GROUP_ICON,
        GROUP_ICON_ID,
        0x1030,
        LANG_EN_US,
        &grp,
    );

    let version_blob = build_vs_version_info(app_version)?;
    // MOVEABLE (0x0010) | PURE (0x0020)
    write_res_record(
        &mut out,
        RT_VERSION,
        VERSION_INFO_ID,
        0x0030,
        LANG_EN_US,
        &version_blob,
    );
    Ok(out)
}

fn write_res_record(
    out: &mut Vec<u8>,
    res_type: u16,
    res_id: u16,
    mem_flags: u16,
    lang_id: u16,
    data: &[u8],
) {
    out.extend_from_slice(&(data.len() as u32).to_le_bytes()); // DataSize
    out.extend_from_slice(&32u32.to_le_bytes()); // HeaderSize
    out.extend_from_slice(&0xFFFFu16.to_le_bytes());
    out.extend_from_slice(&res_type.to_le_bytes());
    out.extend_from_slice(&0xFFFFu16.to_le_bytes());
    out.extend_from_slice(&res_id.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // DataVersion
    out.extend_from_slice(&mem_flags.to_le_bytes());
    out.extend_from_slice(&lang_id.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // Version
    out.extend_from_slice(&0u32.to_le_bytes()); // Characteristics
    out.extend_from_slice(data);
    let pad = (4 - (data.len() & 3)) & 3;
    out.extend_from_slice(&[0u8; 3][..pad]);
}

fn resolve_app_version(manifest_path: &PathBuf) -> Result<String, Box<dyn Error>> {
    if let Ok(raw) = env::var("PARCHMENT_APP_VERSION") {
        let trimmed = raw.trim().trim_start_matches(['v', 'V']);
        if !trimmed.is_empty() {
            parse_semver_triplet(trimmed)?;
            return Ok(trimmed.to_string());
        }
    }
    let text = fs::read_to_string(manifest_path)?;
    let parsed = parse_root_manifest_version(&text)
        .ok_or("Could not find root '.' version in .release-please-manifest.json")?;
    parse_semver_triplet(&parsed)?;
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

fn parse_semver_triplet(ver: &str) -> Result<(u16, u16, u16), Box<dyn Error>> {
    let core = ver.split('-').next().unwrap_or(ver);
    let mut parts = core.split('.');
    let major = parts
        .next()
        .ok_or("Missing major version")?
        .parse::<u16>()?;
    let minor = parts
        .next()
        .ok_or("Missing minor version")?
        .parse::<u16>()?;
    let patch = parts
        .next()
        .ok_or("Missing patch version")?
        .parse::<u16>()?;
    if parts.next().is_some() {
        return Err(format!("Unexpected extra version components in {ver}").into());
    }
    Ok((major, minor, patch))
}

fn align4(buf: &mut Vec<u8>) {
    let pad = (4 - (buf.len() & 3)) & 3;
    buf.extend_from_slice(&[0u8; 3][..pad]);
}

fn push_utf16_nul(buf: &mut Vec<u8>, s: &str) -> usize {
    let mut units = 0usize;
    for u in s.encode_utf16() {
        buf.extend_from_slice(&u.to_le_bytes());
        units += 1;
    }
    buf.extend_from_slice(&0u16.to_le_bytes());
    units + 1
}

fn build_string_entry(key: &str, value: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut buf = Vec::with_capacity(64);
    buf.extend_from_slice(&0u16.to_le_bytes()); // wLength placeholder
    buf.extend_from_slice(&0u16.to_le_bytes()); // wValueLength placeholder (in UTF-16 units)
    buf.extend_from_slice(&1u16.to_le_bytes()); // wType = 1 (text)
    push_utf16_nul(&mut buf, key);
    align4(&mut buf);
    let val_units = push_utf16_nul(&mut buf, value);
    let len = u16::try_from(buf.len()).map_err(|_| "VS_VERSION_INFO string block overflow")?;
    let vlen = u16::try_from(val_units).map_err(|_| "VS_VERSION_INFO string value overflow")?;
    buf[0..2].copy_from_slice(&len.to_le_bytes());
    buf[2..4].copy_from_slice(&vlen.to_le_bytes());
    Ok(buf)
}

fn build_container_block(
    key: &str,
    w_type: u16,
    value: &[u8],
    w_value_len: u16,
    children: &[Vec<u8>],
) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut buf = Vec::with_capacity(128);
    buf.extend_from_slice(&0u16.to_le_bytes()); // wLength placeholder
    buf.extend_from_slice(&w_value_len.to_le_bytes());
    buf.extend_from_slice(&w_type.to_le_bytes());
    push_utf16_nul(&mut buf, key);
    if !value.is_empty() {
        align4(&mut buf);
        buf.extend_from_slice(value);
    }
    for child in children {
        align4(&mut buf);
        buf.extend_from_slice(child);
    }
    let len = u16::try_from(buf.len()).map_err(|_| "VS_VERSION_INFO container overflow")?;
    buf[0..2].copy_from_slice(&len.to_le_bytes());
    Ok(buf)
}

fn build_vs_version_info(app_version: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    let (major, minor, patch) = parse_semver_triplet(app_version)?;
    let ver_ms = (u32::from(major) << 16) | u32::from(minor);
    let ver_ls = u32::from(patch) << 16;
    let file_version_str = format!("{major}.{minor}.{patch}.0");

    let mut fixed = Vec::with_capacity(52);
    for word in [
        0xFEEF_04BDu32, // dwSignature
        0x0001_0000u32, // dwStrucVersion
        ver_ms,         // dwFileVersionMS
        ver_ls,         // dwFileVersionLS
        ver_ms,         // dwProductVersionMS
        ver_ls,         // dwProductVersionLS
        0x0000_003Fu32, // dwFileFlagsMask
        0x0000_0000u32, // dwFileFlags
        0x0004_0004u32, // dwFileOS (VOS_NT_WINDOWS32)
        0x0000_0001u32, // dwFileType (VFT_APP)
        0x0000_0000u32, // dwFileSubtype
        0x0000_0000u32, // dwFileDateMS
        0x0000_0000u32, // dwFileDateLS
    ] {
        fixed.extend_from_slice(&word.to_le_bytes());
    }

    let string_entries = vec![
        build_string_entry("CompanyName", "YL Stack")?,
        build_string_entry("FileDescription", "YL Axiom Desktop")?,
        build_string_entry("FileVersion", &file_version_str)?,
        build_string_entry("InternalName", "YL Axiom")?,
        build_string_entry("OriginalFilename", "YL-Axiom.exe")?,
        build_string_entry("ProductName", "YL Axiom")?,
        build_string_entry("ProductVersion", app_version)?,
    ];
    let string_table = build_container_block("040904B0", 1, &[], 0, &string_entries)?;
    let string_file_info = build_container_block("StringFileInfo", 1, &[], 0, &[string_table])?;

    let translation: [u8; 4] = [0x09, 0x04, 0xB0, 0x04];
    let var_entry = build_container_block("Translation", 0, &translation, 4, &[])?;
    let var_file_info = build_container_block("VarFileInfo", 1, &[], 0, &[var_entry])?;

    build_container_block(
        "VS_VERSION_INFO",
        0,
        &fixed,
        fixed.len() as u16,
        &[string_file_info, var_file_info],
    )
}
