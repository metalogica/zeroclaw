use crate::multimodal;
use crate::providers::traits::{
    ChatMessage, ChatRequest as ProviderChatRequest, ChatResponse as ProviderChatResponse,
    Provider, ProviderCapabilities, TokenUsage, ToolCall as ProviderToolCall,
};
use crate::tools::ToolSpec;
use async_trait::async_trait;
use base64::Engine;
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub struct OpenRouterProvider {
    credential: Option<String>,
    timeout_secs: u64,
    max_tokens: Option<u32>,
    /// Output modalities for the request (e.g. `["image"]`, `["image", "text"]`).
    /// When `None`, the standard text-only chat completion behavior is used.
    modalities: Option<Vec<String>>,
    /// Workspace directory for saving generated images to disk.
    /// When set, image generation responses are written to `{workspace}/media/`
    /// and the response text is replaced with the file path.
    workspace_dir: Option<PathBuf>,
}

const DEFAULT_OPENROUTER_TIMEOUT_SECS: u64 = 120;
const OPENROUTER_CONNECT_TIMEOUT_SECS: u64 = 10;

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    temperature: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    modalities: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
struct Message {
    role: String,
    content: MessageContent,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum MessageContent {
    Text(String),
    Parts(Vec<MessagePart>),
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum MessagePart {
    Text { text: String },
    ImageUrl { image_url: ImageUrlPart },
    InputAudio { input_audio: InputAudioPart },
}

#[derive(Debug, Serialize)]
struct InputAudioPart {
    data: String,
    format: String,
}

#[derive(Debug, Serialize)]
struct ImageUrlPart {
    url: String,
}

#[derive(Debug, Deserialize)]
struct ApiChatResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ResponseMessage,
}

#[derive(Debug, Deserialize)]
struct ResponseMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    reasoning_details: Option<Vec<ReasoningDetail>>,
    /// Image generation models return generated images here as data URIs.
    #[serde(default)]
    images: Option<Vec<ImageResponse>>,
}

#[derive(Debug, Clone, Deserialize)]
struct ImageResponse {
    #[serde(default)]
    image_url: Option<ImageResponseUrl>,
}

#[derive(Debug, Clone, Deserialize)]
struct ImageResponseUrl {
    #[serde(default)]
    url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ReasoningDetail {
    #[serde(rename = "type")]
    detail_type: Option<String>,
    /// Base64-encoded image data when `detail_type` is `"reasoning.encrypted"`.
    data: Option<String>,
}

#[derive(Debug, Serialize)]
struct NativeChatRequest {
    model: String,
    messages: Vec<NativeMessage>,
    temperature: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<NativeToolSpec>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    modalities: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
struct NativeMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<MessageContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<NativeToolCall>>,
    /// Raw reasoning content from thinking models; pass-through for providers
    /// that require it in assistant tool-call history messages.
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_content: Option<String>,
}

#[derive(Debug, Serialize)]
struct NativeToolSpec {
    #[serde(rename = "type")]
    kind: String,
    function: NativeToolFunctionSpec,
}

#[derive(Debug, Serialize)]
struct NativeToolFunctionSpec {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
struct NativeToolCall {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    kind: Option<String>,
    function: NativeFunctionCall,
}

#[derive(Debug, Serialize, Deserialize)]
struct NativeFunctionCall {
    name: String,
    arguments: String,
}

#[derive(Debug, Deserialize)]
struct NativeChatResponse {
    choices: Vec<NativeChoice>,
    #[serde(default)]
    usage: Option<UsageInfo>,
}

#[derive(Debug, Deserialize)]
struct UsageInfo {
    #[serde(default)]
    prompt_tokens: Option<u64>,
    #[serde(default)]
    completion_tokens: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct NativeChoice {
    message: NativeResponseMessage,
}

#[derive(Debug, Deserialize)]
struct NativeResponseMessage {
    #[serde(default)]
    content: Option<String>,
    /// Reasoning/thinking models may return output in `reasoning_content`.
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<NativeToolCall>>,
    #[serde(default)]
    reasoning_details: Option<Vec<ReasoningDetail>>,
    /// Image generation models return generated images here as data URIs.
    #[serde(default)]
    images: Option<Vec<ImageResponse>>,
}

impl OpenRouterProvider {
    pub fn new(credential: Option<&str>, timeout_secs: Option<u64>) -> Self {
        Self {
            credential: credential.map(ToString::to_string),
            timeout_secs: timeout_secs
                .filter(|secs| *secs > 0)
                .unwrap_or(DEFAULT_OPENROUTER_TIMEOUT_SECS),
            max_tokens: None,
            modalities: None,
            workspace_dir: None,
        }
    }

    /// Set output modalities (e.g. `["image"]` for image generation models).
    pub fn with_modalities(mut self, modalities: Vec<String>) -> Self {
        if modalities.is_empty() {
            self.modalities = None;
        } else {
            self.modalities = Some(modalities);
        }
        self
    }

    /// Set workspace directory for saving generated images.
    pub fn with_workspace_dir(mut self, dir: PathBuf) -> Self {
        self.workspace_dir = Some(dir);
        self
    }

    /// Override the HTTP request timeout for LLM API calls.
    pub fn with_timeout_secs(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    /// Set the maximum output tokens for API requests.
    pub fn with_max_tokens(mut self, max_tokens: Option<u32>) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    fn convert_tools(tools: Option<&[ToolSpec]>) -> Option<Vec<NativeToolSpec>> {
        let items = tools?;
        if items.is_empty() {
            return None;
        }
        let valid: Vec<NativeToolSpec> = items
            .iter()
            .filter(|tool| is_valid_openai_tool_name(&tool.name))
            .map(|tool| NativeToolSpec {
                kind: "function".to_string(),
                function: NativeToolFunctionSpec {
                    name: tool.name.clone(),
                    description: tool.description.clone(),
                    parameters: tool.parameters.clone(),
                },
            })
            .collect();
        if valid.is_empty() { None } else { Some(valid) }
    }

    fn convert_messages(messages: &[ChatMessage]) -> Vec<NativeMessage> {
        messages
            .iter()
            .map(|m| {
                if m.role == "assistant" {
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&m.content) {
                        if let Some(tool_calls_value) = value.get("tool_calls") {
                            if let Ok(parsed_calls) =
                                serde_json::from_value::<Vec<ProviderToolCall>>(
                                    tool_calls_value.clone(),
                                )
                            {
                                let tool_calls = parsed_calls
                                    .into_iter()
                                    .map(|tc| NativeToolCall {
                                        id: Some(tc.id),
                                        kind: Some("function".to_string()),
                                        function: NativeFunctionCall {
                                            name: tc.name,
                                            arguments: tc.arguments,
                                        },
                                    })
                                    .collect::<Vec<_>>();
                                let content = value
                                    .get("content")
                                    .and_then(serde_json::Value::as_str)
                                    .map(|value| MessageContent::Text(value.to_string()));
                                let reasoning_content = value
                                    .get("reasoning_content")
                                    .and_then(serde_json::Value::as_str)
                                    .map(ToString::to_string);
                                return NativeMessage {
                                    role: "assistant".to_string(),
                                    content,
                                    tool_call_id: None,
                                    tool_calls: Some(tool_calls),
                                    reasoning_content,
                                };
                            }
                        }
                    }
                }

                if m.role == "tool" {
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&m.content) {
                        let tool_call_id = value
                            .get("tool_call_id")
                            .and_then(serde_json::Value::as_str)
                            .map(ToString::to_string);
                        let content = value
                            .get("content")
                            .and_then(serde_json::Value::as_str)
                            .map(|value| MessageContent::Text(value.to_string()))
                            .or_else(|| Some(MessageContent::Text(m.content.clone())));
                        return NativeMessage {
                            role: "tool".to_string(),
                            content,
                            tool_call_id,
                            tool_calls: None,
                            reasoning_content: None,
                        };
                    }
                }

                NativeMessage {
                    role: m.role.clone(),
                    content: Some(Self::to_message_content(&m.role, &m.content)),
                    tool_call_id: None,
                    tool_calls: None,
                    reasoning_content: None,
                }
            })
            .collect()
    }

    /// Extract image bytes from the response. Checks the `images` field first
    /// (where OpenRouter returns generated images as data URIs), then falls back
    /// to `reasoning_details` for older response formats.
    fn extract_image_data(
        images: &Option<Vec<ImageResponse>>,
        reasoning_details: &Option<Vec<ReasoningDetail>>,
    ) -> Option<Vec<u8>> {
        // Primary: check `images` field (data URI format)
        if let Some(images) = images.as_ref() {
            for img in images {
                if let Some(ref url_obj) = img.image_url {
                    if let Some(ref url) = url_obj.url {
                        // Parse data URI: "data:image/jpeg;base64,/9j/4AAQ..."
                        if let Some(comma_idx) = url.find(',') {
                            let payload = &url[comma_idx + 1..];
                            if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(payload) {
                                if !bytes.is_empty() {
                                    return Some(bytes);
                                }
                            }
                        }
                    }
                }
            }
        }

        // Fallback: check reasoning_details
        if let Some(details) = reasoning_details.as_ref() {
            for detail in details {
                if detail.detail_type.as_deref() == Some("reasoning.encrypted") {
                    if let Some(ref data) = detail.data {
                        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data) {
                            if !bytes.is_empty() {
                                return Some(bytes);
                            }
                        }
                    }
                }
            }
        }

        None
    }

    /// Save image bytes to `{workspace}/media/{name}.png` and return the path.
    /// Creates the `media/` directory if it doesn't exist.
    fn save_image_to_workspace(workspace: &Path, bytes: &[u8]) -> std::io::Result<PathBuf> {
        let media_dir = workspace.join("media");
        std::fs::create_dir_all(&media_dir)?;
        // Detect format from magic bytes to use correct extension
        let ext = if bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF {
            "jpg"
        } else if bytes.len() >= 8 && bytes[0] == 0x89 && bytes[1] == b'P' && bytes[2] == b'N' && bytes[3] == b'G' {
            "png"
        } else if bytes.len() >= 4 && bytes[0] == b'R' && bytes[1] == b'I' && bytes[2] == b'F' && bytes[3] == b'F' {
            "webp"
        } else {
            "png" // fallback
        };
        let filename = format!("generated-{}.{}", uuid::Uuid::new_v4(), ext);
        let path = media_dir.join(&filename);
        std::fs::write(&path, bytes)?;
        Ok(path)
    }

    /// If the response contains image data and we have a workspace, save it
    /// to disk and return a text description. Otherwise return the text content.
    fn process_response_with_image_save(
        &self,
        content: Option<String>,
        images: &Option<Vec<ImageResponse>>,
        reasoning_details: &Option<Vec<ReasoningDetail>>,
    ) -> String {
        // Try to extract image data from images field (primary) or reasoning_details (fallback)
        if let Some(ref workspace) = self.workspace_dir {
            if let Some(image_bytes) = Self::extract_image_data(images, reasoning_details) {
                match Self::save_image_to_workspace(workspace, &image_bytes) {
                    Ok(path) => {
                        let text_part = content
                            .as_deref()
                            .filter(|s| !s.is_empty())
                            .map(|s| format!("\n\n{s}"))
                            .unwrap_or_default();
                        return format!(
                            "Image saved to: {}{}",
                            path.display(),
                            text_part
                        );
                    }
                    Err(e) => {
                        tracing::warn!("Failed to save generated image: {e}");
                    }
                }
            }
        }

        // Fallback: return text content as-is
        content.unwrap_or_default()
    }

    fn to_message_content(role: &str, content: &str) -> MessageContent {
        if role != "user" {
            return MessageContent::Text(content.to_string());
        }

        let (cleaned_text, image_refs) = multimodal::parse_image_markers(content);
        let (cleaned_text, audio_refs) = multimodal::parse_audio_markers(&cleaned_text);

        if image_refs.is_empty() && audio_refs.is_empty() {
            return MessageContent::Text(content.to_string());
        }

        let mut parts = Vec::with_capacity(image_refs.len() + audio_refs.len() + 1);
        let trimmed_text = cleaned_text.trim();
        if !trimmed_text.is_empty() {
            parts.push(MessagePart::Text {
                text: trimmed_text.to_string(),
            });
        }

        for image_ref in image_refs {
            parts.push(MessagePart::ImageUrl {
                image_url: ImageUrlPart { url: image_ref },
            });
        }

        // Audio markers are embedded as [AUDIO:base64data|format] by
        // prepare_messages_for_provider. Parse the pipe-separated payload.
        for audio_ref in audio_refs {
            if let Some((data, format)) = audio_ref.split_once('|') {
                parts.push(MessagePart::InputAudio {
                    input_audio: InputAudioPart {
                        data: data.to_string(),
                        format: format.to_string(),
                    },
                });
            }
        }

        MessageContent::Parts(parts)
    }

    fn parse_native_response(message: NativeResponseMessage) -> ProviderChatResponse {
        let reasoning_content = message.reasoning_content.clone();
        let tool_calls = message
            .tool_calls
            .unwrap_or_default()
            .into_iter()
            .map(|tc| ProviderToolCall {
                id: tc.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                name: tc.function.name,
                arguments: tc.function.arguments,
            })
            .collect::<Vec<_>>();

        ProviderChatResponse {
            text: message.content,
            tool_calls,
            usage: None,
            reasoning_content,
        }
    }

    fn compact_sanitized_body_snippet(body: &str) -> String {
        super::sanitize_api_error(body)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    async fn read_response_body(
        provider_name: &str,
        response: reqwest::Response,
    ) -> anyhow::Result<String> {
        response.text().await.map_err(|error| {
            let sanitized = super::sanitize_api_error(&error.to_string());
            anyhow::anyhow!(
                "{provider_name} transport error while reading response body: {sanitized}"
            )
        })
    }

    fn parse_response_body<T: DeserializeOwned>(
        provider_name: &str,
        body: &str,
        kind: &str,
    ) -> anyhow::Result<T> {
        serde_json::from_str::<T>(body).map_err(|error| {
            let snippet = Self::compact_sanitized_body_snippet(body);
            anyhow::anyhow!(
                "{provider_name} API returned an unexpected {kind} payload: {error}; body={snippet}"
            )
        })
    }

    fn http_client(&self) -> Client {
        crate::config::build_runtime_proxy_client_with_timeouts(
            "provider.openrouter",
            self.timeout_secs,
            OPENROUTER_CONNECT_TIMEOUT_SECS,
        )
    }
}

#[async_trait]
impl Provider for OpenRouterProvider {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            native_tool_calling: true,
            vision: true,
            prompt_caching: false,
        }
    }

    async fn warmup(&self) -> anyhow::Result<()> {
        // Hit a lightweight endpoint to establish TLS + HTTP/2 connection pool.
        // This prevents the first real chat request from timing out on cold start.
        if let Some(credential) = self.credential.as_ref() {
            self.http_client()
                .get("https://openrouter.ai/api/v1/auth/key")
                .header("Authorization", format!("Bearer {credential}"))
                .send()
                .await?
                .error_for_status()?;
        }
        Ok(())
    }

    async fn chat_with_system(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<String> {
        let credential = self.credential.as_ref()
            .ok_or_else(|| anyhow::anyhow!("OpenRouter API key not set. Run `zeroclaw onboard` or set OPENROUTER_API_KEY env var."))?;

        let mut messages = Vec::new();

        if let Some(sys) = system_prompt {
            messages.push(Message {
                role: "system".to_string(),
                content: MessageContent::Text(sys.to_string()),
            });
        }

        messages.push(Message {
            role: "user".to_string(),
            content: Self::to_message_content("user", message),
        });

        let request = ChatRequest {
            model: model.to_string(),
            messages,
            temperature,
            max_tokens: self.max_tokens,
            modalities: self.modalities.clone(),
        };

        let response = self
            .http_client()
            .post("https://openrouter.ai/api/v1/chat/completions")
            .header("Authorization", format!("Bearer {credential}"))
            .header("HTTP-Referer", "https://github.com/zeroclaw-labs/zeroclaw")
            .header("X-Title", "ZeroClaw")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(super::api_error("OpenRouter", response).await);
        }

        let body = Self::read_response_body("OpenRouter", response).await?;
        let chat_response =
            Self::parse_response_body::<ApiChatResponse>("OpenRouter", &body, "chat-completions")?;

        chat_response
            .choices
            .into_iter()
            .next()
            .map(|c| {
                self.process_response_with_image_save(
                    c.message.content,
                    &c.message.images,
                    &c.message.reasoning_details,
                )
            })
            .ok_or_else(|| anyhow::anyhow!("No response from OpenRouter"))
    }

    async fn chat_with_history(
        &self,
        messages: &[ChatMessage],
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<String> {
        let credential = self.credential.as_ref()
            .ok_or_else(|| anyhow::anyhow!("OpenRouter API key not set. Run `zeroclaw onboard` or set OPENROUTER_API_KEY env var."))?;

        let api_messages: Vec<Message> = messages
            .iter()
            .map(|m| Message {
                role: m.role.clone(),
                content: Self::to_message_content(&m.role, &m.content),
            })
            .collect();

        let request = ChatRequest {
            model: model.to_string(),
            messages: api_messages,
            temperature,
            max_tokens: self.max_tokens,
            modalities: self.modalities.clone(),
        };

        let response = self
            .http_client()
            .post("https://openrouter.ai/api/v1/chat/completions")
            .header("Authorization", format!("Bearer {credential}"))
            .header("HTTP-Referer", "https://github.com/zeroclaw-labs/zeroclaw")
            .header("X-Title", "ZeroClaw")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(super::api_error("OpenRouter", response).await);
        }

        let body = Self::read_response_body("OpenRouter", response).await?;
        let chat_response =
            Self::parse_response_body::<ApiChatResponse>("OpenRouter", &body, "chat-completions")?;

        chat_response
            .choices
            .into_iter()
            .next()
            .map(|c| {
                self.process_response_with_image_save(
                    c.message.content,
                    &c.message.images,
                    &c.message.reasoning_details,
                )
            })
            .ok_or_else(|| anyhow::anyhow!("No response from OpenRouter"))
    }

    async fn chat(
        &self,
        request: ProviderChatRequest<'_>,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ProviderChatResponse> {
        let credential = self.credential.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
            "OpenRouter API key not set. Run `zeroclaw onboard` or set OPENROUTER_API_KEY env var."
        )
        })?;

        let tools = Self::convert_tools(request.tools);
        let native_request = NativeChatRequest {
            model: model.to_string(),
            messages: Self::convert_messages(request.messages),
            temperature,
            tool_choice: tools.as_ref().map(|_| "auto".to_string()),
            tools,
            max_tokens: self.max_tokens,
            modalities: self.modalities.clone(),
        };

        let response = self
            .http_client()
            .post("https://openrouter.ai/api/v1/chat/completions")
            .header("Authorization", format!("Bearer {credential}"))
            .header("HTTP-Referer", "https://github.com/zeroclaw-labs/zeroclaw")
            .header("X-Title", "ZeroClaw")
            .json(&native_request)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(super::api_error("OpenRouter", response).await);
        }

        let body = Self::read_response_body("OpenRouter", response).await?;
        let native_response =
            Self::parse_response_body::<NativeChatResponse>("OpenRouter", &body, "native chat")?;
        let usage = native_response.usage.map(|u| TokenUsage {
            input_tokens: u.prompt_tokens,
            output_tokens: u.completion_tokens,
            cached_input_tokens: None,
        });
        let native_choice = native_response
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No response from OpenRouter"))?;
        let images = native_choice.message.images.clone();
        let reasoning_details = native_choice.message.reasoning_details.clone();
        let mut result = Self::parse_native_response(native_choice.message);
        // Save image data to disk if present.
        if result.text.as_deref().map_or(true, str::is_empty) {
            if let Some(ref workspace) = self.workspace_dir {
                if let Some(image_bytes) = Self::extract_image_data(&images, &reasoning_details) {
                    match Self::save_image_to_workspace(workspace, &image_bytes) {
                        Ok(path) => {
                            let text_part = result.text.as_deref()
                                .filter(|s| !s.is_empty())
                                .map(|s| format!("\n\n{s}"))
                                .unwrap_or_default();
                            result.text = Some(format!("Image saved to: {}{}", path.display(), text_part));
                        }
                        Err(e) => tracing::warn!("Failed to save generated image: {e}"),
                    }
                }
            }
        }
        result.usage = usage;
        Ok(result)
    }

    fn supports_native_tools(&self) -> bool {
        true
    }

    async fn chat_with_tools(
        &self,
        messages: &[ChatMessage],
        tools: &[serde_json::Value],
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ProviderChatResponse> {
        let credential = self.credential.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "OpenRouter API key not set. Run `zeroclaw onboard` or set OPENROUTER_API_KEY env var."
            )
        })?;

        // Convert tool JSON values to NativeToolSpec
        let native_tools: Option<Vec<NativeToolSpec>> = if tools.is_empty() {
            None
        } else {
            let specs: Vec<NativeToolSpec> = tools
                .iter()
                .filter_map(|t| {
                    let func = t.get("function")?;
                    Some(NativeToolSpec {
                        kind: "function".to_string(),
                        function: NativeToolFunctionSpec {
                            name: func.get("name")?.as_str()?.to_string(),
                            description: func
                                .get("description")
                                .and_then(|d| d.as_str())
                                .unwrap_or("")
                                .to_string(),
                            parameters: func
                                .get("parameters")
                                .cloned()
                                .unwrap_or(serde_json::json!({})),
                        },
                    })
                })
                .collect();
            if specs.is_empty() { None } else { Some(specs) }
        };

        // Convert ChatMessage to NativeMessage, preserving structured assistant/tool entries
        // when history contains native tool-call metadata.
        let native_messages = Self::convert_messages(messages);

        let native_request = NativeChatRequest {
            model: model.to_string(),
            messages: native_messages,
            temperature,
            tool_choice: native_tools.as_ref().map(|_| "auto".to_string()),
            tools: native_tools,
            max_tokens: self.max_tokens,
            modalities: self.modalities.clone(),
        };

        let response = self
            .http_client()
            .post("https://openrouter.ai/api/v1/chat/completions")
            .header("Authorization", format!("Bearer {credential}"))
            .header("HTTP-Referer", "https://github.com/zeroclaw-labs/zeroclaw")
            .header("X-Title", "ZeroClaw")
            .json(&native_request)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(super::api_error("OpenRouter", response).await);
        }

        let body = Self::read_response_body("OpenRouter", response).await?;
        let native_response =
            Self::parse_response_body::<NativeChatResponse>("OpenRouter", &body, "native chat")?;
        let usage = native_response.usage.map(|u| TokenUsage {
            input_tokens: u.prompt_tokens,
            output_tokens: u.completion_tokens,
            cached_input_tokens: None,
        });
        let native_choice = native_response
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No response from OpenRouter"))?;
        let images = native_choice.message.images.clone();
        let reasoning_details = native_choice.message.reasoning_details.clone();
        let mut result = Self::parse_native_response(native_choice.message);
        // Save image data to disk if present.
        if result.text.as_deref().map_or(true, str::is_empty) {
            if let Some(ref workspace) = self.workspace_dir {
                if let Some(image_bytes) = Self::extract_image_data(&images, &reasoning_details) {
                    match Self::save_image_to_workspace(workspace, &image_bytes) {
                        Ok(path) => {
                            let text_part = result.text.as_deref()
                                .filter(|s| !s.is_empty())
                                .map(|s| format!("\n\n{s}"))
                                .unwrap_or_default();
                            result.text = Some(format!("Image saved to: {}{}", path.display(), text_part));
                        }
                        Err(e) => tracing::warn!("Failed to save generated image: {e}"),
                    }
                }
            }
        }
        result.usage = usage;
        Ok(result)
    }
}

/// Check if a tool name is valid for OpenAI-compatible APIs.
/// Must match `^[a-zA-Z0-9_-]{1,64}$`.
fn is_valid_openai_tool_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::traits::{ChatMessage, Provider};

    #[test]
    fn capabilities_report_vision_support() {
        let provider = OpenRouterProvider::new(Some("openrouter-test-credential"), None);
        let caps = <OpenRouterProvider as Provider>::capabilities(&provider);
        assert!(caps.native_tool_calling);
        assert!(caps.vision);
    }

    #[test]
    fn creates_with_key() {
        let provider = OpenRouterProvider::new(Some("openrouter-test-credential"), None);
        assert_eq!(
            provider.credential.as_deref(),
            Some("openrouter-test-credential")
        );
    }

    #[test]
    fn creates_without_key() {
        let provider = OpenRouterProvider::new(None, None);
        assert!(provider.credential.is_none());
    }

    #[test]
    fn uses_configured_timeout_when_provided() {
        let provider = OpenRouterProvider::new(Some("openrouter-test-credential"), Some(1200));
        assert_eq!(provider.timeout_secs, 1200);
    }

    #[test]
    fn falls_back_to_default_timeout_for_zero() {
        let provider = OpenRouterProvider::new(Some("openrouter-test-credential"), Some(0));
        assert_eq!(provider.timeout_secs, DEFAULT_OPENROUTER_TIMEOUT_SECS);
    }

    #[tokio::test]
    async fn warmup_without_key_is_noop() {
        let provider = OpenRouterProvider::new(None, None);
        let result = provider.warmup().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn chat_with_system_fails_without_key() {
        let provider = OpenRouterProvider::new(None, None);
        let result = provider
            .chat_with_system(Some("system"), "hello", "openai/gpt-4o", 0.2)
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("API key not set"));
    }

    #[tokio::test]
    async fn chat_with_history_fails_without_key() {
        let provider = OpenRouterProvider::new(None, None);
        let messages = vec![
            ChatMessage {
                role: "system".into(),
                content: "be concise".into(),
            },
            ChatMessage {
                role: "user".into(),
                content: "hello".into(),
            },
        ];

        let result = provider
            .chat_with_history(&messages, "anthropic/claude-sonnet-4", 0.7)
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("API key not set"));
    }

    #[test]
    fn chat_request_serializes_with_system_and_user() {
        let request = ChatRequest {
            model: "anthropic/claude-sonnet-4".into(),
            messages: vec![
                Message {
                    role: "system".into(),
                    content: MessageContent::Text("You are helpful".into()),
                },
                Message {
                    role: "user".into(),
                    content: MessageContent::Text("Summarize this".into()),
                },
            ],
            temperature: 0.5,
            max_tokens: None,
            modalities: None,
        };

        let json = serde_json::to_string(&request).unwrap();

        assert!(json.contains("anthropic/claude-sonnet-4"));
        assert!(json.contains("\"role\":\"system\""));
        assert!(json.contains("\"role\":\"user\""));
        assert!(json.contains("\"temperature\":0.5"));
    }

    #[test]
    fn chat_request_serializes_history_messages() {
        let messages = [
            ChatMessage {
                role: "assistant".into(),
                content: "Previous answer".into(),
            },
            ChatMessage {
                role: "user".into(),
                content: "Follow-up".into(),
            },
        ];

        let request = ChatRequest {
            model: "google/gemini-2.5-pro".into(),
            messages: messages
                .iter()
                .map(|msg| Message {
                    role: msg.role.clone(),
                    content: MessageContent::Text(msg.content.clone()),
                })
                .collect(),
            temperature: 0.0,
            max_tokens: None,
            modalities: None,
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"role\":\"assistant\""));
        assert!(json.contains("\"role\":\"user\""));
        assert!(json.contains("google/gemini-2.5-pro"));
    }

    #[test]
    fn response_deserializes_single_choice() {
        let json = r#"{"choices":[{"message":{"content":"Hi from OpenRouter"}}]}"#;

        let response: ApiChatResponse = serde_json::from_str(json).unwrap();

        assert_eq!(response.choices.len(), 1);
        assert_eq!(response.choices[0].message.content.as_deref(), Some("Hi from OpenRouter"));
    }

    #[test]
    fn response_deserializes_empty_choices() {
        let json = r#"{"choices":[]}"#;

        let response: ApiChatResponse = serde_json::from_str(json).unwrap();

        assert!(response.choices.is_empty());
    }

    #[test]
    fn parse_chat_response_body_reports_sanitized_snippet() {
        let body = r#"{"choices":"invalid","api_key":"sk-test-secret-value"}"#;
        let err = OpenRouterProvider::parse_response_body::<ApiChatResponse>(
            "OpenRouter",
            body,
            "chat-completions",
        )
        .expect_err("payload should fail");
        let msg = err.to_string();

        assert!(msg.contains("OpenRouter API returned an unexpected chat-completions payload"));
        assert!(msg.contains("body="));
        assert!(msg.contains("[REDACTED]"));
        assert!(!msg.contains("sk-test-secret-value"));
    }

    #[test]
    fn parse_native_response_body_reports_sanitized_snippet() {
        let body = r#"{"choices":123,"api_key":"sk-another-secret"}"#;
        let err = OpenRouterProvider::parse_response_body::<NativeChatResponse>(
            "OpenRouter",
            body,
            "native chat",
        )
        .expect_err("payload should fail");
        let msg = err.to_string();

        assert!(msg.contains("OpenRouter API returned an unexpected native chat payload"));
        assert!(msg.contains("body="));
        assert!(msg.contains("[REDACTED]"));
        assert!(!msg.contains("sk-another-secret"));
    }

    #[tokio::test]
    async fn chat_with_tools_fails_without_key() {
        let provider = OpenRouterProvider::new(None, None);
        let messages = vec![ChatMessage {
            role: "user".into(),
            content: "What is the date?".into(),
        }];
        let tools = vec![serde_json::json!({
            "type": "function",
            "function": {
                "name": "shell",
                "description": "Run a shell command",
                "parameters": {"type": "object", "properties": {"command": {"type": "string"}}}
            }
        })];

        let result = provider
            .chat_with_tools(&messages, &tools, "deepseek/deepseek-chat", 0.5)
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("API key not set"));
    }

    #[test]
    fn native_response_deserializes_with_tool_calls() {
        let json = r#"{
            "choices":[{
                "message":{
                    "content":null,
                    "tool_calls":[
                        {"id":"call_123","type":"function","function":{"name":"get_price","arguments":"{\"symbol\":\"BTC\"}"}}
                    ]
                }
            }]
        }"#;

        let response: NativeChatResponse = serde_json::from_str(json).unwrap();

        assert_eq!(response.choices.len(), 1);
        let message = &response.choices[0].message;
        assert!(message.content.is_none());
        let tool_calls = message.tool_calls.as_ref().unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id.as_deref(), Some("call_123"));
        assert_eq!(tool_calls[0].function.name, "get_price");
        assert_eq!(tool_calls[0].function.arguments, "{\"symbol\":\"BTC\"}");
    }

    #[test]
    fn native_response_deserializes_with_text_and_tool_calls() {
        let json = r#"{
            "choices":[{
                "message":{
                    "content":"I'll get that for you.",
                    "tool_calls":[
                        {"id":"call_456","type":"function","function":{"name":"shell","arguments":"{\"command\":\"date\"}"}}
                    ]
                }
            }]
        }"#;

        let response: NativeChatResponse = serde_json::from_str(json).unwrap();

        assert_eq!(response.choices.len(), 1);
        let message = &response.choices[0].message;
        assert_eq!(message.content.as_deref(), Some("I'll get that for you."));
        let tool_calls = message.tool_calls.as_ref().unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].function.name, "shell");
    }

    #[test]
    fn parse_native_response_converts_to_chat_response() {
        let message = NativeResponseMessage {
            content: Some("Here you go.".into()),
            reasoning_content: None,
            reasoning_details: None,
            images: None,
            tool_calls: Some(vec![NativeToolCall {
                id: Some("call_789".into()),
                kind: Some("function".into()),
                function: NativeFunctionCall {
                    name: "file_read".into(),
                    arguments: r#"{"path":"test.txt"}"#.into(),
                },
            }]),
        };

        let response = OpenRouterProvider::parse_native_response(message);

        assert_eq!(response.text.as_deref(), Some("Here you go."));
        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].id, "call_789");
        assert_eq!(response.tool_calls[0].name, "file_read");
    }

    #[test]
    fn convert_messages_parses_assistant_tool_call_payload() {
        let messages = vec![ChatMessage {
            role: "assistant".into(),
            content: r#"{"content":"Using tool","tool_calls":[{"id":"call_abc","name":"shell","arguments":"{\"command\":\"pwd\"}"}]}"#
                .into(),
        }];

        let converted = OpenRouterProvider::convert_messages(&messages);
        assert_eq!(converted.len(), 1);
        assert_eq!(converted[0].role, "assistant");
        assert_eq!(
            converted[0]
                .content
                .as_ref()
                .and_then(|content| match content {
                    MessageContent::Text(value) => Some(value.as_str()),
                    MessageContent::Parts(_) => None,
                }),
            Some("Using tool")
        );

        let tool_calls = converted[0].tool_calls.as_ref().unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id.as_deref(), Some("call_abc"));
        assert_eq!(tool_calls[0].function.name, "shell");
        assert_eq!(tool_calls[0].function.arguments, r#"{"command":"pwd"}"#);
    }

    #[test]
    fn convert_messages_parses_tool_result_payload() {
        let messages = vec![ChatMessage {
            role: "tool".into(),
            content: r#"{"tool_call_id":"call_xyz","content":"done"}"#.into(),
        }];

        let converted = OpenRouterProvider::convert_messages(&messages);
        assert_eq!(converted.len(), 1);
        assert_eq!(converted[0].role, "tool");
        assert_eq!(converted[0].tool_call_id.as_deref(), Some("call_xyz"));
        assert_eq!(
            converted[0]
                .content
                .as_ref()
                .and_then(|content| match content {
                    MessageContent::Text(value) => Some(value.as_str()),
                    MessageContent::Parts(_) => None,
                }),
            Some("done")
        );
        assert!(converted[0].tool_calls.is_none());
    }

    #[test]
    fn to_message_content_converts_image_markers_to_openai_parts() {
        let content = "Describe this\n\n[IMAGE:data:image/png;base64,abcd]";
        let value =
            serde_json::to_value(OpenRouterProvider::to_message_content("user", content)).unwrap();
        let parts = value
            .as_array()
            .expect("multimodal content should be an array");
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0]["type"], "text");
        assert_eq!(parts[0]["text"], "Describe this");
        assert_eq!(parts[1]["type"], "image_url");
        assert_eq!(parts[1]["image_url"]["url"], "data:image/png;base64,abcd");
    }

    #[test]
    fn to_message_content_converts_audio_markers_to_input_audio() {
        let content = "Transcribe this\n\n[AUDIO:dGVzdA==|webm]";
        let value =
            serde_json::to_value(OpenRouterProvider::to_message_content("user", content)).unwrap();
        let parts = value.as_array().expect("multimodal content should be an array");
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0]["type"], "text");
        assert_eq!(parts[0]["text"], "Transcribe this");
        assert_eq!(parts[1]["type"], "input_audio");
        assert_eq!(parts[1]["input_audio"]["data"], "dGVzdA==");
        assert_eq!(parts[1]["input_audio"]["format"], "webm");
    }

    #[test]
    fn to_message_content_mixed_image_and_audio() {
        let content = "[IMAGE:data:image/png;base64,img]\n\n[AUDIO:aud|mp3]\n\nDescribe both";
        let value =
            serde_json::to_value(OpenRouterProvider::to_message_content("user", content)).unwrap();
        let parts = value.as_array().expect("multimodal content should be an array");
        assert_eq!(parts.len(), 3);
        // Text part
        assert_eq!(parts[0]["type"], "text");
        assert!(parts[0]["text"].as_str().unwrap().contains("Describe both"));
        // Image part
        assert_eq!(parts[1]["type"], "image_url");
        // Audio part
        assert_eq!(parts[2]["type"], "input_audio");
        assert_eq!(parts[2]["input_audio"]["format"], "mp3");
    }

    #[test]
    fn to_message_content_audio_only_no_text() {
        let content = "[AUDIO:dGVzdA==|wav]";
        let value =
            serde_json::to_value(OpenRouterProvider::to_message_content("user", content)).unwrap();
        let parts = value.as_array().expect("should be array");
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0]["type"], "input_audio");
    }

    #[test]
    fn to_message_content_ignores_audio_in_non_user_role() {
        let content = "[AUDIO:dGVzdA==|wav]";
        let value =
            serde_json::to_value(OpenRouterProvider::to_message_content("assistant", content))
                .unwrap();
        // Non-user messages are plain text, not parsed
        assert!(value.is_string());
    }

    #[test]
    fn input_audio_serializes_correctly() {
        let part = MessagePart::InputAudio {
            input_audio: InputAudioPart {
                data: "abc123".into(),
                format: "mp3".into(),
            },
        };
        let json = serde_json::to_value(&part).unwrap();
        assert_eq!(json["type"], "input_audio");
        assert_eq!(json["input_audio"]["data"], "abc123");
        assert_eq!(json["input_audio"]["format"], "mp3");
    }

    #[test]
    fn native_response_parses_usage() {
        let json = r#"{
            "choices": [{"message": {"content": "Hello"}}],
            "usage": {"prompt_tokens": 42, "completion_tokens": 15}
        }"#;
        let resp: NativeChatResponse = serde_json::from_str(json).unwrap();
        let usage = resp.usage.unwrap();
        assert_eq!(usage.prompt_tokens, Some(42));
        assert_eq!(usage.completion_tokens, Some(15));
    }

    #[test]
    fn native_response_parses_without_usage() {
        let json = r#"{"choices": [{"message": {"content": "Hello"}}]}"#;
        let resp: NativeChatResponse = serde_json::from_str(json).unwrap();
        assert!(resp.usage.is_none());
    }

    // ═══════════════════════════════════════════════════════════════════════
    // reasoning_content pass-through tests
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn parse_native_response_captures_reasoning_content() {
        let message = NativeResponseMessage {
            content: Some("answer".into()),
            reasoning_content: Some("thinking step".into()),
            reasoning_details: None,
            images: None,
            tool_calls: Some(vec![NativeToolCall {
                id: Some("call_1".into()),
                kind: Some("function".into()),
                function: NativeFunctionCall {
                    name: "shell".into(),
                    arguments: "{}".into(),
                },
            }]),
        };
        let parsed = OpenRouterProvider::parse_native_response(message);
        assert_eq!(parsed.reasoning_content.as_deref(), Some("thinking step"));
        assert_eq!(parsed.tool_calls.len(), 1);
    }

    #[test]
    fn parse_native_response_none_reasoning_content_for_normal_model() {
        let message = NativeResponseMessage {
            content: Some("hello".into()),
            reasoning_content: None,
            reasoning_details: None,
            images: None,
            tool_calls: None,
        };
        let parsed = OpenRouterProvider::parse_native_response(message);
        assert!(parsed.reasoning_content.is_none());
    }

    #[test]
    fn native_response_deserializes_reasoning_content() {
        let json = r#"{
            "choices":[{
                "message":{
                    "content":"answer",
                    "reasoning_content":"deep thought",
                    "tool_calls":[
                        {"id":"call_r1","type":"function","function":{"name":"shell","arguments":"{}"}}
                    ]
                }
            }]
        }"#;
        let resp: NativeChatResponse = serde_json::from_str(json).unwrap();
        let message = &resp.choices[0].message;
        assert_eq!(message.reasoning_content.as_deref(), Some("deep thought"));
    }

    #[test]
    fn convert_messages_round_trips_reasoning_content() {
        let history_json = serde_json::json!({
            "content": "I will check",
            "tool_calls": [{
                "id": "tc_1",
                "name": "shell",
                "arguments": "{}"
            }],
            "reasoning_content": "Let me think..."
        });

        let messages = vec![ChatMessage {
            role: "assistant".into(),
            content: history_json.to_string(),
        }];
        let native = OpenRouterProvider::convert_messages(&messages);
        assert_eq!(native.len(), 1);
        assert_eq!(
            native[0].reasoning_content.as_deref(),
            Some("Let me think...")
        );
    }

    #[test]
    fn convert_messages_no_reasoning_content_when_absent() {
        let history_json = serde_json::json!({
            "content": "I will check",
            "tool_calls": [{
                "id": "tc_1",
                "name": "shell",
                "arguments": "{}"
            }]
        });

        let messages = vec![ChatMessage {
            role: "assistant".into(),
            content: history_json.to_string(),
        }];
        let native = OpenRouterProvider::convert_messages(&messages);
        assert_eq!(native.len(), 1);
        assert!(native[0].reasoning_content.is_none());
    }

    #[test]
    fn native_message_omits_reasoning_content_when_none() {
        let msg = NativeMessage {
            role: "assistant".to_string(),
            content: Some(MessageContent::Text("hi".into())),
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(!json.contains("reasoning_content"));
    }

    #[test]
    fn native_message_includes_reasoning_content_when_some() {
        let msg = NativeMessage {
            role: "assistant".to_string(),
            content: Some(MessageContent::Text("hi".into())),
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: Some("thinking...".to_string()),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("reasoning_content"));
        assert!(json.contains("thinking..."));
    }

    // ═══════════════════════════════════════════════════════════════════════
    // timeout_secs configuration tests
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn default_timeout_is_120() {
        let provider = OpenRouterProvider::new(Some("key"), None);
        assert_eq!(provider.timeout_secs, 120);
    }

    #[test]
    fn with_timeout_secs_overrides_default() {
        let provider = OpenRouterProvider::new(Some("key"), None).with_timeout_secs(300);
        assert_eq!(provider.timeout_secs, 300);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // tool name validation tests
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn valid_openai_tool_names() {
        assert!(is_valid_openai_tool_name("shell"));
        assert!(is_valid_openai_tool_name("file_read"));
        assert!(is_valid_openai_tool_name("web-search"));
        assert!(is_valid_openai_tool_name("Tool123"));
        assert!(is_valid_openai_tool_name("a"));
    }

    #[test]
    fn invalid_openai_tool_names() {
        assert!(!is_valid_openai_tool_name(""));
        assert!(!is_valid_openai_tool_name("mcp:server.tool"));
        assert!(!is_valid_openai_tool_name("node.js"));
        assert!(!is_valid_openai_tool_name("tool name"));
        assert!(!is_valid_openai_tool_name(
            "this_tool_name_is_way_too_long_and_exceeds_the_sixty_four_character_limit_xxxxx"
        ));
    }

    #[test]
    fn convert_tools_skips_invalid_names() {
        use crate::tools::ToolSpec;

        let tools = vec![
            ToolSpec {
                name: "valid_tool".into(),
                description: "A valid tool".into(),
                parameters: serde_json::json!({"type": "object"}),
            },
            ToolSpec {
                name: "mcp:server.bad".into(),
                description: "Invalid name".into(),
                parameters: serde_json::json!({"type": "object"}),
            },
            ToolSpec {
                name: "another-valid".into(),
                description: "Also valid".into(),
                parameters: serde_json::json!({"type": "object"}),
            },
        ];

        let result = OpenRouterProvider::convert_tools(Some(&tools)).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].function.name, "valid_tool");
        assert_eq!(result[1].function.name, "another-valid");
    }

    #[test]
    fn convert_tools_returns_none_when_all_invalid() {
        use crate::tools::ToolSpec;

        let tools = vec![ToolSpec {
            name: "mcp:bad.name".into(),
            description: "Invalid".into(),
            parameters: serde_json::json!({"type": "object"}),
        }];

        assert!(OpenRouterProvider::convert_tools(Some(&tools)).is_none());
    }

    // ── Image Generation: Serialization (#1-4) ──────────────────

    #[test]
    fn chat_request_serializes_modalities_when_present() {
        let request = ChatRequest {
            model: "flux".into(),
            messages: vec![],
            temperature: 0.7,
            max_tokens: None,
            modalities: Some(vec!["image".into()]),
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains(r#""modalities":["image"]"#));
    }

    #[test]
    fn chat_request_omits_modalities_when_none() {
        let request = ChatRequest {
            model: "gpt".into(),
            messages: vec![],
            temperature: 0.7,
            max_tokens: None,
            modalities: None,
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(!json.contains("modalities"));
    }

    #[test]
    fn native_chat_request_serializes_modalities() {
        let request = NativeChatRequest {
            model: "flux".into(),
            messages: vec![],
            temperature: 0.7,
            tools: None,
            tool_choice: None,
            max_tokens: None,
            modalities: Some(vec!["image".into(), "text".into()]),
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains(r#""modalities":["image","text"]"#));
    }

    #[test]
    fn native_chat_request_omits_modalities_when_none() {
        let request = NativeChatRequest {
            model: "gpt".into(),
            messages: vec![],
            temperature: 0.7,
            tools: None,
            tool_choice: None,
            max_tokens: None,
            modalities: None,
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(!json.contains("modalities"));
    }

    // ── Image Generation: Response Deserialization (#5-10) ──────

    #[test]
    fn response_deserializes_null_content_with_reasoning_details() {
        let json = r#"{
            "choices": [{
                "message": {
                    "content": null,
                    "reasoning_details": [{"type": "reasoning.encrypted", "data": "aGVsbG8="}]
                }
            }]
        }"#;
        let response: ApiChatResponse = serde_json::from_str(json).unwrap();
        assert!(response.choices[0].message.content.is_none());
        assert!(response.choices[0].message.reasoning_details.is_some());
    }

    #[test]
    fn response_deserializes_text_content_without_reasoning_details() {
        let json = r#"{"choices": [{"message": {"content": "hello"}}]}"#;
        let response: ApiChatResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.choices[0].message.content.as_deref(), Some("hello"));
        assert!(response.choices[0].message.reasoning_details.is_none());
    }

    #[test]
    fn response_deserializes_both_content_and_reasoning_details() {
        let json = r#"{
            "choices": [{
                "message": {
                    "content": "description",
                    "reasoning_details": [{"type": "reasoning.encrypted", "data": "aGVsbG8="}]
                }
            }]
        }"#;
        let response: ApiChatResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.choices[0].message.content.as_deref(), Some("description"));
        assert!(response.choices[0].message.reasoning_details.is_some());
    }

    #[test]
    fn native_response_deserializes_reasoning_details() {
        let json = r#"{
            "choices": [{
                "message": {
                    "content": null,
                    "reasoning_details": [{"type": "reasoning.encrypted", "data": "dGVzdA=="}]
                }
            }]
        }"#;
        let response: NativeChatResponse = serde_json::from_str(json).unwrap();
        let details = response.choices[0].message.reasoning_details.as_ref().unwrap();
        assert_eq!(details[0].detail_type.as_deref(), Some("reasoning.encrypted"));
        assert_eq!(details[0].data.as_deref(), Some("dGVzdA=="));
    }

    #[test]
    fn native_response_defaults_reasoning_details_to_none() {
        let json = r#"{"choices": [{"message": {"content": "hi"}}]}"#;
        let response: NativeChatResponse = serde_json::from_str(json).unwrap();
        assert!(response.choices[0].message.reasoning_details.is_none());
    }

    #[test]
    fn reasoning_detail_parses_type_and_data() {
        let json = r#"{"type": "reasoning.encrypted", "data": "abc123"}"#;
        let detail: ReasoningDetail = serde_json::from_str(json).unwrap();
        assert_eq!(detail.detail_type.as_deref(), Some("reasoning.encrypted"));
        assert_eq!(detail.data.as_deref(), Some("abc123"));
    }

    // ── Image Generation: Extraction (#11-17) ──────────────────

    #[test]
    fn extract_image_data_from_images_field() {
        let jpeg_data = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10]; // JPEG header
        let b64 = base64::engine::general_purpose::STANDARD.encode(&jpeg_data);
        let images = Some(vec![ImageResponse {
            image_url: Some(ImageResponseUrl {
                url: Some(format!("data:image/jpeg;base64,{b64}")),
            }),
        }]);
        let result = OpenRouterProvider::extract_image_data(&images, &None);
        assert_eq!(result.unwrap(), jpeg_data);
    }

    #[test]
    fn extract_image_data_images_field_takes_priority_over_reasoning() {
        let jpeg_data = vec![0xFF, 0xD8, 0xFF];
        let b64 = base64::engine::general_purpose::STANDARD.encode(&jpeg_data);
        let images = Some(vec![ImageResponse {
            image_url: Some(ImageResponseUrl {
                url: Some(format!("data:image/jpeg;base64,{b64}")),
            }),
        }]);
        let details = Some(vec![ReasoningDetail {
            detail_type: Some("reasoning.encrypted".into()),
            data: Some(base64::engine::general_purpose::STANDARD.encode(b"WRONG")),
        }]);
        // images field should win
        let result = OpenRouterProvider::extract_image_data(&images, &details);
        assert_eq!(result.unwrap(), jpeg_data);
    }

    #[test]
    fn extract_image_data_valid_base64() {
        let details = Some(vec![ReasoningDetail {
            detail_type: Some("reasoning.encrypted".into()),
            data: Some(base64::engine::general_purpose::STANDARD.encode(b"PNG_BYTES")),
        }]);
        let result = OpenRouterProvider::extract_image_data(&None, &details);
        assert_eq!(result.unwrap(), b"PNG_BYTES");
    }

    #[test]
    fn extract_image_data_invalid_base64() {
        let details = Some(vec![ReasoningDetail {
            detail_type: Some("reasoning.encrypted".into()),
            data: Some("not!valid!base64!!!".into()),
        }]);
        assert!(OpenRouterProvider::extract_image_data(&None, &details).is_none());
    }

    #[test]
    fn extract_image_data_empty_data_field() {
        let details = Some(vec![ReasoningDetail {
            detail_type: Some("reasoning.encrypted".into()),
            data: Some(String::new()),
        }]);
        assert!(OpenRouterProvider::extract_image_data(&None, &details).is_none());
    }

    #[test]
    fn extract_image_data_wrong_type() {
        let details = Some(vec![ReasoningDetail {
            detail_type: Some("reasoning.text".into()),
            data: Some(base64::engine::general_purpose::STANDARD.encode(b"data")),
        }]);
        assert!(OpenRouterProvider::extract_image_data(&None, &details).is_none());
    }

    #[test]
    fn extract_image_data_empty_vec() {
        let details: Option<Vec<ReasoningDetail>> = Some(vec![]);
        assert!(OpenRouterProvider::extract_image_data(&None, &details).is_none());
    }

    #[test]
    fn extract_image_data_none() {
        assert!(OpenRouterProvider::extract_image_data(&None, &None).is_none());
    }

    #[test]
    fn extract_image_data_multiple_entries_picks_valid() {
        let details = Some(vec![
            ReasoningDetail {
                detail_type: Some("reasoning.text".into()),
                data: Some("ignored".into()),
            },
            ReasoningDetail {
                detail_type: Some("reasoning.encrypted".into()),
                data: Some(base64::engine::general_purpose::STANDARD.encode(b"IMG")),
            },
        ]);
        assert_eq!(OpenRouterProvider::extract_image_data(&None, &details).unwrap(), b"IMG");
    }

    // ── Image Generation: Disk Write (#18-22) ──────────────────

    #[tokio::test]
    async fn save_image_creates_media_dir_and_writes_file() {
        let dir = std::env::temp_dir().join("zeroclaw_test_img_save");
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();

        let bytes = b"fake PNG data";
        let path = OpenRouterProvider::save_image_to_workspace(&dir, bytes).unwrap();

        assert!(path.exists());
        assert!(path.starts_with(dir.join("media")));
        assert!(path.file_name().unwrap().to_str().unwrap().starts_with("generated-"));
        assert!(path.extension().unwrap() == "png");

        let written = std::fs::read(&path).unwrap();
        assert_eq!(written, bytes);

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[test]
    fn save_image_filename_is_uuid_pattern() {
        let dir = std::env::temp_dir().join("zeroclaw_test_img_uuid");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let path = OpenRouterProvider::save_image_to_workspace(&dir, b"data").unwrap();
        let stem = path.file_stem().unwrap().to_str().unwrap();
        assert!(stem.starts_with("generated-"));
        // UUID after prefix should be 36 chars (8-4-4-4-12)
        assert_eq!(stem.len(), "generated-".len() + 36);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_image_succeeds_when_media_dir_exists() {
        let dir = std::env::temp_dir().join("zeroclaw_test_img_existing_media");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("media")).unwrap();

        let result = OpenRouterProvider::save_image_to_workspace(&dir, b"data");
        assert!(result.is_ok());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_image_fails_on_unwritable_path() {
        let result = OpenRouterProvider::save_image_to_workspace(
            Path::new("/nonexistent/impossible/path"),
            b"data",
        );
        assert!(result.is_err());
    }

    // ── Image Generation: Response Processing (#23-28) ─────────

    #[test]
    fn process_response_saves_image_when_workspace_set() {
        let dir = std::env::temp_dir().join("zeroclaw_test_img_process_save");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let provider = OpenRouterProvider::new(None, None)
            .with_workspace_dir(dir.clone());
        let details = Some(vec![ReasoningDetail {
            detail_type: Some("reasoning.encrypted".into()),
            data: Some(base64::engine::general_purpose::STANDARD.encode(b"IMG_DATA")),
        }]);

        let result = provider.process_response_with_image_save(None, &None, &details);
        assert!(result.starts_with("Image saved to:"));
        assert!(result.contains("media/generated-"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn process_response_appends_text_content_after_path() {
        let dir = std::env::temp_dir().join("zeroclaw_test_img_process_text");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let provider = OpenRouterProvider::new(None, None)
            .with_workspace_dir(dir.clone());
        let details = Some(vec![ReasoningDetail {
            detail_type: Some("reasoning.encrypted".into()),
            data: Some(base64::engine::general_purpose::STANDARD.encode(b"IMG")),
        }]);

        let result = provider.process_response_with_image_save(
            Some("A beautiful sunset".into()),
            &None,
            &details,
        );
        assert!(result.starts_with("Image saved to:"));
        assert!(result.contains("A beautiful sunset"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn process_response_returns_text_when_no_image() {
        let provider = OpenRouterProvider::new(None, None)
            .with_workspace_dir(std::env::temp_dir());

        let result = provider.process_response_with_image_save(
            Some("normal text".into()),
            &None,
            &None,
        );
        assert_eq!(result, "normal text");
    }

    #[test]
    fn process_response_returns_empty_when_no_image_and_null_content() {
        let provider = OpenRouterProvider::new(None, None)
            .with_workspace_dir(std::env::temp_dir());

        let result = provider.process_response_with_image_save(None, &None, &None);
        assert_eq!(result, "");
    }

    #[test]
    fn process_response_skips_save_when_no_workspace() {
        let provider = OpenRouterProvider::new(None, None); // no workspace_dir
        let details = Some(vec![ReasoningDetail {
            detail_type: Some("reasoning.encrypted".into()),
            data: Some(base64::engine::general_purpose::STANDARD.encode(b"IMG")),
        }]);

        let result = provider.process_response_with_image_save(None, &None, &details);
        // No workspace → falls back to empty content
        assert_eq!(result, "");
    }

    #[test]
    fn process_response_falls_back_on_save_failure() {
        let provider = OpenRouterProvider::new(None, None)
            .with_workspace_dir(PathBuf::from("/nonexistent/impossible"));
        let details = Some(vec![ReasoningDetail {
            detail_type: Some("reasoning.encrypted".into()),
            data: Some(base64::engine::general_purpose::STANDARD.encode(b"IMG")),
        }]);

        let result = provider.process_response_with_image_save(
            Some("fallback text".into()),
            &None,
            &details,
        );
        // Save failed → falls back to text content
        assert_eq!(result, "fallback text");
    }

    // ── Image Generation: Provider Struct (#29-31) ─────────────

    #[test]
    fn provider_new_has_no_workspace_dir() {
        let provider = OpenRouterProvider::new(None, None);
        assert!(provider.workspace_dir.is_none());
    }

    #[test]
    fn provider_with_workspace_dir_sets_field() {
        let provider = OpenRouterProvider::new(None, None)
            .with_workspace_dir(PathBuf::from("/tmp/ws"));
        assert_eq!(provider.workspace_dir.as_deref(), Some(Path::new("/tmp/ws")));
    }

    #[test]
    fn provider_builder_chain_preserves_all_fields() {
        let provider = OpenRouterProvider::new(Some("key"), Some(300))
            .with_max_tokens(Some(4096))
            .with_modalities(vec!["image".into()])
            .with_workspace_dir(PathBuf::from("/ws"));

        assert_eq!(provider.credential.as_deref(), Some("key"));
        assert_eq!(provider.timeout_secs, 300);
        assert_eq!(provider.max_tokens, Some(4096));
        assert_eq!(provider.modalities, Some(vec!["image".to_string()]));
        assert_eq!(provider.workspace_dir.as_deref(), Some(Path::new("/ws")));
    }

    // ── Image Generation: Provider modalities (#32-33) ─────────

    #[test]
    fn with_modalities_sets_field() {
        let provider = OpenRouterProvider::new(None, None)
            .with_modalities(vec!["image".into()]);
        assert_eq!(provider.modalities, Some(vec!["image".to_string()]));
    }

    #[test]
    fn with_modalities_empty_resets_to_none() {
        let provider = OpenRouterProvider::new(None, None)
            .with_modalities(vec![]);
        assert!(provider.modalities.is_none());
    }

    // ── Image Generation: Backward Compatibility (#42-44) ──────

    #[test]
    fn provider_without_modalities_or_workspace_unchanged() {
        let provider = OpenRouterProvider::new(Some("key"), None);
        assert!(provider.modalities.is_none());
        assert!(provider.workspace_dir.is_none());
        // Existing fields unaffected
        assert_eq!(provider.credential.as_deref(), Some("key"));
        assert_eq!(provider.timeout_secs, DEFAULT_OPENROUTER_TIMEOUT_SECS);
        assert!(provider.max_tokens.is_none());
    }

    #[test]
    fn text_only_response_unaffected_by_image_handling() {
        let provider = OpenRouterProvider::new(None, None)
            .with_workspace_dir(std::env::temp_dir());

        // Normal text response, no images or reasoning_details
        let result = provider.process_response_with_image_save(
            Some("just text".into()),
            &None,
            &None,
        );
        assert_eq!(result, "just text");
    }
}
