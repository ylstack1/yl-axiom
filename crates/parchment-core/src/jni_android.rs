//! Direct zero-JNA JNI bridge for Android (`io.parchment.ai.platform.ParchmentNativeCore`).
//!
//! Uses `jni 0.22` (`EnvUnowned::with_env`) so Rust panics can never unwind across JNI boundaries,
//! keeping the Android APK lean (0 JNA overhead, 0 reflection) and R8-safe.

use std::path::Path;
use std::sync::Arc;

use jni::EnvUnowned;
use jni::errors::{Error as JniError, ThrowRuntimeExAndDefault};
use jni::objects::{JByteArray, JClass, JString};
use jni::sys::{jboolean, jint, jlong};

use crate::ParchmentCore;
use parchment_coding::Task;

unsafe fn borrow_handle<'a>(handle: jlong) -> Option<&'a ParchmentCore> {
    if handle == 0 {
        None
    } else {
        Some(unsafe { &*(handle as usize as *const ParchmentCore) })
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_parchment_ai_platform_ParchmentNativeCore_nativeOpen<'caller>(
    mut unowned_env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    storage_dir: JString<'caller>,
) -> jlong {
    unowned_env
        .with_env(|env| -> Result<jlong, JniError> {
            let dir_str = storage_dir.try_to_string(env)?;
            let dir_path = Path::new(&dir_str);
            let _ = std::fs::create_dir_all(dir_path);
            let Ok(core) = ParchmentCore::open(dir_path.join("parchment.db")) else {
                return Ok(0);
            };
            // Idempotent, non-destructive migration of existing v2 Android files on first open.
            let _ = core.migrate_v2_data_dir(dir_path, false);
            Ok(Arc::into_raw(core) as usize as jlong)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_parchment_ai_platform_ParchmentNativeCore_nativeLoadSettingsJson<
    'caller,
>(
    mut unowned_env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    handle: jlong,
) -> JString<'caller> {
    unowned_env
        .with_env(|env| -> Result<JString<'caller>, JniError> {
            let Some(core) = (unsafe { borrow_handle(handle) }) else {
                return Ok(JString::null());
            };
            match core.load_settings_json() {
                Ok(Some(json)) => JString::from_str(env, json),
                _ => Ok(JString::null()),
            }
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_parchment_ai_platform_ParchmentNativeCore_nativeSaveSettingsJson<
    'caller,
>(
    mut unowned_env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    handle: jlong,
    json: JString<'caller>,
) -> jboolean {
    unowned_env
        .with_env(|env| -> Result<jboolean, JniError> {
            let Some(core) = (unsafe { borrow_handle(handle) }) else {
                return Ok(false as jboolean);
            };
            let raw = json.try_to_string(env)?;
            Ok(core.save_settings_json(&raw).is_ok())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_parchment_ai_platform_ParchmentNativeCore_nativeLoadChatJson<
    'caller,
>(
    mut unowned_env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    handle: jlong,
    chat_id: JString<'caller>,
) -> JString<'caller> {
    unowned_env
        .with_env(|env| -> Result<JString<'caller>, JniError> {
            let Some(core) = (unsafe { borrow_handle(handle) }) else {
                return Ok(JString::null());
            };
            let cid = chat_id.try_to_string(env)?;
            match core.load_chat_json(&cid) {
                Ok(Some(json)) => JString::from_str(env, json),
                _ => Ok(JString::null()),
            }
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_parchment_ai_platform_ParchmentNativeCore_nativeSaveChatJson<
    'caller,
>(
    mut unowned_env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    handle: jlong,
    json: JString<'caller>,
) -> jboolean {
    unowned_env
        .with_env(|env| -> Result<jboolean, JniError> {
            let Some(core) = (unsafe { borrow_handle(handle) }) else {
                return Ok(false as jboolean);
            };
            let raw = json.try_to_string(env)?;
            Ok(core.save_chat_json(&raw).is_ok())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_parchment_ai_platform_ParchmentNativeCore_nativeDeleteChat<
    'caller,
>(
    mut unowned_env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    handle: jlong,
    chat_id: JString<'caller>,
) -> jboolean {
    unowned_env
        .with_env(|env| -> Result<jboolean, JniError> {
            let Some(core) = (unsafe { borrow_handle(handle) }) else {
                return Ok(false as jboolean);
            };
            let cid = chat_id.try_to_string(env)?;
            Ok(core.delete_chat(&cid).unwrap_or(false))
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_parchment_ai_platform_ParchmentNativeCore_nativeSearchChatIds<
    'caller,
>(
    mut unowned_env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    handle: jlong,
    query: JString<'caller>,
    limit: jint,
) -> JString<'caller> {
    unowned_env
        .with_env(|env| -> Result<JString<'caller>, JniError> {
            let Some(core) = (unsafe { borrow_handle(handle) }) else {
                return Ok(JString::null());
            };
            let q = query.try_to_string(env)?;
            let lim = limit.max(1) as u32;
            match core.search_chat_ids_newline(&q, lim) {
                Ok(lines) => JString::from_str(env, lines),
                Err(_) => Ok(JString::null()),
            }
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_parchment_ai_platform_ParchmentNativeCore_nativeWorkspaceSearch<
    'caller,
>(
    mut unowned_env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    root_path: JString<'caller>,
    query: JString<'caller>,
    scope_path: JString<'caller>,
    file_glob: JString<'caller>,
    max_results: jint,
) -> JString<'caller> {
    unowned_env
        .with_env(|env| -> Result<JString<'caller>, JniError> {
            let root = root_path.try_to_string(env)?;
            let q = query.try_to_string(env)?;
            let scope = scope_path.try_to_string(env)?;
            let glob = file_glob.try_to_string(env)?;
            let max_res = max_results.clamp(1, 500) as usize;
            match ParchmentCore::workspace_search_native(&root, &q, &scope, &glob, max_res) {
                Ok(hits) => JString::from_str(env, hits),
                Err(_) => Ok(JString::null()),
            }
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_parchment_ai_platform_ParchmentNativeCore_nativeExtractDocx<
    'caller,
>(
    mut unowned_env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    bytes: JByteArray<'caller>,
) -> JString<'caller> {
    unowned_env
        .with_env(|env| -> Result<JString<'caller>, JniError> {
            let raw = env.convert_byte_array(&bytes)?;
            match ParchmentCore::extract_docx_native(&raw) {
                Ok(text) => JString::from_str(env, text),
                Err(_) => Ok(JString::null()),
            }
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// Lists coding task ids as a newline-separated string, newest first.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_parchment_ai_platform_ParchmentNativeCore_nativeListCodingTaskIds<
    'caller,
>(
    mut unowned_env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    handle: jlong,
) -> JString<'caller> {
    unowned_env
        .with_env(|env| -> Result<JString<'caller>, JniError> {
            let Some(core) = (unsafe { borrow_handle(handle) }) else {
                return Ok(JString::null());
            };
            let store = core.lock_store_for_jni();
            match store.list_coding_task_ids(None) {
                Ok(ids) => JString::from_str(env, ids.join("\n")),
                Err(_) => Ok(JString::null()),
            }
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// Loads one coding task as JSON, or null when it does not exist.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_parchment_ai_platform_ParchmentNativeCore_nativeLoadCodingTaskJson<
    'caller,
>(
    mut unowned_env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    handle: jlong,
    task_id: JString<'caller>,
) -> JString<'caller> {
    unowned_env
        .with_env(|env| -> Result<JString<'caller>, JniError> {
            let Some(core) = (unsafe { borrow_handle(handle) }) else {
                return Ok(JString::null());
            };
            let tid = task_id.try_to_string(env)?;
            let store = core.lock_store_for_jni();
            match store.load_coding_task_json(&tid) {
                Ok(Some(json)) => JString::from_str(env, json),
                _ => Ok(JString::null()),
            }
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// Saves a coding task from its JSON form. Returns true on success.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_parchment_ai_platform_ParchmentNativeCore_nativeSaveCodingTaskJson<
    'caller,
>(
    mut unowned_env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    handle: jlong,
    json: JString<'caller>,
) -> jboolean {
    unowned_env
        .with_env(|env| -> Result<jboolean, JniError> {
            let Some(core) = (unsafe { borrow_handle(handle) }) else {
                return Ok(false as jboolean);
            };
            let raw = json.try_to_string(env)?;
            let value: serde_json::Value = match serde_json::from_str(&raw) {
                Ok(v) => v,
                Err(_) => return Ok(false as jboolean),
            };
            let _task: Task = match serde_json::from_str(&raw) {
                Ok(t) => t,
                Err(_) => return Ok(false as jboolean),
            };
            let id = value.get("id").and_then(|v| v.as_str()).unwrap_or("");
            if id.is_empty() {
                return Ok(false as jboolean);
            }
            let mut store = core.lock_store_for_jni_mut();
            store
                .upsert_coding_task(
                    id,
                    value.get("title").and_then(|v| v.as_str()).unwrap_or(""),
                    value
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                    value
                        .get("state")
                        .and_then(|v| v.as_str())
                        .unwrap_or("PENDING"),
                    value
                        .get("workspacePath")
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                    value.get("repoUrl").and_then(|v| v.as_str()).unwrap_or(""),
                    value
                        .get("createdAtMs")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0),
                    value
                        .get("updatedAtMs")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0),
                    value.get("completedAtMs").and_then(|v| v.as_i64()),
                    &raw,
                    value
                        .get("ownerSessionId")
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                    value.get("ownerRunId").and_then(|v| v.as_str()),
                )
                .is_ok()
            })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// Deletes a coding task. Returns true when a row was removed.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_parchment_ai_platform_ParchmentNativeCore_nativeDeleteCodingTask<
    'caller,
>(
    mut unowned_env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    handle: jlong,
    task_id: JString<'caller>,
) -> jboolean {
    unowned_env
        .with_env(|env| -> Result<jboolean, JniError> {
            let Some(core) = (unsafe { borrow_handle(handle) }) else {
                return Ok(false as jboolean);
            };
            let tid = task_id.try_to_string(env)?;
            let mut store = core.lock_store_for_jni_mut();
            Ok(store.delete_coding_task(&tid).unwrap_or(false))
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}
