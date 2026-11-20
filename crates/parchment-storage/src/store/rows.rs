use super::*;

pub fn validate_session_id(id: &str) -> StorageResult<()> {
    let trimmed = id.trim();
    if trimmed.is_empty()
        || trimmed == "."
        || trimmed == ".."
        || trimmed.starts_with('.')
        || trimmed.ends_with('.')
        || trimmed.contains(['/', '\\', '\0'])
        || is_windows_device_name(trimmed)
    {
        return Err(StorageError::InvalidId(id.to_string()));
    }
    Ok(())
}

pub(crate) fn is_windows_device_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name);
    if stem.eq_ignore_ascii_case("CON")
        || stem.eq_ignore_ascii_case("PRN")
        || stem.eq_ignore_ascii_case("AUX")
        || stem.eq_ignore_ascii_case("NUL")
    {
        return true;
    }
    let b = stem.as_bytes();
    b.len() == 4
        && (b[..3].eq_ignore_ascii_case(b"COM") || b[..3].eq_ignore_ascii_case(b"LPT"))
        && b[3].is_ascii_digit()
}

/// Quotes every whitespace-separated token as an FTS5 phrase literal so punctuation like `C++`
/// or `foo-bar` never causes an FTS5 syntax error.
pub fn escape_fts5_query(raw: &str) -> Option<String> {
    let mut out = String::with_capacity(raw.len() + 8);
    for token in raw.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push('"');
        for ch in token.chars() {
            if ch == '"' {
                out.push('"');
            }
            out.push(ch);
        }
        out.push('"');
    }
    (!out.is_empty()).then_some(out)
}

pub(super) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub(super) fn map_pinned_stub(row: &rusqlite::Row<'_>) -> rusqlite::Result<PinnedMessageStub> {
    let mut preview: String = row.get(4)?;
    if let Some((cut, _)) = preview.char_indices().nth(180) {
        preview.truncate(cut);
    }
    Ok(PinnedMessageStub {
        chat_id: row.get(0)?,
        chat_title: row.get(1)?,
        message_id: row.get(2)?,
        role: MessageRole::from_db_str(&row.get::<_, String>(3)?),
        preview,
        created_at: row.get(5)?,
    })
}

pub(super) fn upsert_chat_row(tx: &Transaction<'_>, session: &ChatSession) -> StorageResult<()> {
    let pinned_count = session.messages.iter().filter(|m| m.pinned).count() as i64;
    upsert_chat_row_with_counts(tx, session, session.messages.len() as i64, pinned_count)
}

pub(super) fn upsert_chat_row_with_counts(
    tx: &Transaction<'_>,
    session: &ChatSession,
    message_count: i64,
    pinned_message_count: i64,
) -> StorageResult<()> {
    let meta = ChatMetaRef {
        user_name: &session.user_name,
        system_prompt: &session.system_prompt,
        active_card: &session.active_card,
        story_summary: &session.story_summary,
        fragments: &session.fragments,
        authors_note: &session.authors_note,
        authors_note_depth: session.authors_note_depth,
        permission_overrides: &session.permission_overrides,
        utility_usage_records: &session.utility_usage_records,
    };
    let meta_json = serde_json::to_string(&meta)?;

    tx.execute(
        "INSERT INTO chats (
             id, title, created_at_ms, updated_at_ms, profile_id, model_override, assistant_id,
             is_pinned, is_title_manually_set, draft_input, draft_prefill, view_mode,
             workspace_path, workspace_mode, message_count, pinned_message_count, meta_json
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
         ON CONFLICT(id) DO UPDATE SET
             title = excluded.title,
             created_at_ms = excluded.created_at_ms,
             updated_at_ms = excluded.updated_at_ms,
             profile_id = excluded.profile_id,
             model_override = excluded.model_override,
             assistant_id = excluded.assistant_id,
             is_pinned = excluded.is_pinned,
             is_title_manually_set = excluded.is_title_manually_set,
             draft_input = excluded.draft_input,
             draft_prefill = excluded.draft_prefill,
             view_mode = excluded.view_mode,
             workspace_path = excluded.workspace_path,
             workspace_mode = excluded.workspace_mode,
             message_count = excluded.message_count,
             pinned_message_count = excluded.pinned_message_count,
             meta_json = excluded.meta_json",
        params![
            &session.id,
            &session.title,
            session.created_at,
            session.updated_at,
            &session.profile_id,
            &session.model_override,
            &session.assistant_id,
            i64::from(session.is_pinned),
            i64::from(session.is_title_manually_set),
            &session.draft_input,
            &session.draft_prefill,
            session.view_mode.as_db_str(),
            &session.workspace_path,
            session.workspace_mode.as_db_str(),
            message_count,
            pinned_message_count,
            meta_json,
        ],
    )?;
    Ok(())
}

pub(super) fn insert_message_node(
    tx: &Transaction<'_>,
    chat_id: &str,
    seq: i64,
    node: &MessageNode,
) -> StorageResult<()> {
    let created_at = node.active_variant().map(|v| v.created_at).unwrap_or(0);
    tx.execute(
        "INSERT INTO messages (
             id, chat_id, seq, role, active_variant_index, included_in_context,
             pinned, created_at_ms, active_content
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            &node.id,
            chat_id,
            seq,
            node.role.as_db_str(),
            node.active_variant_index as i64,
            i64::from(node.included_in_context),
            i64::from(node.pinned),
            created_at,
            node.active_content(),
        ],
    )?;

    for (v_idx, variant) in node.variants.iter().enumerate() {
        upsert_variant_and_tools(tx, chat_id, &node.id, v_idx as i64, variant)?;
    }
    Ok(())
}

pub(super) fn upsert_variant_and_tools(
    tx: &Transaction<'_>,
    chat_id: &str,
    message_id: &str,
    variant_index: i64,
    variant: &MessageVariant,
) -> StorageResult<()> {
    let proto_str = variant
        .provider_protocol
        .map(ProviderProtocol::as_db_str)
        .unwrap_or("");
    let meta = VariantMetaRef {
        goal_tracker: &variant.goal_tracker,
        media_items: &variant.media_items,
        attachments: &variant.attachments,
        web_sources: &variant.web_sources,
        token_metrics: &variant.token_metrics,
        api_usage_records: &variant.api_usage_records,
        legacy_usage_owner_session_id: &variant.legacy_usage_owner_session_id,
        raw_request_preview: &variant.raw_request_preview,
        raw_response_preview: &variant.raw_response_preview,
    };
    let meta_json = serde_json::to_string(&meta)?;

    tx.execute(
        "INSERT INTO variants (
             id, message_id, chat_id, variant_index, content, thinking_summary, trailing_thinking,
             thinking_signature, thinking_duration_ms, provider_interaction_id,
             pending_background_interaction_id, is_streaming, provider_protocol,
             provider_wire_dialect, model_used, created_at_ms, meta_json
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
         ON CONFLICT(chat_id, message_id, id) DO UPDATE SET
             variant_index = excluded.variant_index,
             content = excluded.content,
             thinking_summary = excluded.thinking_summary,
             trailing_thinking = excluded.trailing_thinking,
             thinking_signature = excluded.thinking_signature,
             thinking_duration_ms = excluded.thinking_duration_ms,
             provider_interaction_id = excluded.provider_interaction_id,
             pending_background_interaction_id = excluded.pending_background_interaction_id,
             is_streaming = excluded.is_streaming,
             provider_protocol = excluded.provider_protocol,
             provider_wire_dialect = excluded.provider_wire_dialect,
             model_used = excluded.model_used,
             created_at_ms = excluded.created_at_ms,
             meta_json = excluded.meta_json",
        params![
            &variant.id,
            message_id,
            chat_id,
            variant_index,
            &variant.content,
            &variant.thinking_summary,
            &variant.trailing_thinking,
            &variant.thinking_signature,
            variant.thinking_duration_ms,
            &variant.provider_interaction_id,
            &variant.pending_background_interaction_id,
            i64::from(variant.is_streaming),
            proto_str,
            &variant.provider_wire_dialect,
            &variant.model_used,
            variant.created_at,
            meta_json,
        ],
    )?;

    tx.execute(
        "DELETE FROM tool_calls WHERE chat_id = ?1 AND message_id = ?2 AND variant_id = ?3",
        params![chat_id, message_id, &variant.id],
    )?;
    for (seq, tc) in variant.tool_calls.iter().enumerate() {
        let (inline_text, overflow_blob): (&str, Option<&[u8]>) =
            if tc.result_text.len() > INLINE_TOOL_RESULT_LIMIT_BYTES {
                let mut cut = INLINE_TOOL_RESULT_LIMIT_BYTES;
                while cut > 0 && !tc.result_text.is_char_boundary(cut) {
                    cut -= 1;
                }
                (&tc.result_text[..cut], Some(tc.result_text.as_bytes()))
            } else {
                (tc.result_text.as_str(), None)
            };
        tx.execute(
            "INSERT INTO tool_calls (
                 id, variant_id, message_id, chat_id, seq, name, arguments_json,
                 result_text, result_overflow, is_error, is_running, duration_ms,
                 thought_signature, preceding_thinking, preceding_text,
                 message_phases, starts_tool_step
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
            params![
                &tc.id,
                &variant.id,
                message_id,
                chat_id,
                seq as i64,
                &tc.name,
                &tc.arguments_json,
                inline_text,
                overflow_blob,
                i64::from(tc.is_error),
                i64::from(tc.is_running),
                tc.duration_ms,
                &tc.thought_signature,
                &tc.preceding_thinking,
                &tc.preceding_text,
                i64::from(tc.message_phases),
                i64::from(tc.starts_tool_step),
            ],
        )?;
    }

    for rec in &variant.api_usage_records {
        insert_usage_record(
            tx,
            chat_id,
            message_id,
            &variant.id,
            rec,
            variant.created_at,
        )?;
    }
    Ok(())
}

pub(super) fn insert_usage_record(
    tx: &Transaction<'_>,
    chat_id: &str,
    message_id: &str,
    variant_id: &str,
    rec: &ApiUsageRecord,
    created_at_ms: i64,
) -> StorageResult<()> {
    let record_json = serde_json::to_string(rec)?;
    tx.execute(
        "INSERT INTO usage_records (
             request_id, chat_id, message_id, variant_id, owner_session_id, owner_run_id,
             purpose, profile_id, model, wire_dialect, input_tokens, output_tokens,
             cached_input_tokens, reasoning_tokens, reported_total_usd, record_json, created_at_ms
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
         ON CONFLICT(chat_id, request_id) DO UPDATE SET
             owner_session_id = excluded.owner_session_id,
             input_tokens = excluded.input_tokens,
             output_tokens = excluded.output_tokens,
             cached_input_tokens = excluded.cached_input_tokens,
             reasoning_tokens = excluded.reasoning_tokens,
             reported_total_usd = excluded.reported_total_usd,
             record_json = excluded.record_json",
        params![
            &rec.request_id,
            chat_id,
            message_id,
            variant_id,
            &rec.owner_session_id,
            &rec.owner_run_id,
            &rec.purpose,
            &rec.profile_id,
            &rec.model,
            &rec.wire_dialect,
            rec.usage.input_tokens,
            rec.usage.output_tokens,
            rec.usage.cached_input_tokens,
            rec.usage.reasoning_tokens,
            rec.usage.charges.reported_total_usd,
            record_json,
            created_at_ms,
        ],
    )?;
    Ok(())
}

pub(super) type MessageHeader = (String, i64, MessageRole, usize, bool, bool);

pub(super) fn hydrate_message_headers(
    conn: &Connection,
    chat_id: &str,
    headers: Vec<MessageHeader>,
    hydrate_large_tools: bool,
) -> StorageResult<Vec<MessageNode>> {
    if headers.is_empty() {
        return Ok(Vec::new());
    }

    // Headers come from a contiguous `seq` slice: bound both scans by `[min_seq, max_seq]` so
    // loading the latest 50 turns of a 10,000-turn chat stays strictly O(limit) in time and RAM.
    let min_seq = headers.first().map(|h| h.1).unwrap_or(0);
    let max_seq = headers.last().map(|h| h.1).unwrap_or(min_seq);
    let mut variants_by_msg: HashMap<String, Vec<MessageVariant>> =
        HashMap::with_capacity(headers.len());

    let mut tool_stmt = conn.prepare_cached(if hydrate_large_tools {
        "SELECT tc.message_id, tc.variant_id, tc.id, tc.name, tc.arguments_json, tc.result_text,
                tc.result_overflow, tc.is_error, tc.is_running, tc.duration_ms,
                tc.thought_signature, tc.preceding_thinking, tc.preceding_text,
                tc.message_phases, tc.starts_tool_step
         FROM messages m
         JOIN tool_calls tc ON tc.chat_id = m.chat_id AND tc.message_id = m.id
         WHERE m.chat_id = ?1 AND m.seq BETWEEN ?2 AND ?3
         ORDER BY m.seq ASC, tc.variant_id, tc.seq ASC"
    } else {
        "SELECT tc.message_id, tc.variant_id, tc.id, tc.name, tc.arguments_json, tc.result_text,
                NULL, tc.is_error, tc.is_running, tc.duration_ms,
                tc.thought_signature, tc.preceding_thinking, tc.preceding_text,
                tc.message_phases, tc.starts_tool_step
         FROM messages m
         JOIN tool_calls tc ON tc.chat_id = m.chat_id AND tc.message_id = m.id
         WHERE m.chat_id = ?1 AND m.seq BETWEEN ?2 AND ?3
         ORDER BY m.seq ASC, tc.variant_id, tc.seq ASC"
    })?;
    let mut tools_by_msg: HashMap<String, HashMap<String, Vec<ToolCallRecord>>> = HashMap::new();
    let tool_rows = tool_stmt.query_map(params![chat_id, min_seq, max_seq], |r| {
        let msg_id: String = r.get(0)?;
        let var_id: String = r.get(1)?;
        let inline_res: String = r.get(5)?;
        let overflow: Option<Vec<u8>> = r.get(6)?;
        let result_text = match overflow {
            Some(bytes) => String::from_utf8_lossy_owned(bytes),
            None => inline_res,
        };
        Ok((
            msg_id,
            var_id,
            ToolCallRecord {
                id: r.get(2)?,
                name: r.get(3)?,
                arguments_json: r.get(4)?,
                result_text,
                is_error: r.get::<_, i64>(7)? != 0,
                is_running: r.get::<_, i64>(8)? != 0,
                duration_ms: r.get(9)?,
                thought_signature: r.get(10)?,
                preceding_thinking: r.get(11)?,
                preceding_text: r.get(12)?,
                screenshot_jpeg_base64: String::new(),
                message_phases: r.get::<_, i64>(13)? != 0,
                starts_tool_step: r.get::<_, i64>(14)? != 0,
            },
        ))
    })?;
    for row in tool_rows {
        let (mid, vid, rec) = row?;
        tools_by_msg
            .entry(mid)
            .or_default()
            .entry(vid)
            .or_default()
            .push(rec);
    }

    let mut var_stmt = conn.prepare_cached(
        "SELECT v.message_id, v.id, v.content, v.thinking_summary, v.trailing_thinking,
                v.thinking_signature, v.thinking_duration_ms, v.provider_interaction_id,
                v.pending_background_interaction_id, v.is_streaming, v.provider_protocol,
                v.provider_wire_dialect, v.model_used, v.created_at_ms, v.meta_json
         FROM messages m
         JOIN variants v ON v.chat_id = m.chat_id AND v.message_id = m.id
         WHERE m.chat_id = ?1 AND m.seq BETWEEN ?2 AND ?3
         ORDER BY m.seq ASC, v.variant_index ASC",
    )?;
    let var_rows = var_stmt.query_map(params![chat_id, min_seq, max_seq], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, i64>(6)?,
            r.get::<_, String>(7)?,
            r.get::<_, String>(8)?,
            r.get::<_, i64>(9)? != 0,
            r.get::<_, String>(10)?,
            r.get::<_, String>(11)?,
            r.get::<_, String>(12)?,
            r.get::<_, i64>(13)?,
            r.get::<_, String>(14)?,
        ))
    })?;
    for row in var_rows {
        let (
            msg_id,
            id,
            content,
            thinking_summary,
            trailing_thinking,
            thinking_signature,
            thinking_duration_ms,
            provider_interaction_id,
            pending_background_interaction_id,
            is_streaming,
            proto_raw,
            provider_wire_dialect,
            model_used,
            created_at,
            meta_json,
        ) = row?;
        let meta: VariantMetaJson = if meta_json.is_empty() || meta_json == "{}" {
            VariantMetaJson::default()
        } else {
            serde_json::from_str(&meta_json).unwrap_or_default()
        };
        let provider_protocol = ProviderProtocol::from_db_str(&proto_raw);
        let tool_calls = tools_by_msg
            .get_mut(&msg_id)
            .and_then(|by_var| by_var.remove(&id))
            .unwrap_or_default();
        variants_by_msg
            .entry(msg_id)
            .or_default()
            .push(MessageVariant {
                id,
                content,
                thinking_summary,
                trailing_thinking,
                thinking_signature,
                thinking_duration_ms,
                tool_calls,
                goal_tracker: meta.goal_tracker,
                media_items: meta.media_items,
                attachments: meta.attachments,
                web_sources: meta.web_sources,
                provider_interaction_id,
                pending_background_interaction_id,
                is_streaming,
                provider_protocol,
                provider_wire_dialect,
                model_used,
                created_at,
                token_metrics: meta.token_metrics,
                api_usage_records: meta.api_usage_records,
                legacy_usage_owner_session_id: meta.legacy_usage_owner_session_id,
                raw_request_preview: meta.raw_request_preview,
                raw_response_preview: meta.raw_response_preview,
            });
    }

    Ok(headers
        .into_iter()
        .map(
            |(id, _seq, role, active_variant_index, included_in_context, pinned)| {
                let variants = variants_by_msg.remove(&id).unwrap_or_default();
                MessageNode {
                    id,
                    role,
                    variants,
                    active_variant_index,
                    included_in_context,
                    pinned,
                }
            },
        )
        .collect())
}
