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
