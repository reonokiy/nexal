//! Chat Completions API client (`/v1/chat/completions`).
//!
//! Translates between the internal ResponseEvent stream and the standard
//! OpenAI Chat Completions SSE format, enabling support for providers like
//! Moonshot, DeepSeek, Anthropic (via proxy), and any OpenAI-compatible API.

use std::collections::HashMap;
use std::pin::Pin;

use futures::Stream;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;
use tracing::{debug, warn};

use crate::common::ResponseEvent;
use nexal_protocol::models::ResponseItem;
use nexal_protocol::protocol::TokenUsage;

/// A streaming Chat Completions session.
pub struct ChatCompletionsSession {
    base_url: String,
    headers: reqwest::header::HeaderMap,
    client: reqwest::Client,
}

impl ChatCompletionsSession {
    pub fn new(base_url: String, headers: reqwest::header::HeaderMap, client: reqwest::Client) -> Self {
        Self { base_url, headers, client }
    }

    /// Stream a chat completions request, yielding ResponseEvent items.
    pub async fn stream(
        &self,
        messages: Vec<ChatMessage>,
        model: &str,
        tools: Option<Vec<ChatTool>>,
        temperature: Option<f64>,
        max_tokens: Option<i64>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ResponseEvent, ChatCompletionsError>> + Send>>, ChatCompletionsError>
    {
        let url = format!(
            "{}/chat/completions",
            self.base_url.trim_end_matches('/')
        );

        let mut body = serde_json::json!({
            "model": model,
            "messages": messages,
            "stream": true,
            "stream_options": { "include_usage": true },
        });

        if let Some(tools) = &tools {
            if !tools.is_empty() {
                body["tools"] = serde_json::to_value(tools)
                    .map_err(|e| ChatCompletionsError::Serialization(e.to_string()))?;
            }
        }
        if let Some(temp) = temperature {
            body["temperature"] = serde_json::json!(temp);
        }
        if let Some(max) = max_tokens {
            body["max_tokens"] = serde_json::json!(max);
        }

        // OpenTelemetry GenAI semantic convention span.
        // Compact span attributes for logs; verbose data goes to debug events (OTLP only).
        let gen_ai_span = tracing::info_span!(
            "gen_ai.chat",
            "gen_ai.system" = extract_provider(&self.base_url),
            "gen_ai.request.model" = %model,
            "gen_ai.usage.input_tokens" = tracing::field::Empty,
            "gen_ai.usage.output_tokens" = tracing::field::Empty,
            "gen_ai.response.finish_reason" = tracing::field::Empty,
        );

        let _enter = gen_ai_span.enter();

        // Verbose data → debug events (visible in OTLP, not in default logs).
        // Redact inline data URIs (base64 images) to keep logs readable.
        if let Ok(msgs_json) = serde_json::to_string(&messages) {
            let redacted = redact_data_uris(&msgs_json);
            debug!(gen_ai.prompt.messages = %redacted, "prompt");
        }
        if let Some(ref tools) = tools {
            let tool_names: Vec<&str> = tools.iter().map(|t| t.function.name.as_str()).collect();
            debug!(gen_ai.request.available_tools = ?tool_names, "available tools");
        }

        debug!(url = %url, model = %model, "starting chat completions stream");

        let response = self
            .client
            .post(&url)
            .headers(self.headers.clone())
            .header("content-type", "application/json")
            .body(body.to_string())
            .send()
            .await
            .map_err(|e| ChatCompletionsError::Http(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown".to_string());
            return Err(ChatCompletionsError::ApiError {
                status: status.as_u16(),
                body,
            });
        }

        let byte_stream = response.bytes_stream();
        let (tx, rx) = mpsc::channel::<Result<ResponseEvent, ChatCompletionsError>>(64);

        // Spawn SSE parser task
        let span = gen_ai_span.clone();
        drop(_enter);
        tokio::spawn(async move {
            let mut sse_stream = SseParser::new(byte_stream);
            let mut tool_calls: HashMap<i32, PartialToolCall> = HashMap::new();
            let mut response_id = String::new();
            let mut sent_created = false;
            let mut sent_message_item = false;

            while let Some(event) = sse_stream.next().await {
                match event {
                    SseEvent::Data(data) => {
                        if data.trim() == "[DONE]" {
                            // Final event — emit Completed
                            // Flush any pending tool calls
                            for (_, tc) in tool_calls.drain() {
                                let item = tc.into_response_item();
                                let _ = tx.send(Ok(ResponseEvent::OutputItemDone(item))).await;
                            }
                            let _ = tx
                                .send(Ok(ResponseEvent::Completed {
                                    response_id: response_id.clone(),
                                    token_usage: None,
                                }))
                                .await;
                            break;
                        }

                        let chunk: ChatCompletionChunk = match serde_json::from_str(&data) {
                            Ok(c) => c,
                            Err(e) => {
                                warn!(data = %data, "failed to parse SSE chunk: {e}");
                                continue;
                            }
                        };

                        if !sent_created {
                            let _ = tx.send(Ok(ResponseEvent::Created)).await;
                            sent_created = true;
                        }

                        response_id = chunk.id.unwrap_or(response_id);

                        // Handle usage (often in the final chunk with stream_options)
                        if let Some(usage) = chunk.usage {
                            let input_tokens = usage.prompt_tokens.unwrap_or(0);
                            let output_tokens = usage.completion_tokens.unwrap_or(0);
                            span.record("gen_ai.usage.input_tokens", input_tokens);
                            span.record("gen_ai.usage.output_tokens", output_tokens);
                            span.record("gen_ai.response.id", &response_id);
                            let token_usage = TokenUsage {
                                input_tokens,
                                output_tokens,
                                total_tokens: usage.total_tokens.unwrap_or(0),
                                ..Default::default()
                            };
                            let _ = tx
                                .send(Ok(ResponseEvent::Completed {
                                    response_id: response_id.clone(),
                                    token_usage: Some(token_usage),
                                }))
                                .await;
                            break;
                        }

                        for choice in chunk.choices.unwrap_or_default() {
                            let delta = choice.delta;

                            // Text content
                            if let Some(content) = delta.content {
                                if !content.is_empty() {
                                    // Emit OutputItemAdded before the first text delta
                                    // (the core expects this to set up the active item)
                                    if !sent_message_item {
                                        sent_message_item = true;
                                        let msg_item = ResponseItem::Message {
                                            id: None,
                                            role: "assistant".to_string(),
                                            content: vec![],
                                            end_turn: None,
                                            phase: None,
                                        };
                                        let _ = tx
                                            .send(Ok(ResponseEvent::OutputItemAdded(msg_item)))
                                            .await;
                                    }
                                    let _ = tx
                                        .send(Ok(ResponseEvent::OutputTextDelta(content)))
                                        .await;
                                }
                            }

                            // Tool calls (streamed incrementally)
                            if let Some(tcs) = delta.tool_calls {
                                for tc in tcs {
                                    let idx = tc.index.unwrap_or(0);
                                    let entry = tool_calls.entry(idx).or_insert_with(|| {
                                        PartialToolCall {
                                            id: String::new(),
                                            name: String::new(),
                                            arguments: String::new(),
                                        }
                                    });
                                    if let Some(id) = tc.id {
                                        entry.id = id;
                                    }
                                    if let Some(func) = tc.function {
                                        if let Some(name) = func.name {
                                            entry.name = name;
                                        }
                                        if let Some(args) = func.arguments {
                                            entry.arguments.push_str(&args);
                                        }
                                    }
                                }
                            }

                            // finish_reason signals end of generation
                            if let Some(reason) = choice.finish_reason {
                                span.record("gen_ai.response.finish_reason", reason.as_str());
                                // Flush tool calls
                                if reason == "tool_calls" || reason == "stop" {
                                    for (_, tc) in tool_calls.drain() {
                                        // Log each tool call as an event on the parent span.
                                        tracing::info!(
                                            parent: &span,
                                            tool_call.name = %tc.name,
                                            tool_call.id = %tc.id,
                                            tool_call.arguments = %tc.arguments,
                                            "gen_ai.tool_call"
                                        );
                                        let item = tc.into_response_item();
                                        let _ = tx
                                            .send(Ok(ResponseEvent::OutputItemDone(item)))
                                            .await;
                                    }
                                }
                                // Emit OutputItemDone for the message if we streamed text
                                if reason == "stop" && sent_message_item {
                                    let done_item = ResponseItem::Message {
                                        id: None,
                                        role: "assistant".to_string(),
                                        content: vec![], // text was already streamed
                                        end_turn: Some(true),
                                        phase: None,
                                    };
                                    let _ = tx
                                        .send(Ok(ResponseEvent::OutputItemDone(done_item)))
                                        .await;
                                }
                            }
                        }
                    }
                    SseEvent::Error(e) => {
                        let _ = tx.send(Err(ChatCompletionsError::Stream(e))).await;
                        break;
                    }
                }
            }
        });

        Ok(Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }
}

/// Accumulated partial tool call from streaming chunks.
struct PartialToolCall {
    id: String,
    name: String,
    arguments: String,
}

impl PartialToolCall {
    fn into_response_item(self) -> ResponseItem {
        // Map all tool calls as FunctionCall — the core's tool router
        // will determine if it's a shell command or MCP tool.
        ResponseItem::FunctionCall {
            id: Some(self.id.clone()),
            name: self.name,
            namespace: None,
            call_id: self.id,
            arguments: self.arguments,
        }
    }
}

// ── SSE Parser ──────────────────────────────────────────────────────────

enum SseEvent {
    Data(String),
    Error(String),
}

struct SseParser<S> {
    stream: S,
    buffer: String,
}

impl<S> SseParser<S>
where
    S: Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Unpin,
{
    fn new(stream: S) -> Self {
        Self {
            stream,
            buffer: String::new(),
        }
    }

    async fn next(&mut self) -> Option<SseEvent> {
        loop {
            // Try to extract a complete SSE event from the buffer
            if let Some(event) = self.try_parse_event() {
                return Some(event);
            }

            // Read more data from the stream
            match self.stream.next().await {
                Some(Ok(bytes)) => {
                    self.buffer
                        .push_str(&String::from_utf8_lossy(&bytes));
                }
                Some(Err(e)) => {
                    return Some(SseEvent::Error(e.to_string()));
                }
                None => {
                    // Stream ended — try to parse any remaining data
                    if !self.buffer.trim().is_empty() {
                        return self.try_parse_event();
                    }
                    return None;
                }
            }
        }
    }

    fn try_parse_event(&mut self) -> Option<SseEvent> {
        // SSE events are separated by double newlines
        let sep = if self.buffer.contains("\n\n") {
            "\n\n"
        } else if self.buffer.contains("\r\n\r\n") {
            "\r\n\r\n"
        } else {
            return None;
        };

        let idx = self.buffer.find(sep)?;
        let event_text = self.buffer[..idx].to_string();
        self.buffer = self.buffer[idx + sep.len()..].to_string();

        // Parse "data: ..." lines
        for line in event_text.lines() {
            if let Some(data) = line.strip_prefix("data: ").or_else(|| line.strip_prefix("data:"))
            {
                return Some(SseEvent::Data(data.to_string()));
            }
        }

        None
    }
}

// ── Chat Completions API Types ──────────────────────────────────────────

#[derive(Debug, Serialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ChatToolCallMessage>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// Required by providers with thinking/reasoning mode (e.g. Kimi).
    /// Set via `thinking_mode = true` in provider config.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct ChatToolCallMessage {
    pub id: String,
    pub r#type: String,
    pub function: ChatFunctionCall,
}

#[derive(Debug, Serialize, Clone)]
pub struct ChatFunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct ChatTool {
    pub r#type: String,
    pub function: ChatToolFunction,
}

#[derive(Debug, Serialize, Clone)]
pub struct ChatToolFunction {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionChunk {
    id: Option<String>,
    choices: Option<Vec<ChunkChoice>>,
    usage: Option<ChunkUsage>,
}

#[derive(Debug, Deserialize)]
struct ChunkChoice {
    delta: ChunkDelta,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct ChunkDelta {
    content: Option<String>,
    tool_calls: Option<Vec<ChunkToolCall>>,
}

#[derive(Debug, Deserialize)]
struct ChunkToolCall {
    index: Option<i32>,
    id: Option<String>,
    function: Option<ChunkFunction>,
}

#[derive(Debug, Deserialize)]
struct ChunkFunction {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChunkUsage {
    prompt_tokens: Option<i64>,
    completion_tokens: Option<i64>,
    total_tokens: Option<i64>,
}

// ── Error Type ──────────────────────────────────────────────────────────

/// Replace inline `data:` URIs with a short placeholder to keep logs readable.
/// Matches patterns like `"data:image/jpeg;base64,/9j/4AAQ..."` and replaces
/// the base64 payload with `<base64:N bytes>`.
fn redact_data_uris(s: &str) -> String {
    // Fast path: no data URIs at all.
    if !s.contains("data:") {
        return s.to_string();
    }

    let mut result = String::with_capacity(s.len());
    let mut rest = s;

    while let Some(start) = rest.find("data:") {
        result.push_str(&rest[..start]);

        // Find the base64 payload after the comma.
        let after = &rest[start..];
        if let Some(comma) = after.find(",") {
            let header = &after[..comma]; // e.g. "data:image/jpeg;base64"

            // Find the end of the base64 string (next quote or whitespace).
            let payload_start = comma + 1;
            let payload_end = after[payload_start..]
                .find(|c: char| c == '"' || c == '\'' || c.is_whitespace())
                .map(|i| payload_start + i)
                .unwrap_or(after.len());

            let payload_len = payload_end - payload_start;
            // Estimate original byte size (base64 is ~4/3 of original).
            let byte_size = payload_len * 3 / 4;

            result.push_str(header);
            result.push_str(&format!(",<base64:{byte_size} bytes>"));
            rest = &rest[start + payload_end..];
        } else {
            // No comma found — not a real data URI, just copy it through.
            result.push_str("data:");
            rest = &rest[start + 5..];
        }
    }

    result.push_str(rest);
    result
}

/// Extract a provider name from the base URL for telemetry.
fn extract_provider(base_url: &str) -> &str {
    if base_url.contains("openai.com") {
        "openai"
    } else if base_url.contains("anthropic.com") {
        "anthropic"
    } else if base_url.contains("moonshot.cn") {
        "moonshot"
    } else if base_url.contains("deepseek.com") {
        "deepseek"
    } else if base_url.contains("googleapis.com") {
        "google"
    } else {
        "custom"
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ChatCompletionsError {
    #[error("HTTP error: {0}")]
    Http(String),
    #[error("API error (status {status}): {body}")]
    ApiError { status: u16, body: String },
    #[error("stream error: {0}")]
    Stream(String),
    #[error("serialization error: {0}")]
    Serialization(String),
}
