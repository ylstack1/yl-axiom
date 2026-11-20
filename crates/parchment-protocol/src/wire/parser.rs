use super::*;

/// Stateful per-round SSE parser translating all 5 wire dialects into unified `StreamEvent`s.
pub struct WireStreamParser {
    dialect: WireDialect,
    finished: bool,
    // Accumulators for chunked function/tool calls across SSE events.
    chat_tools: BTreeMap<usize, (String, String, String)>,
    anthropic_blocks: BTreeMap<usize, AnthropicBlockState>,
    responses_calls: BTreeMap<String, (String, String, String)>,
    gemini_fn_calls: BTreeMap<usize, (String, String, String, String)>,
    last_thought_sig: String,
}

enum AnthropicBlockState {
    Text,
    Thinking,
    ToolUse {
        id: String,
        name: String,
        args: String,
    },
}

impl WireStreamParser {
    pub fn new(dialect: WireDialect) -> Self {
        Self {
            dialect,
            finished: false,
            chat_tools: BTreeMap::new(),
            anthropic_blocks: BTreeMap::new(),
            responses_calls: BTreeMap::new(),
            gemini_fn_calls: BTreeMap::new(),
            last_thought_sig: String::new(),
        }
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    pub fn consume_sse(
        &mut self,
        event_type: &str,
        data: &str,
        out: &mut Vec<StreamEvent>,
    ) -> Result<(), ProtocolError> {
        if data == "[DONE]" {
            self.flush_pending_tools(out);
            self.finished = true;
            out.push(StreamEvent::Done);
            return Ok(());
        }
        let Ok(val) = serde_json::from_str::<Value>(data) else {
            return Ok(());
        };
        if let Some(arr) = val.as_array() {
            for item in arr {
                if let Some(obj) = item.as_object() {
                    self.dispatch_object(event_type, obj, out)?;
                }
            }
            return Ok(());
        }
        let Some(obj) = val.as_object() else {
            return Ok(());
        };
        self.dispatch_object(event_type, obj, out)
    }

    fn dispatch_object(
        &mut self,
        event_type: &str,
        obj: &Map<String, Value>,
        out: &mut Vec<StreamEvent>,
    ) -> Result<(), ProtocolError> {
        match self.dialect {
            WireDialect::OpenAiResponses => self.parse_openai_responses(event_type, obj, out),
            WireDialect::OpenAiChat => self.parse_openai_chat(obj, out),
            WireDialect::AnthropicMessages => self.parse_anthropic(event_type, obj, out),
            WireDialect::GeminiInteractions => self.parse_gemini_interactions(event_type, obj, out),
            WireDialect::GeminiGenerateContent => self.parse_gemini_generate_content(obj, out),
        }
    }

    pub fn flush_pending_tools(&mut self, out: &mut Vec<StreamEvent>) {
        let chat = std::mem::take(&mut self.chat_tools);
        for (_, (id, name, args)) in chat {
            if !name.is_empty() {
                out.push(StreamEvent::ToolCall {
                    id,
                    name,
                    arguments_json: if args.trim().is_empty() {
                        "{}".into()
                    } else {
                        args
                    },
                    thought_signature: String::new(),
                });
            }
        }
        let gem = std::mem::take(&mut self.gemini_fn_calls);
        for (_, (id, name, args, sig)) in gem {
            if !name.is_empty() {
                out.push(StreamEvent::ToolCall {
                    id,
                    name,
                    arguments_json: if args.trim().is_empty() {
                        "{}".into()
                    } else {
                        args
                    },
                    thought_signature: sig,
                });
            }
        }
    }

    fn parse_openai_responses(
        &mut self,
        event_type: &str,
        obj: &Map<String, Value>,
        out: &mut Vec<StreamEvent>,
    ) -> Result<(), ProtocolError> {
        let ev = obj
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or(event_type);
        match ev {
            "response.created" | "response.in_progress" => {
                if let Some(id) = obj
                    .get("response")
                    .and_then(|r| r.get("id"))
                    .and_then(Value::as_str)
                {
                    out.push(StreamEvent::InteractionId(id.to_string()));
                }
            }
            "response.output_text.delta" => {
                if let Some(d) = obj.get("delta").and_then(Value::as_str) {
                    out.push(StreamEvent::TextDelta(d.to_string()));
                }
            }
            "response.output_text.annotation.added" => {
                if let Some(ann) = obj.get("annotation").and_then(Value::as_object) {
                    extract_url_citation_annotation(ann, out);
                }
            }
            "response.reasoning_summary_text.delta" | "response.reasoning.delta" => {
                if let Some(d) = obj.get("delta").and_then(Value::as_str) {
                    out.push(StreamEvent::ThinkingDelta(d.to_string()));
                }
            }
            "response.output_item.added" => {
                if let Some(item) = obj.get("item").and_then(Value::as_object)
                    && item.get("type").and_then(Value::as_str) == Some("function_call")
                {
                    let item_id = item
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let call_id = item
                        .get("call_id")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .unwrap_or_else(|| item_id.clone());
                    let key = if !item_id.is_empty() {
                        item_id
                    } else {
                        call_id.clone()
                    };
                    let name = item
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let args = item
                        .get("arguments")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    if !key.is_empty() {
                        self.responses_calls.insert(key, (call_id, name, args));
                    }
                }
            }
            "response.function_call_arguments.delta" => {
                if let Some(delta) = obj.get("delta").and_then(Value::as_str) {
                    let item_id = obj.get("item_id").and_then(Value::as_str);
                    let call_id = obj.get("call_id").and_then(Value::as_str);
                    if let Some(key) = item_id
                        && let Some(entry) = self.responses_calls.get_mut(key)
                    {
                        entry.2.push_str(delta);
                    } else if let Some(cid) = call_id {
                        if let Some(entry) = self.responses_calls.get_mut(cid) {
                            entry.2.push_str(delta);
                        } else if let Some((_, entry)) = self
                            .responses_calls
                            .iter_mut()
                            .find(|(_, (stored_cid, _, _))| stored_cid == cid)
                        {
                            entry.2.push_str(delta);
                        }
                    }
                }
            }
            "response.output_item.done" => {
                if let Some(item) = obj.get("item").and_then(Value::as_object) {
                    match item.get("type").and_then(Value::as_str) {
                        Some("reasoning") => {
                            if let Some(enc) = item.get("encrypted_content").and_then(Value::as_str)
                            {
                                out.push(StreamEvent::ThinkingSignature(enc.to_string()));
                            }
                        }
                        Some("message") => {
                            if let Some(content) = item.get("content").and_then(Value::as_array) {
                                for part in content {
                                    if let Some(anns) =
                                        part.get("annotations").and_then(Value::as_array)
                                    {
                                        for ann in anns {
                                            if let Some(aobj) = ann.as_object() {
                                                extract_url_citation_annotation(aobj, out);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        Some("function_call") => {
                            let item_id = item.get("id").and_then(Value::as_str).unwrap_or("");
                            let raw_call_id = item
                                .get("call_id")
                                .and_then(Value::as_str)
                                .filter(|s| !s.is_empty())
                                .unwrap_or(item_id);
                            let cached = (!item_id.is_empty())
                                .then(|| self.responses_calls.remove(item_id))
                                .flatten()
                                .or_else(|| {
                                    (!raw_call_id.is_empty())
                                        .then(|| self.responses_calls.remove(raw_call_id))
                                        .flatten()
                                });
                            let call_id = if !raw_call_id.is_empty() {
                                raw_call_id.to_string()
                            } else {
                                cached.as_ref().map(|c| c.0.clone()).unwrap_or_default()
                            };
                            let name = item
                                .get("name")
                                .and_then(Value::as_str)
                                .filter(|s| !s.is_empty())
                                .map(str::to_string)
                                .or_else(|| cached.as_ref().map(|c| c.1.clone()))
                                .unwrap_or_default();
                            let args = item
                                .get("arguments")
                                .and_then(Value::as_str)
                                .filter(|s| !s.trim().is_empty())
                                .map(str::to_string)
                                .or_else(|| cached.map(|c| c.2).filter(|s| !s.trim().is_empty()))
                                .unwrap_or_else(|| "{}".to_string());
                            if !name.is_empty() {
                                out.push(StreamEvent::ToolCall {
                                    id: call_id,
                                    name,
                                    arguments_json: args,
                                    thought_signature: String::new(),
                                });
                            }
                        }
                        _ => {}
                    }
                }
            }
            "response.completed" | "response.incomplete" => {
                let resp = obj.get("response").and_then(Value::as_object);
                let resp_id = resp
                    .and_then(|r| r.get("id"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                if let Some(usage_obj) =
                    resp.and_then(|r| r.get("usage")).and_then(Value::as_object)
                {
                    out.push(StreamEvent::Usage {
                        response_id: resp_id,
                        usage: parse_provider_usage(
                            UsageDialect::OpenAiResponses,
                            Some(usage_obj),
                            true,
                            resp,
                        ),
                    });
                }
                if ev == "response.incomplete" {
                    out.push(StreamEvent::OutputLimit);
                }
                self.finished = true;
                out.push(StreamEvent::Done);
            }
            "response.failed" | "error" => {
                let msg = obj
                    .get("error")
                    .or_else(|| obj.get("response").and_then(|r| r.get("error")))
                    .and_then(|e| e.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or("OpenAI Responses stream error");
                return Err(ProtocolError::StreamInterrupted(msg.to_string()));
            }
            _ => {}
        }
        Ok(())
    }

    fn parse_openai_chat(
        &mut self,
        obj: &Map<String, Value>,
        out: &mut Vec<StreamEvent>,
    ) -> Result<(), ProtocolError> {
        if let Some(err) = obj.get("error") {
            let msg = err
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("OpenAI Chat stream error");
            return Err(ProtocolError::StreamInterrupted(msg.to_string()));
        }
        let resp_id = obj
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if let Some(usage_obj) = obj.get("usage").and_then(Value::as_object) {
            out.push(StreamEvent::Usage {
                response_id: resp_id,
                usage: parse_provider_usage(
                    UsageDialect::OpenAiChat,
                    Some(usage_obj),
                    true,
                    Some(obj),
                ),
            });
        }
        if let Some(choice) = obj
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(Value::as_object)
        {
            if let Some(delta) = choice.get("delta").and_then(Value::as_object) {
                if let Some(r) = delta
                    .get("reasoning_content")
                    .or_else(|| delta.get("reasoning"))
                    .and_then(Value::as_str)
                    && !r.is_empty()
                {
                    out.push(StreamEvent::ThinkingDelta(r.to_string()));
                }
                if let Some(c) = delta.get("content").and_then(Value::as_str)
                    && !c.is_empty()
                {
                    out.push(StreamEvent::TextDelta(c.to_string()));
                }
                if let Some(tcs) = delta.get("tool_calls").and_then(Value::as_array) {
                    for tc in tcs {
                        let idx = tc.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                        let entry = self
                            .chat_tools
                            .entry(idx)
                            .or_insert_with(|| (String::new(), String::new(), String::new()));
                        if let Some(id) = tc.get("id").and_then(Value::as_str)
                            && !id.is_empty()
                        {
                            entry.0 = id.to_string();
                        }
                        if let Some(func) = tc.get("function").and_then(Value::as_object) {
                            if let Some(n) = func.get("name").and_then(Value::as_str)
                                && !n.is_empty()
                            {
                                entry.1.push_str(n);
                            }
                            if let Some(a) = func.get("arguments").and_then(Value::as_str) {
                                entry.2.push_str(a);
                            }
                        }
                    }
                }
            }
            if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
                self.flush_pending_tools(out);
                if reason == "length" {
                    out.push(StreamEvent::OutputLimit);
                }
                self.finished = true;
            }
        }
        Ok(())
    }

    fn parse_anthropic(
        &mut self,
        event_type: &str,
        obj: &Map<String, Value>,
        out: &mut Vec<StreamEvent>,
    ) -> Result<(), ProtocolError> {
        let ev = obj
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or(event_type);
        match ev {
            "message_start" => {
                if let Some(msg) = obj.get("message").and_then(Value::as_object) {
                    let resp_id = msg
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    if let Some(u) = msg.get("usage").and_then(Value::as_object) {
                        out.push(StreamEvent::Usage {
                            response_id: resp_id,
                            usage: parse_provider_usage(
                                UsageDialect::Anthropic,
                                Some(u),
                                false,
                                Some(msg),
                            ),
                        });
                    }
                }
            }
            "content_block_start" => {
                let idx = obj.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                if let Some(blk) = obj.get("content_block").and_then(Value::as_object) {
                    match blk.get("type").and_then(Value::as_str) {
                        Some("text") => {
                            self.anthropic_blocks.insert(idx, AnthropicBlockState::Text);
                        }
                        Some("thinking") => {
                            self.anthropic_blocks
                                .insert(idx, AnthropicBlockState::Thinking);
                        }
                        Some("tool_use") => {
                            let id = blk
                                .get("id")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string();
                            let name = blk
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string();
                            self.anthropic_blocks.insert(
                                idx,
                                AnthropicBlockState::ToolUse {
                                    id,
                                    name,
                                    args: String::new(),
                                },
                            );
                        }
                        Some("web_search_tool_result") => {
                            if let Some(items) = blk.get("content").and_then(Value::as_array) {
                                for item in items {
                                    if let Some(iobj) = item.as_object() {
                                        push_citation_from_fields(iobj, out);
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            "content_block_delta" => {
                let idx = obj.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                if let Some(delta) = obj.get("delta").and_then(Value::as_object) {
                    match delta.get("type").and_then(Value::as_str) {
                        Some("text_delta") => {
                            if let Some(t) = delta.get("text").and_then(Value::as_str) {
                                out.push(StreamEvent::TextDelta(t.to_string()));
                            }
                        }
                        Some("thinking_delta") => {
                            if let Some(t) = delta.get("thinking").and_then(Value::as_str) {
                                out.push(StreamEvent::ThinkingDelta(t.to_string()));
                            }
                        }
                        Some("signature_delta") => {
                            if let Some(s) = delta.get("signature").and_then(Value::as_str) {
                                out.push(StreamEvent::ThinkingSignature(s.to_string()));
                            }
                        }
                        Some("input_json_delta") => {
                            if let Some(j) = delta.get("partial_json").and_then(Value::as_str)
                                && let Some(AnthropicBlockState::ToolUse { args, .. }) =
                                    self.anthropic_blocks.get_mut(&idx)
                            {
                                args.push_str(j);
                            }
                        }
                        Some("citations_delta") => {
                            if let Some(cit) = delta.get("citation").and_then(Value::as_object) {
                                push_citation_from_fields(cit, out);
                            }
                        }
                        _ => {}
                    }
                }
            }
            "content_block_stop" => {
                let idx = obj.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                if let Some(AnthropicBlockState::ToolUse { id, name, args }) =
                    self.anthropic_blocks.remove(&idx)
                    && !name.is_empty()
                {
                    out.push(StreamEvent::ToolCall {
                        id,
                        name,
                        arguments_json: if args.trim().is_empty() {
                            "{}".into()
                        } else {
                            args
                        },
                        thought_signature: String::new(),
                    });
                }
            }
            "message_delta" => {
                if let Some(u) = obj.get("usage").and_then(Value::as_object) {
                    out.push(StreamEvent::Usage {
                        response_id: String::new(),
                        usage: parse_provider_usage(
                            UsageDialect::Anthropic,
                            Some(u),
                            true,
                            Some(obj),
                        ),
                    });
                }
                if obj
                    .get("delta")
                    .and_then(|d| d.get("stop_reason"))
                    .and_then(Value::as_str)
                    == Some("max_tokens")
                {
                    out.push(StreamEvent::OutputLimit);
                }
            }
            "message_stop" => {
                self.finished = true;
                out.push(StreamEvent::Done);
            }
            "error" => {
                let msg = obj
                    .get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or("Anthropic stream error");
                return Err(ProtocolError::StreamInterrupted(msg.to_string()));
            }
            _ => {}
        }
        Ok(())
    }

    fn parse_gemini_interactions(
        &mut self,
        event_type: &str,
        obj: &Map<String, Value>,
        out: &mut Vec<StreamEvent>,
    ) -> Result<(), ProtocolError> {
        let ev = obj
            .get("event_type")
            .or_else(|| obj.get("type"))
            .and_then(Value::as_str)
            .unwrap_or(event_type);
        let inter = obj.get("interaction").and_then(Value::as_object);
        if let Some(id) = inter
            .and_then(|i| i.get("id"))
            .or_else(|| obj.get("interaction_id"))
            .and_then(Value::as_str)
            && !id.is_empty()
        {
            out.push(StreamEvent::InteractionId(id.to_string()));
        }
        if let Some(u) = obj
            .get("usage")
            .or_else(|| inter.and_then(|i| i.get("usage")))
            .and_then(Value::as_object)
        {
            let is_term = matches!(
                ev,
                "interaction.complete" | "interaction.completed" | "interaction.incomplete"
            );
            out.push(StreamEvent::Usage {
                response_id: String::new(),
                usage: parse_provider_usage(
                    UsageDialect::GeminiInteractions,
                    Some(u),
                    is_term,
                    inter.or(Some(obj)),
                ),
            });
        }
        extract_gemini_grounding_citations(obj, out);
        if let Some(iobj) = inter {
            extract_gemini_grounding_citations(iobj, out);
            if let Some(outputs) = iobj.get("outputs").and_then(Value::as_array) {
                for item in outputs {
                    if let Some(anns) = item.get("annotations").and_then(Value::as_array) {
                        for ann in anns {
                            if let Some(aobj) = ann.as_object() {
                                push_citation_from_fields(aobj, out);
                            }
                        }
                    }
                }
            }
        }
        match ev {
            "content.delta" | "step.delta" => {
                let idx = obj.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                if let Some(delta) = obj.get("delta").and_then(Value::as_object) {
                    if let Some(anns) = delta.get("annotations").and_then(Value::as_array) {
                        for ann in anns {
                            if let Some(aobj) = ann.as_object() {
                                push_citation_from_fields(aobj, out);
                            }
                        }
                    }
                    match delta.get("type").and_then(Value::as_str) {
                        Some("text") => {
                            if let Some(t) = delta.get("text").and_then(Value::as_str) {
                                out.push(StreamEvent::TextDelta(t.to_string()));
                            }
                        }
                        Some("thought") | Some("thought_summary") => {
                            if let Some(t) = delta
                                .get("content")
                                .and_then(|c| c.get("text"))
                                .or_else(|| delta.get("text"))
                                .and_then(Value::as_str)
                            {
                                out.push(StreamEvent::ThinkingDelta(t.to_string()));
                            }
                            if let Some(sig) = delta.get("signature").and_then(Value::as_str) {
                                self.last_thought_sig = sig.to_string();
                                out.push(StreamEvent::ThinkingSignature(sig.to_string()));
                            }
                        }
                        Some("thought_signature") => {
                            if let Some(sig) = delta.get("signature").and_then(Value::as_str) {
                                self.last_thought_sig = sig.to_string();
                                out.push(StreamEvent::ThinkingSignature(sig.to_string()));
                            }
                        }
                        Some("function_call") => {
                            let id = delta
                                .get("id")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string();
                            let name = delta
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string();
                            let args = delta
                                .get("arguments")
                                .map(|a| {
                                    a.as_str()
                                        .map(str::to_string)
                                        .unwrap_or_else(|| a.to_string())
                                })
                                .unwrap_or_default();
                            let entry = self.gemini_fn_calls.entry(idx).or_insert_with(|| {
                                (
                                    id.clone(),
                                    name.clone(),
                                    String::new(),
                                    self.last_thought_sig.clone(),
                                )
                            });
                            if !id.is_empty() {
                                entry.0 = id;
                            }
                            if !name.is_empty() {
                                entry.1 = name;
                            }
                            entry.2.push_str(&args);
                        }
                        _ => {}
                    }
                }
            }
            "interaction.complete"
            | "interaction.completed"
            | "interaction.requires_action"
            | "interaction.incomplete" => {
                self.flush_pending_tools(out);
                if ev == "interaction.incomplete" {
                    out.push(StreamEvent::OutputLimit);
                }
                self.finished = true;
                out.push(StreamEvent::Done);
            }
            "interaction.failed" | "error" => {
                let msg = obj
                    .get("error")
                    .or_else(|| inter.and_then(|i| i.get("error")))
                    .and_then(|e| e.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or("Gemini Interactions error");
                return Err(ProtocolError::NoRetry(msg.to_string()));
            }
            _ => {}
        }
        Ok(())
    }

    fn parse_gemini_generate_content(
        &mut self,
        obj: &Map<String, Value>,
        out: &mut Vec<StreamEvent>,
    ) -> Result<(), ProtocolError> {
        if let Some(err) = obj.get("error") {
            let msg = err
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Gemini generateContent error");
            return Err(ProtocolError::StreamInterrupted(msg.to_string()));
        }
        if let Some(u) = obj.get("usageMetadata").and_then(Value::as_object) {
            let is_final = obj
                .get("candidates")
                .and_then(Value::as_array)
                .and_then(|a| a.first())
                .and_then(|c| c.get("finishReason"))
                .and_then(Value::as_str)
                .is_some();
            out.push(StreamEvent::Usage {
                response_id: String::new(),
                usage: parse_provider_usage(
                    UsageDialect::GeminiContent,
                    Some(u),
                    is_final,
                    Some(obj),
                ),
            });
        }
        if let Some(cand) = obj
            .get("candidates")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(Value::as_object)
        {
            extract_gemini_grounding_citations(cand, out);
            if let Some(parts) = cand
                .get("content")
                .and_then(|c| c.get("parts"))
                .and_then(Value::as_array)
            {
                for (p_idx, part) in parts.iter().enumerate() {
                    let Some(pobj) = part.as_object() else {
                        continue;
                    };
                    let is_server_tool_part = pobj.contains_key("toolCall")
                        || pobj.contains_key("toolResponse")
                        || pobj.contains_key("executableCode")
                        || pobj.contains_key("codeExecutionResult");
                    if !is_server_tool_part
                        && let Some(raw_sig) = pobj
                            .get("thoughtSignature")
                            .or_else(|| pobj.get("thought_signature"))
                            .and_then(Value::as_str)
                    {
                        let sig = sanitize_gemini_thought_signature(
                            raw_sig,
                            pobj.contains_key("functionCall"),
                        );
                        if !sig.is_empty() {
                            self.last_thought_sig = sig.clone();
                            out.push(StreamEvent::ThinkingSignature(sig));
                        }
                    }
                    let is_thought = pobj.get("thought").and_then(Value::as_bool) == Some(true);
                    if let Some(text) = pobj.get("text").and_then(Value::as_str)
                        && !text.is_empty()
                    {
                        if is_thought {
                            out.push(StreamEvent::ThinkingDelta(text.to_string()));
                        } else {
                            out.push(StreamEvent::TextDelta(text.to_string()));
                        }
                    }
                    if let Some(fc) = pobj.get("functionCall").and_then(Value::as_object) {
                        let name = fc
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        let args = fc
                            .get("args")
                            .map(|a| a.to_string())
                            .unwrap_or_else(|| "{}".to_string());
                        if !name.is_empty() {
                            out.push(StreamEvent::ToolCall {
                                id: format!("gem_call_{p_idx}"),
                                name,
                                arguments_json: args,
                                thought_signature: self.last_thought_sig.clone(),
                            });
                        }
                    }
                }
            }
            if let Some(reason) = cand.get("finishReason").and_then(Value::as_str) {
                if reason == "MAX_TOKENS" {
                    out.push(StreamEvent::OutputLimit);
                }
                self.finished = true;
                out.push(StreamEvent::Done);
            }
        }
        Ok(())
    }
}

fn push_citation_from_fields(obj: &Map<String, Value>, out: &mut Vec<StreamEvent>) {
    let url = obj
        .get("url")
        .or_else(|| obj.get("uri"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if url.is_empty() {
        return;
    }
    let title = obj
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let snippet = obj
        .get("snippet")
        .or_else(|| obj.get("cited_text"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    out.push(StreamEvent::Citation(RawWebCitation {
        url: url.to_string(),
        title,
        snippet,
    }));
}

fn extract_url_citation_annotation(ann: &Map<String, Value>, out: &mut Vec<StreamEvent>) {
    if ann.get("type").and_then(Value::as_str) != Some("url_citation") {
        return;
    }
    if let Some(nested) = ann.get("url_citation").and_then(Value::as_object) {
        push_citation_from_fields(nested, out);
    } else {
        push_citation_from_fields(ann, out);
    }
}

fn extract_gemini_grounding_citations(obj: &Map<String, Value>, out: &mut Vec<StreamEvent>) {
    let Some(gm) = obj
        .get("groundingMetadata")
        .or_else(|| obj.get("grounding_metadata"))
        .and_then(Value::as_object)
    else {
        return;
    };
    let Some(chunks) = gm
        .get("groundingChunks")
        .or_else(|| gm.get("grounding_chunks"))
        .and_then(Value::as_array)
    else {
        return;
    };
    for chunk in chunks {
        if let Some(web) = chunk.get("web").and_then(Value::as_object) {
            push_citation_from_fields(web, out);
        }
    }
}
