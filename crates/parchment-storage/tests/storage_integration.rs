#![allow(clippy::unwrap_used, clippy::expect_used, clippy::print_stdout)]

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use parchment_model::{
    AgentPermissionResolver, ApiUsageRecord, ChatSession, GeneratedMediaItem, MediaKind,
    MessageNode, MessageRole, MessageVariant, PromptAttachment, ProviderChargeUsage,
    ProviderProtocol, ProviderTokenUsage, ToolCallRecord, WorkspaceMode,
};
use parchment_storage::{
    ChatArchiveLimits, ParchmentStore, export_chats_to_zip, import_chat_file_bytes,
    migrate_v2_directory,
};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

fn temp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("parchment_p1_{label}_{nanos}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}

struct DirGuard(PathBuf);
impl Drop for DirGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn sample_chat(id: &str, title: &str) -> ChatSession {
    ChatSession {
        id: id.into(),
        title: title.into(),
        created_at: 1_700_000_000_000,
        updated_at: 1_700_000_001_000,
        profile_id: "prof-gemini".into(),
        model_override: String::new(),
        assistant_id: "asst-default".into(),
        is_pinned: false,
        is_title_manually_set: false,
        draft_input: String::new(),
        draft_prefill: String::new(),
        user_name: "User".into(),
        system_prompt: String::new(),
        active_card: None,
        story_summary: String::new(),
        fragments: vec![],
        authors_note: String::new(),
        authors_note_depth: 2,
        view_mode: Default::default(),
        workspace_path: "C:\\Users\\dev\\repo".into(),
        workspace_mode: WorkspaceMode::ReadWrite,
        permission_overrides: None,
        utility_usage_records: vec![],
        messages: vec![MessageNode {
            id: "m1".into(),
            role: MessageRole::Assistant,
            variants: vec![MessageVariant {
                id: "v1".into(),
                content: "Hello C++ & Rust foo-bar \"quoted\" world".into(),
                thinking_signature: "opaque_sig_==+/123".into(),
                tool_calls: vec![ToolCallRecord {
                    id: "tc1".into(),
                    name: "fs_read".into(),
                    arguments_json: "{\"path\":\"lib.rs\"}".into(),
                    result_text: "ok".into(),
                    thought_signature: "tool_sig_verbatim_999".into(),
                    screenshot_jpeg_base64: "MUST_NEVER_PERSIST".into(),
                    starts_tool_step: true,
                    ..Default::default()
                }],
                provider_protocol: Some(ProviderProtocol::GeminiInteractions),
                provider_wire_dialect: "gemini_interactions".into(),
                model_used: "gemini-3-pro".into(),
                created_at: 1_700_000_001_000,
                ..Default::default()
            }],
            active_variant_index: 0,
            included_in_context: true,
            pinned: true,
        }],
    }
}

#[test]
fn normalized_store_handles_300_chats_streaming_checkpoint_fts5_and_large_tool_spill() {
    let dir = temp_dir("store300");
    let _guard = DirGuard(dir.clone());
    let mut store = ParchmentStore::open(&dir.join("parchment.db")).unwrap();
    store.seed_benchmark_dataset(300, 40, 200).unwrap();

    let t_stubs = Instant::now();
    let stubs = store.list_chat_stubs().unwrap();
    let stubs_ms = t_stubs.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(stubs.len(), 300);

    let t_tail = Instant::now();
    let latest = store.load_latest_messages("perf-000", 50, false).unwrap();
    let tail_ms = t_tail.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(latest.len(), 50);
    assert_eq!(latest.last().unwrap().id, "perf-000-m199");

    // Append a live turn with a 150 KiB tool output and checkpoint it via upsert_streaming_variant.
    let huge_output = "0123456789abcdef".repeat(10_000); // 160 KB > 64 KiB spill threshold
    let live_node = MessageNode {
        id: "perf-000-live".into(),
        role: MessageRole::Assistant,
        variants: vec![MessageVariant {
            id: "v-live".into(),
            content: "Initial".into(),
            ..Default::default()
        }],
        active_variant_index: 0,
        included_in_context: true,
        pinned: false,
    };
    store
        .append_message_node("perf-000", &live_node, 1_800_000_000_000)
        .unwrap();

    let updated_variant = MessageVariant {
        id: "v-live".into(),
        content: "Updated C++ & Rust foo-bar UniqueWalStreamToken!".into(),
        thinking_signature: "opaque_stream_sig_42".into(),
        tool_calls: vec![ToolCallRecord {
            id: "tc-huge".into(),
            name: "shell_exec".into(),
            arguments_json: "{\"cmd\":\"echo\"}".into(),
            result_text: huge_output.clone(),
            thought_signature: "tc_opaque_sig".into(),
            screenshot_jpeg_base64: "TRANSIENT_SCREENSHOT".into(),
            starts_tool_step: true,
            ..Default::default()
        }],
        created_at: 1_800_000_000_100,
        ..Default::default()
    };

    let t_chk = Instant::now();
    store
        .upsert_streaming_variant(
            "perf-000",
            "perf-000-live",
            0,
            &updated_variant,
            1_800_000_000_100,
        )
        .unwrap();
    let chk_ms = t_chk.elapsed().as_secs_f64() * 1000.0;

    // Viewport load (hydrate_large_tools=false) keeps inline preview <= 64 KiB and drops screenshot.
    let tail_compact = store.load_latest_messages("perf-000", 1, false).unwrap();
    let compact_var = &tail_compact[0].variants[0];
    assert_eq!(compact_var.thinking_signature, "opaque_stream_sig_42");
    assert_eq!(compact_var.tool_calls[0].thought_signature, "tc_opaque_sig");
    assert!(compact_var.tool_calls[0].screenshot_jpeg_base64.is_empty());
    assert!(compact_var.tool_calls[0].result_text.len() <= 64 * 1024);

    // On-demand drawer load & full session hydration return the exact 160 KB output.
    let full_tool = store
        .load_tool_call_full_result("perf-000", "perf-000-live", "v-live", 0)
        .unwrap()
        .unwrap();
    assert_eq!(full_tool, huge_output);

    let full_session = store.load_chat_session("perf-000").unwrap().unwrap();
    assert_eq!(
        full_session.messages.last().unwrap().variants[0].tool_calls[0].result_text,
        huge_output
    );

    // Safe FTS5 search handles punctuation, operators, and unmatched quotes without error.
    let t_fts = Instant::now();
    let hits = store
        .search_messages("C++ foo-bar \"UniqueWalStreamToken", 10)
        .unwrap();
    let fts_ms = t_fts.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].message_id, "perf-000-live");

    // Pinning updates sidebar pinned index cleanly.
    store
        .set_message_pinned("perf-000", "perf-000-live", true)
        .unwrap();
    let pinned = store.list_pinned_messages(None, 10).unwrap();
    assert_eq!(pinned.len(), 1);
    assert_eq!(pinned[0].message_id, "perf-000-live");

    eprintln!(
        "PHASE1_STORAGE stubs_300_ms={stubs_ms:.2} tail_50_ms={tail_ms:.2} stream_chk_ms={chk_ms:.2} fts5_ms={fts_ms:.2}"
    );
    assert!(stubs_ms < 50.0);
    assert!(tail_ms < 50.0);
    assert!(chk_ms < 50.0);
    assert!(fts_ms < 50.0);
}

#[test]
fn forked_usage_records_are_counted_once_per_owned_request() {
    let mut store = ParchmentStore::open_in_memory().unwrap();
    let usage = ApiUsageRecord {
        request_id: "req-shared-1".into(),
        response_id: "resp-1".into(),
        wire_dialect: "openai_responses".into(),
        profile_id: "p1".into(),
        model: "gpt-5".into(),
        pricing: Default::default(),
        usage: ProviderTokenUsage {
            input_tokens: Some(1000),
            output_tokens: Some(500),
            charges: ProviderChargeUsage {
                reported_total_usd: Some(0.125),
                ..Default::default()
            },
            ..Default::default()
        },
        owner_session_id: "chat-orig".into(),
        owner_run_id: "run-1".into(),
        purpose: "chat".into(),
        context_pricing_tiers: vec![],
        charge_pricing: Default::default(),
    };

    let mut orig = sample_chat("chat-orig", "Original");
    orig.messages[0].variants[0]
        .api_usage_records
        .push(usage.clone());
    let mut fork = sample_chat("chat-fork", "Fork");
    fork.messages[0].variants[0].api_usage_records.push(usage);

    store.upsert_chat_session(&orig).unwrap();
    store.upsert_chat_session(&fork).unwrap();

    let total = store.sum_owned_reported_usd(None).unwrap();
    let orig_only = store.sum_owned_reported_usd(Some("chat-orig")).unwrap();
    let fork_only = store.sum_owned_reported_usd(Some("chat-fork")).unwrap();
    assert!((total - 0.125).abs() < 1e-9);
    assert!((orig_only - 0.125).abs() < 1e-9);
    assert!((fork_only - 0.0).abs() < 1e-9);
}

#[test]
fn v2_migrator_folds_matching_live_json_ignores_stale_and_is_nondestructive() {
    let dir = temp_dir("v2mig");
    let _guard = DirGuard(dir.clone());

    // Create v2 settings.json with legacy BYTEDANCE_ARK protocol.
    let settings_json = r#"{
        "profiles": [{ "id": "p1", "name": "Ark", "protocol": "BYTEDANCE_ARK", "storeServerSide": false }],
        "activeProfileId": "p1",
        "otaGithubRepo": "baros4294/parchment"
    }"#;
    fs::write(dir.join("settings.json"), settings_json).unwrap();

    // Create chat-a with matching live.json sidecar + real non-empty media + empty media.
    let chat_a_dir = dir.join("chats").join("chat-a");
    fs::create_dir_all(chat_a_dir.join("media")).unwrap();
    fs::write(chat_a_dir.join("media").join("real.png"), b"PNGDATA").unwrap();
    fs::write(chat_a_dir.join("media").join("empty.png"), b"").unwrap();

    let mut chat_a = sample_chat("chat-a", "Live Folded Chat");
    chat_a.messages[0].variants[0].attachments = vec![
        PromptAttachment {
            id: "a1".into(),
            file_name: "real.png".into(),
            mime_type: "image/png".into(),
            base64_data: "SHOULD_STRIP".into(),
            text_content: String::new(),
            local_file_path: "chats/chat-a/media/real.png".into(),
        },
        PromptAttachment {
            id: "a2".into(),
            file_name: "empty.png".into(),
            mime_type: "image/png".into(),
            base64_data: "MUST_KEEP_WHEN_EMPTY".into(),
            text_content: String::new(),
            local_file_path: "chats/chat-a/media/empty.png".into(),
        },
    ];
    let chat_a_file = chat_a_dir.join("chat.json");
    let chat_a_bytes = serde_json::to_vec(&chat_a).unwrap();
    fs::write(&chat_a_file, &chat_a_bytes).unwrap();
    let meta = fs::metadata(&chat_a_file).unwrap();
    let mtime_ms = meta
        .modified()
        .unwrap()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;

    let mut folded_var = chat_a.messages[0].variants[0].clone();
    folded_var.content = "CRASH_RECOVERED_STREAM_TEXT".into();
    let live_json = serde_json::json!({
        "chatLength": meta.len(),
        "chatModifiedMs": mtime_ms,
        "turn": { "nodeId": "m1", "variant": folded_var }
    });
    fs::write(chat_a_dir.join("live.json"), live_json.to_string()).unwrap();

    // Create chat-stale with mismatched chatLength in live.json.
    let chat_stale_dir = dir.join("chats").join("chat-stale");
    fs::create_dir_all(&chat_stale_dir).unwrap();
    let chat_stale = sample_chat("chat-stale", "Stale Sidecar");
    fs::write(
        chat_stale_dir.join("chat.json"),
        serde_json::to_vec(&chat_stale).unwrap(),
    )
    .unwrap();
    let stale_live = serde_json::json!({
        "chatLength": 999999,
        "chatModifiedMs": 12345,
        "turn": { "nodeId": "m1", "variant": { "id": "v1", "content": "STALE_MUST_NOT_APPLY" } }
    });
    fs::write(chat_stale_dir.join("live.json"), stale_live.to_string()).unwrap();

    let mut store = ParchmentStore::open(&dir.join("parchment.db")).unwrap();
    let report = migrate_v2_directory(&mut store, &dir, false).unwrap();
    assert!(!report.already_migrated);
    assert!(report.settings_migrated);
    assert_eq!(report.chats_migrated, 2);
    assert_eq!(report.live_sidecars_folded, 1);

    // Check settings migration.
    let settings = store.load_settings().unwrap();
    assert_eq!(
        settings.profiles[0].protocol,
        ProviderProtocol::OpenAiResponses
    );

    // Check folded live.json + selective base64 stripping.
    let loaded_a = store.load_chat_session("chat-a").unwrap().unwrap();
    assert_eq!(
        loaded_a.messages[0].variants[0].content,
        "CRASH_RECOVERED_STREAM_TEXT"
    );
    assert!(
        loaded_a.messages[0].variants[0].attachments[0]
            .base64_data
            .is_empty()
    );
    assert_eq!(
        loaded_a.messages[0].variants[0].attachments[1].base64_data,
        "MUST_KEEP_WHEN_EMPTY"
    );

    // Check stale sidecar was ignored.
    let loaded_stale = store.load_chat_session("chat-stale").unwrap().unwrap();
    assert_ne!(
        loaded_stale.messages[0].variants[0].content,
        "STALE_MUST_NOT_APPLY"
    );

    // Non-destructive: v2 files remain untouched, and second call is idempotent.
    assert!(chat_a_dir.join("live.json").is_file());
    assert!(dir.join("settings.json").is_file());
    let second = migrate_v2_directory(&mut store, &dir, false).unwrap();
    assert!(second.already_migrated);
}

#[test]
fn archive_roundtrip_and_hostile_zip_rejections() {
    let src_root = temp_dir("arch_src");
    let dst_root = temp_dir("arch_dst");
    let _g1 = DirGuard(src_root.clone());
    let _g2 = DirGuard(dst_root.clone());

    let media_dir = src_root.join("chats").join("c1").join("media");
    fs::create_dir_all(&media_dir).unwrap();
    fs::write(media_dir.join("art.png"), b"REAL_PNG_BYTES").unwrap();

    let mut chat = sample_chat("c1", "Exportable");
    chat.messages[0].variants[0]
        .media_items
        .push(GeneratedMediaItem {
            id: "g1".into(),
            kind: MediaKind::Image,
            mime_type: "image/png".into(),
            local_file_path: "chats/c1/media/art.png".into(),
            remote_url: "https://example.com/art.png".into(),
            revised_prompt: String::new(),
            aspect_ratio: "1:1".into(),
        });

    let zip_path = src_root.join("bundle.parchment");
    export_chats_to_zip(&zip_path, &[chat], &src_root).unwrap();
    let zip_bytes = fs::read(&zip_path).unwrap();

    let imported = import_chat_file_bytes(
        "bundle.parchment",
        &zip_bytes,
        &dst_root,
        ChatArchiveLimits::default(),
    )
    .unwrap();
    assert_eq!(imported.len(), 1);
    assert_eq!(
        fs::read(
            dst_root
                .join("chats")
                .join("c1")
                .join("media")
                .join("art.png")
        )
        .unwrap(),
        b"REAL_PNG_BYTES"
    );
    // Untrusted import must strip workspace/tools/shell permissions.
    let policy = AgentPermissionResolver::resolve(&imported[0], None, None);
    assert!(!policy.tools_enabled);
    assert!(!policy.has_workspace());
    assert!(!policy.allow_shell());

    // Hostile 1: Conflicting existing media file rejects entire archive without clobbering.
    fs::write(
        dst_root
            .join("chats")
            .join("c1")
            .join("media")
            .join("art.png"),
        b"DIFFERENT_EXISTING",
    )
    .unwrap();
    let err = import_chat_file_bytes(
        "bundle.parchment",
        &zip_bytes,
        &dst_root,
        ChatArchiveLimits::default(),
    )
    .unwrap_err();
    assert!(err.to_string().contains("existing media file"));
    assert_eq!(
        fs::read(
            dst_root
                .join("chats")
                .join("c1")
                .join("media")
                .join("art.png")
        )
        .unwrap(),
        b"DIFFERENT_EXISTING"
    );

    // Helper to build custom hostile zips in memory.
    let make_zip = |entries: &[(&str, &[u8])]| -> Vec<u8> {
        let mut cursor = std::io::Cursor::new(Vec::new());
        {
            let mut zw = ZipWriter::new(&mut cursor);
            let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
            for (name, data) in entries {
                zw.start_file(*name, opts).unwrap();
                zw.write_all(data).unwrap();
            }
            zw.finish().unwrap();
        }
        cursor.into_inner()
    };
    let valid_chat_bytes = serde_json::to_vec(&sample_chat("c2", "Safe")).unwrap();

    // Hostile 2: Zip-slip `..` traversal
    let z_slip = make_zip(&[
        ("chats/c2/chat.json", &valid_chat_bytes),
        ("../escape.txt", b"x"),
    ]);
    assert!(
        import_chat_file_bytes("bad.zip", &z_slip, &dst_root, ChatArchiveLimits::default())
            .is_err()
    );

    // Hostile 3: Windows reserved device name in media
    let z_dev = make_zip(&[
        ("chats/c2/chat.json", &valid_chat_bytes),
        ("chats/c2/media/CON.png", b"x"),
    ]);
    assert!(
        import_chat_file_bytes("dev.zip", &z_dev, &dst_root, ChatArchiveLimits::default()).is_err()
    );

    // Hostile 4: Case-insensitive media collision
    let z_col = make_zip(&[
        ("chats/c2/chat.json", &valid_chat_bytes),
        ("chats/c2/media/Pic.png", b"1"),
        ("chats/c2/media/pic.png", b"2"),
    ]);
    assert!(
        import_chat_file_bytes("col.zip", &z_col, &dst_root, ChatArchiveLimits::default()).is_err()
    );

    // Hostile 5: Truncated / corrupt ZIP
    let truncated = &zip_bytes[..zip_bytes.len() / 2];
    assert!(
        import_chat_file_bytes(
            "trunc.zip",
            truncated,
            &dst_root,
            ChatArchiveLimits::default()
        )
        .is_err()
    );

    // Hostile 6: Bounded decompression limit (anti-zip-bomb)
    let tiny_limits = ChatArchiveLimits {
        max_total_bytes: 64,
        ..ChatArchiveLimits::default()
    };
    assert!(import_chat_file_bytes("bomb.zip", &zip_bytes, &dst_root, tiny_limits).is_err());
}

#[test]
fn wal_journal_size_limit_and_reclaim_storage_keep_disk_bounded() {
    let dir = temp_dir("wal-reclaim");
    let db_path = dir.join("parchment.db");
    let mut store = ParchmentStore::open(&db_path).unwrap();
    store.seed_benchmark_dataset(10, 4, 10).unwrap();
    store.reclaim_storage().unwrap();
    let wal_path = dir.join("parchment.db-wal");
    let wal_len = fs::metadata(&wal_path).map_or(0, |m| m.len());
    assert!(
        wal_len <= 8 * 1024 * 1024,
        "WAL file should stay bounded after reclaim_storage (got {wal_len} B)"
    );
    let _ = fs::remove_dir_all(&dir);
}
