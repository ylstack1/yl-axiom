//! Zero-dependency `.docx` `word/document.xml` text extractor (XXE-free, bounded zip bomb guard)
//! and SillyTavern / Parchment PNG (`tEXt`/`iTXt`) + JSON Card, System Prompt, & Theme codec.

use std::collections::BTreeMap;
use std::io::{Cursor, Read};

use base64::Engine;
use parchment_model::{
    CharacterOrScenarioCard, ContextFragment, FragmentActivationMode, SystemPromptPreset,
    ThemeConfig,
};
use serde_json::Value;
use zip::ZipArchive;

const MAX_DOCX_ARCHIVE_BYTES: usize = 25 * 1024 * 1024;
const MAX_DOCX_XML_BYTES: usize = 16 * 1024 * 1024;
const MAX_DOCX_TEXT_CHARS: usize = 8 * 1024 * 1024;
const MAX_DOCX_ENTRIES: usize = 512;

const PNG_SIG: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

#[derive(Debug, Clone)]
pub enum CardImportResult {
    Card {
        card: CharacterOrScenarioCard,
        source_type: String,
    },
    SystemPrompt {
        preset: SystemPromptPreset,
        source_type: String,
    },
    Theme(ThemeConfig),
}

/// Extracts readable text from a `.docx` archive without any DOM parser (XXE-free, bounded).
pub fn extract_docx_text(bytes: &[u8]) -> Result<String, String> {
    if bytes.len() < 4 || bytes[0] != 0x50 || bytes[1] != 0x4B {
        return Err("File is not a valid .docx archive.".into());
    }
    if bytes.len() > MAX_DOCX_ARCHIVE_BYTES {
        return Err("Document archive exceeds 25 MB limit.".into());
    }
    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| "File is not a valid .docx archive.".to_string())?;
    if archive.len() > MAX_DOCX_ENTRIES {
        return Err("Archive has too many entries.".into());
    }

    let mut xml_bytes: Option<Vec<u8>> = None;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|_| "Corrupted .docx zip entry.".to_string())?;
        let raw_name = entry
            .name()
            .map_err(|_| "Archive contains an invalid entry name.".to_string())?;
        let norm = raw_name.replace('\\', "/");
        if norm.split('/').any(|seg| seg == "..") || norm.contains('\0') {
            return Err("Archive contains an unsafe entry path.".into());
        }
        if !entry.is_dir()
            && norm
                .trim_start_matches('/')
                .eq_ignore_ascii_case("word/document.xml")
        {
            let mut buf = Vec::new();
            let mut limited = (&mut entry).take((MAX_DOCX_XML_BYTES + 1) as u64);
            limited
                .read_to_end(&mut buf)
                .map_err(|e| format!("Failed reading word/document.xml: {e}"))?;
            if buf.len() > MAX_DOCX_XML_BYTES {
                return Err("word/document.xml exceeds 16 MB safety limit.".into());
            }
            xml_bytes = Some(buf);
            break;
        }
    }

    let xml_raw =
        xml_bytes.ok_or_else(|| "Document archive has no word/document.xml.".to_string())?;
    let xml = String::from_utf8_lossy(&xml_raw);
    let extracted = extract_text_from_document_xml(&xml);
    if extracted.trim().is_empty() {
        return Err("No readable text found in word/document.xml.".into());
    }
    Ok(extracted)
}

pub fn extract_text_from_document_xml(xml: &str) -> String {
    let mut out = String::with_capacity((xml.len() / 2).min(64 * 1024));
    let mut para = String::new();
    let mut in_text_run = false;
    let mut in_table_cell = false;
    let mut cell_has_content = false;
    let mut i = 0usize;
    let bytes = xml.as_bytes();

    let flush_para = |out: &mut String, para: &mut String| {
        let trimmed = para.trim_end_matches([' ', '\t']);
        if !trimmed.is_empty() {
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(trimmed);
            out.push('\n');
        } else if !out.is_empty() && !out.ends_with("\n\n") {
            out.push('\n');
        }
        para.clear();
    };

    while i < bytes.len() && out.len() + para.len() < MAX_DOCX_TEXT_CHARS {
        if bytes[i] == b'<' {
            if xml[i..].starts_with("<!--") {
                i = xml[i + 4..]
                    .find("-->")
                    .map(|r| i + 4 + r + 3)
                    .unwrap_or(bytes.len());
                continue;
            }
            if xml[i..].starts_with("<![CDATA[") {
                let end = xml[i + 9..]
                    .find("]]>")
                    .map(|r| i + 9 + r)
                    .unwrap_or(bytes.len());
                if in_text_run {
                    para.push_str(&xml[i + 9..end]);
                    if in_table_cell {
                        cell_has_content = true;
                    }
                }
                i = (end + 3).min(bytes.len());
                continue;
            }
            let Some(rel_gt) = xml[i..].find('>') else {
                break;
            };
            let raw_tag = xml[i + 1..i + rel_gt].trim();
            i += rel_gt + 1;
            let closing = raw_tag.starts_with('/');
            let self_closing = raw_tag.ends_with('/');
            let tag_name = raw_tag
                .trim_start_matches('/')
                .trim_end_matches('/')
                .split_whitespace()
                .next()
                .unwrap_or("");
            let local = tag_name.rsplit(':').next().unwrap_or(tag_name);
            match local {
                "t" => {
                    if !self_closing {
                        in_text_run = !closing;
                    }
                }
                "tab" if !closing => para.push('\t'),
                "br" | "cr" if !closing => para.push('\n'),
                "tc" => {
                    if closing {
                        if cell_has_content {
                            para.push('\t');
                        }
                        in_table_cell = false;
                        cell_has_content = false;
                    } else if !self_closing {
                        in_table_cell = true;
                        cell_has_content = false;
                    }
                }
                "p" | "tr" if closing => flush_para(&mut out, &mut para),
                _ => {}
            }
        } else {
            let next_lt = xml[i..].find('<').map(|r| i + r).unwrap_or(bytes.len());
            if in_text_run {
                let decoded = decode_xml_entities(&xml[i..next_lt]);
                para.push_str(&decoded);
                if in_table_cell && !decoded.trim().is_empty() {
                    cell_has_content = true;
                }
            }
            i = next_lt;
        }
    }
    flush_para(&mut out, &mut para);
    out.trim().to_string()
}

fn decode_xml_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

/// Imports a Character/Scenario Card, System Prompt Preset, or Theme from PNG or JSON bytes.
pub fn import_card_or_prompt_bytes(bytes: &[u8]) -> Result<CardImportResult, String> {
    if bytes.starts_with(&PNG_SIG) {
        let chunks = extract_png_text_chunks(bytes);
        if let Some(payload) = chunks.get("parchment_prompt") {
            let json_text = decode_base64_or_raw_json(payload)?;
            return Ok(CardImportResult::SystemPrompt {
                preset: parse_system_prompt_json(&json_text)?,
                source_type: "PNG (parchment_prompt chunk)".into(),
            });
        }
        let raw_chunk = chunks
            .get("ccv3")
            .or_else(|| chunks.get("chara"))
            .or_else(|| chunks.get("parchment_card"))
            .or_else(|| {
                chunks
                    .iter()
                    .find(|(k, _)| {
                        let low = k.to_ascii_lowercase();
                        low == "ccv3" || low == "chara" || low.contains("card") || low.contains("prompt")
                    })
                    .map(|(_, v)| v)
            })
            .ok_or_else(|| {
                "PNG file does not contain an embedded instruction chunk (chara, ccv3, parchment_card, parchment_prompt)."
                    .to_string()
            })?;
        let json_text = decode_base64_or_raw_json(raw_chunk)?;
        let root: Value =
            serde_json::from_str(&json_text).map_err(|e| format!("Invalid card JSON: {e}"))?;
        let obj = root
            .as_object()
            .ok_or_else(|| "Embedded chunk is not a JSON object.".to_string())?;
        if obj.contains_key("systemPrompt")
            && !obj.contains_key("firstMessage")
            && !obj.contains_key("first_mes")
            && !obj.contains_key("data")
        {
            return Ok(CardImportResult::SystemPrompt {
                preset: parse_system_prompt_json(&json_text)?,
                source_type: "PNG Embedded System Prompt".into(),
            });
        }
        return Ok(CardImportResult::Card {
            card: parse_card_json(&json_text)?,
            source_type: "PNG Embedded Metadata".into(),
        });
    }

    let text = std::str::from_utf8(bytes)
        .map_err(|_| "Import file is not valid UTF-8.".to_string())?
        .trim()
        .trim_start_matches('\u{FEFF}');
    let root: Value = serde_json::from_str(text).map_err(|e| format!("Invalid JSON: {e}"))?;
    let obj = root
        .as_object()
        .ok_or_else(|| "JSON root must be an object.".to_string())?;

    if obj.contains_key("canvasBgHex") && obj.contains_key("primaryInkHex") {
        let theme: ThemeConfig =
            serde_json::from_str(text).map_err(|e| format!("Invalid theme JSON: {e}"))?;
        return Ok(CardImportResult::Theme(theme.without_network_font_paths()));
    }
    if obj.contains_key("spec")
        || obj.contains_key("data")
        || obj.contains_key("first_mes")
        || obj.contains_key("firstMessage")
        || obj.contains_key("personality")
        || obj.contains_key("scenario")
    {
        return Ok(CardImportResult::Card {
            card: parse_card_json(text)?,
            source_type: "JSON Card".into(),
        });
    }
    Ok(CardImportResult::SystemPrompt {
        preset: parse_system_prompt_json(text)?,
        source_type: "JSON System Prompt".into(),
    })
}

fn extract_png_text_chunks(bytes: &[u8]) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut pos = 8usize;
    while pos + 12 <= bytes.len() {
        let len = u32::from_be_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]])
            as usize;
        let chunk_type = &bytes[pos + 4..pos + 8];
        let data_start = pos + 8;
        let Some(data_end) = data_start.checked_add(len) else {
            break;
        };
        if data_end + 4 > bytes.len() {
            break;
        }
        let payload = &bytes[data_start..data_end];
        if chunk_type == b"tEXt" {
            if let Some(nul) = payload.iter().position(|&b| b == 0) {
                let key = String::from_utf8_lossy(&payload[..nul]).to_string();
                let val = String::from_utf8_lossy(&payload[nul + 1..]).to_string();
                out.insert(key, val);
            }
        } else if chunk_type == b"iTXt"
            && let Some(nul) = payload.iter().position(|&b| b == 0)
        {
            let key = String::from_utf8_lossy(&payload[..nul]).to_string();
            // Skip compression flag (1) + method (1) + lang_tag\0 + translated_kw\0
            let rest = &payload[nul + 1..];
            if rest.len() >= 2 && rest[0] == 0 {
                let after_comp = &rest[2..];
                if let Some(l_nul) = after_comp.iter().position(|&b| b == 0) {
                    let after_lang = &after_comp[l_nul + 1..];
                    if let Some(t_nul) = after_lang.iter().position(|&b| b == 0) {
                        let val = String::from_utf8_lossy(&after_lang[t_nul + 1..]).to_string();
                        out.insert(key, val);
                    }
                }
            }
        }
        pos = data_end + 4;
    }
    out
}

fn decode_base64_or_raw_json(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.starts_with('{') {
        return Ok(trimmed.to_string());
    }
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(trimmed)
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(trimmed))
        .map_err(|_| "Failed decoding base64 PNG card chunk.".to_string())?;
    String::from_utf8(decoded).map_err(|_| "Decoded PNG card chunk is not UTF-8.".to_string())
}

fn parse_system_prompt_json(json_text: &str) -> Result<SystemPromptPreset, String> {
    if let Ok(preset) = serde_json::from_str::<SystemPromptPreset>(json_text) {
        return Ok(preset);
    }
    let root: Value = serde_json::from_str(json_text).map_err(|e| e.to_string())?;
    let obj = root.as_object().ok_or("Not a JSON object")?;
    let str_any = |keys: &[&str]| -> String {
        for k in keys {
            if let Some(s) = obj.get(*k).and_then(Value::as_str)
                && !s.trim().is_empty()
            {
                return s.to_string();
            }
        }
        String::new()
    };
    Ok(SystemPromptPreset {
        id: str_any(&["id"]),
        name: {
            let n = str_any(&["name", "title"]);
            if n.is_empty() {
                "Imported Prompt".into()
            } else {
                n
            }
        },
        description: str_any(&["description"]),
        system_prompt: str_any(&["systemPrompt", "system_prompt", "prompt", "content"]),
        authors_note_default: str_any(&["authorsNoteDefault", "authors_note"]),
        suggested_temperature: obj.get("suggestedTemperature").and_then(Value::as_f64),
        suggested_top_p: obj.get("suggestedTopP").and_then(Value::as_f64),
        suggested_thinking_effort: {
            let e = str_any(&["suggestedThinkingEffort"]);
            if e.is_empty() { "high".into() } else { e }
        },
    })
}

fn parse_card_json(json_text: &str) -> Result<CharacterOrScenarioCard, String> {
    let root: Value = serde_json::from_str(json_text).map_err(|e| e.to_string())?;
    let obj = root.as_object().ok_or("Card root must be a JSON object")?;
    let spec = obj
        .get("spec")
        .and_then(Value::as_str)
        .unwrap_or("parchment-v1")
        .to_string();
    let data = obj.get("data").and_then(Value::as_object).unwrap_or(obj);
    let str_any = |keys: &[&str]| -> String {
        for k in keys {
            if let Some(s) = data.get(*k).and_then(Value::as_str)
                && !s.trim().is_empty()
            {
                return s.to_string();
            }
        }
        String::new()
    };
    let mut fragments = Vec::new();
    if let Some(entries) = data
        .get("character_book")
        .and_then(|cb| cb.get("entries"))
        .and_then(Value::as_array)
    {
        for (idx, entry) in entries.iter().enumerate() {
            let Some(eobj) = entry.as_object() else {
                continue;
            };
            let content = eobj
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            if content.trim().is_empty() {
                continue;
            }
            let keys: Vec<String> = eobj
                .get("keys")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let constant = eobj
                .get("constant")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let enabled = eobj.get("enabled").and_then(Value::as_bool).unwrap_or(true);
            fragments.push(ContextFragment {
                id: format!("cb-frag-{idx}"),
                title: eobj
                    .get("name")
                    .or_else(|| eobj.get("comment"))
                    .and_then(Value::as_str)
                    .unwrap_or("World Book Entry")
                    .to_string(),
                content,
                keywords: keys,
                mode: if !enabled {
                    FragmentActivationMode::Muted
                } else if constant {
                    FragmentActivationMode::Pinned
                } else {
                    FragmentActivationMode::Keyword
                },
            });
        }
    }

    Ok(CharacterOrScenarioCard {
        name: {
            let n = str_any(&["name"]);
            if n.is_empty() {
                "Imported Card".into()
            } else {
                n
            }
        },
        description: str_any(&["description"]),
        personality: str_any(&["personality"]),
        scenario: str_any(&["scenario"]),
        first_message: str_any(&["firstMessage", "first_mes"]),
        alternate_greetings: data
            .get("alternate_greetings")
            .or_else(|| data.get("alternateGreetings"))
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        example_dialogue: str_any(&["exampleDialogue", "mes_example"]),
        system_prompt: str_any(&["systemPromptOverride", "systemPrompt", "system_prompt"]),
        post_history_instructions: str_any(&[
            "postHistoryInstructions",
            "post_history_instructions",
        ]),
        creator_notes: str_any(&["creatorNotes", "creator_notes"]),
        tags: data
            .get("tags")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        embedded_fragments: fragments,
        source_format: spec,
        source_data: String::new(),
    })
}

/// Exports a `CharacterOrScenarioCard` as a standard SillyTavern v2/v3 `.card.png` carrying
/// base64-encoded `ccv3` and `chara` `tEXt` metadata chunks (zero external image dependencies).
pub fn export_card_to_png_bytes(card: &CharacterOrScenarioCard) -> Vec<u8> {
    let entries: Vec<Value> = card
        .embedded_fragments
        .iter()
        .map(|f| {
            serde_json::json!({
                "name": f.title,
                "content": f.content,
                "keys": f.keywords,
                "constant": f.mode == FragmentActivationMode::Pinned,
                "enabled": f.mode != FragmentActivationMode::Muted,
            })
        })
        .collect();
    let payload = serde_json::json!({
        "spec": "chara_card_v3",
        "spec_version": "3.0",
        "data": {
            "name": card.name,
            "description": card.description,
            "personality": card.personality,
            "scenario": card.scenario,
            "first_mes": card.first_message,
            "alternate_greetings": card.alternate_greetings,
            "mes_example": card.example_dialogue,
            "system_prompt": card.system_prompt,
            "post_history_instructions": card.post_history_instructions,
            "creator_notes": card.creator_notes,
            "tags": card.tags,
            "character_book": { "entries": entries },
        }
    });
    let json_bytes = serde_json::to_vec(&payload).unwrap_or_default();
    let b64 = base64::engine::general_purpose::STANDARD.encode(&json_bytes);

    let mut out = Vec::with_capacity(b64.len() * 2 + 256);
    out.extend_from_slice(&PNG_SIG);

    // 1x1 RGB warm parchment pixel (#F4ECD8)
    let ihdr: [u8; 13] = [0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0];
    write_png_chunk(&mut out, b"IHDR", &ihdr);

    // Uncompressed zlib stream for 1 scanline: [filter=0, R=0xF4, G=0xEC, B=0xD8]
    let scanline: [u8; 4] = [0x00, 0xF4, 0xEC, 0xD8];
    let adler = adler32(&scanline);
    let mut idat = Vec::with_capacity(15);
    idat.extend_from_slice(&[0x78, 0x01, 0x01, 0x04, 0x00, 0xFB, 0xFF]);
    idat.extend_from_slice(&scanline);
    idat.extend_from_slice(&adler.to_be_bytes());
    write_png_chunk(&mut out, b"IDAT", &idat);

    for kw in [b"ccv3".as_slice(), b"chara".as_slice()] {
        let mut chunk = Vec::with_capacity(kw.len() + 1 + b64.len());
        chunk.extend_from_slice(kw);
        chunk.push(0);
        chunk.extend_from_slice(b64.as_bytes());
        write_png_chunk(&mut out, b"tEXt", &chunk);
    }

    write_png_chunk(&mut out, b"IEND", &[]);
    out
}

fn write_png_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32_png(kind, data);
    out.extend_from_slice(&crc.to_be_bytes());
}

fn crc32_png(kind: &[u8; 4], data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in kind.iter().chain(data.iter()) {
        crc ^= b as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    const MOD: u32 = 65521;
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for &byte in data {
        a = (a + byte as u32) % MOD;
        b = (b + a) % MOD;
    }
    (b << 16) | a
}

/// Encodes raw 8-bit RGBA pixels (`width * height * 4` bytes) into a valid PNG byte stream
/// using uncompressed zlib stored blocks (zero external image dependencies).
pub fn encode_rgba_to_png_bytes(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    if width == 0 || height == 0 || width > 16_384 || height > 16_384 {
        return Err(format!("Unsupported PNG dimensions: {width}x{height}"));
    }
    let row_bytes = (width as usize)
        .checked_mul(4)
        .ok_or_else(|| "PNG row overflow".to_string())?;
    let expected = row_bytes
        .checked_mul(height as usize)
        .ok_or_else(|| "PNG buffer overflow".to_string())?;
    if rgba.len() != expected {
        return Err(format!(
            "RGBA buffer length {} does not match {width}x{height} ({expected} bytes).",
            rgba.len()
        ));
    }

    let raw_len = (row_bytes + 1) * (height as usize);
    let mut raw = Vec::with_capacity(raw_len);
    for row in rgba.chunks_exact(row_bytes) {
        raw.push(0x00); // PNG filter type 0 (None)
        raw.extend_from_slice(row);
    }
    let adler = adler32(&raw);

    // Wrap raw scanlines in uncompressed zlib stored blocks (max 65,535 bytes per block).
    const MAX_STORED_BLOCK: usize = 65_535;
    let block_count = raw.len().div_ceil(MAX_STORED_BLOCK).max(1);
    let mut idat = Vec::with_capacity(2 + raw.len() + block_count * 5 + 4);
    idat.extend_from_slice(&[0x78, 0x01]);
    let mut pos = 0usize;
    loop {
        let rem = raw.len().saturating_sub(pos);
        let take = rem.min(MAX_STORED_BLOCK);
        let is_final = pos + take >= raw.len();
        idat.push(if is_final { 0x01 } else { 0x00 });
        let len = take as u16;
        let nlen = !len;
        idat.extend_from_slice(&len.to_le_bytes());
        idat.extend_from_slice(&nlen.to_le_bytes());
        idat.extend_from_slice(&raw[pos..pos + take]);
        pos += take;
        if is_final {
            break;
        }
    }
    idat.extend_from_slice(&adler.to_be_bytes());

    let mut out = Vec::with_capacity(8 + 25 + 12 + idat.len() + 12);
    out.extend_from_slice(&PNG_SIG);
    let mut ihdr = [0u8; 13];
    ihdr[0..4].copy_from_slice(&width.to_be_bytes());
    ihdr[4..8].copy_from_slice(&height.to_be_bytes());
    ihdr[8] = 8; // bit depth
    ihdr[9] = 6; // color type RGBA
    write_png_chunk(&mut out, b"IHDR", &ihdr);
    write_png_chunk(&mut out, b"IDAT", &idat);
    write_png_chunk(&mut out, b"IEND", &[]);
    Ok(out)
}

/// Extracts bounded `.md` skill files from a ZIP archive (max 64 files, 512 KiB each).
pub fn extract_md_skills_from_zip(zip_bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut archive = ZipArchive::new(Cursor::new(zip_bytes))
        .map_err(|e| format!("Invalid skill ZIP archive: {e}"))?;
    let mut out = Vec::new();
    for idx in 0..archive.len().min(MAX_DOCX_ENTRIES) {
        let entry = archive.by_index(idx).map_err(|e| e.to_string())?;
        let name_str = entry.name().map_err(|e| e.to_string())?.into_owned();
        if entry.is_dir() || !name_str.to_ascii_lowercase().ends_with(".md") {
            continue;
        }
        let fname = std::path::Path::new(&name_str)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("skill.md")
            .to_string();
        let mut buf = Vec::new();
        entry
            .take(512 * 1024 + 1)
            .read_to_end(&mut buf)
            .map_err(|e| e.to_string())?;
        if buf.len() <= 512 * 1024 {
            out.push((fname, buf));
            if out.len() >= 64 {
                break;
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_png_export_roundtrips_through_import() {
        let card = CharacterOrScenarioCard {
            name: "Librarian".into(),
            description: "Keeps warm parchment archives.".into(),
            first_message: "Welcome to the reading room.".into(),
            ..Default::default()
        };
        let png = export_card_to_png_bytes(&card);
        let imported = import_card_or_prompt_bytes(&png).expect("png roundtrip");
        match imported {
            CardImportResult::Card { card: c, .. } => {
                assert_eq!(c.name, "Librarian");
                assert_eq!(c.description, "Keeps warm parchment archives.");
                assert_eq!(c.first_message, "Welcome to the reading room.");
            }
            other => panic!("expected Card, got {other:?}"),
        }
    }

    #[test]
    fn encode_rgba_to_png_produces_valid_png_chunks() {
        let rgba = [
            255, 0, 0, 255, 0, 255, 0, 255, //
            0, 0, 255, 255, 255, 255, 255, 255,
        ];
        let png = encode_rgba_to_png_bytes(2, 2, &rgba).expect("encode 2x2");
        assert!(png.starts_with(&PNG_SIG));
        assert_eq!(&png[12..16], b"IHDR");
        assert_eq!(u32::from_be_bytes([png[16], png[17], png[18], png[19]]), 2);
        assert_eq!(u32::from_be_bytes([png[20], png[21], png[22], png[23]]), 2);
        assert_eq!(png[25], 6);
        assert!(png.ends_with(b"\x00\x00\x00\x00IEND\xAE\x42\x60\x82"));
    }
}
