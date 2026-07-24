use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct AiConfig {
    pub enabled: Option<bool>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub endpoint: Option<String>,
    #[serde(rename = "maxContextLines")]
    pub max_context_lines: Option<usize>,
    pub forwarding: Option<ForwardingConfig>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct ForwardingConfig {
    pub enabled: Option<bool>,
    #[serde(rename = "webhookUrl")]
    pub webhook_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone)]
pub struct AiResponse {
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
}

pub struct AiClient {
    config: AiConfig,
    api_key: String,
    client: reqwest::Client,
    pub last_response: Mutex<Option<String>>,
}

impl AiClient {
    pub fn new(config: AiConfig, api_key: String) -> Self {
        Self {
            config,
            api_key,
            client: reqwest::Client::new(),
            last_response: Mutex::new(None),
        }
    }

    pub fn endpoint(&self) -> &str {
        self.config
            .endpoint
            .as_deref()
            .unwrap_or("https://api.deepseek.com/v1/chat/completions")
    }

    pub fn model(&self) -> &str {
        self.config.model.as_deref().unwrap_or("deepseek-v4-flash")
    }

    pub fn max_context_lines(&self) -> usize {
        self.config.max_context_lines.unwrap_or(200)
    }

    fn tools() -> Value {
        json!([
            {
                "type": "function",
                "function": {
                    "name": "toggle_service",
                    "description": "Show or hide a service/log filter. Use 'all' to show all, 'none' to hide all, or a service label (like 'Agriculture', 'Logs', 'Weather') to toggle it.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "action": {
                                "type": "string",
                                "description": "The filter action: 'all', 'none', or a specific service label"
                            }
                        },
                        "required": ["action"]
                    }
                }
            },
            {
                "type": "function",
                "function": {
                    "name": "add_route",
                    "description": "Add a new proxy route mapping a prefix to a target URL.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "prefix": {
                                "type": "string",
                                "description": "URL prefix for the route, e.g. /api"
                            },
                            "target": {
                                "type": "string",
                                "description": "Target URL to proxy to, e.g. https://example.com/api"
                            },
                            "label": {
                                "type": "string",
                                "description": "Human-readable label for the service"
                            }
                        },
                        "required": ["prefix", "target", "label"]
                    }
                }
            },
            {
                "type": "function",
                "function": {
                    "name": "remove_route",
                    "description": "Remove a dynamic proxy route by its prefix.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "prefix": {
                                "type": "string",
                                "description": "The route prefix to remove, e.g. /api"
                            }
                        },
                        "required": ["prefix"]
                    }
                }
            },
            {
                "type": "function",
                "function": {
                    "name": "enable_monitoring",
                    "description": "Toggle request monitoring on/off to track transfers.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "enable": {
                                "type": "boolean",
                                "description": "true to enable monitoring, false to disable"
                            }
                        },
                        "required": ["enable"]
                    }
                }
            },
            {
                "type": "function",
                "function": {
                    "name": "forward_observation",
                    "description": "Forward an observation or finding to another agent or service via webhook.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "message": {
                                "type": "string",
                                "description": "The observation message to forward"
                            },
                            "urgency": {
                                "type": "string",
                                "enum": ["low", "medium", "high", "critical"],
                                "description": "Urgency level of the observation"
                            }
                        },
                        "required": ["message"]
                    }
                }
            }
        ])
    }

    fn system_prompt() -> &'static str {
        "You are an observability assistant integrated into DebugProxy, an HTTP proxy debugger. \
        You have access to recent proxy logs (requests, responses, errors). \
        Your job is to:\n\
        1. Analyze the logs for issues (errors, timeouts, unusual patterns)\n\
        2. Suggest concrete actions (toggle filters, add routes, enable monitoring)\n\
        3. Forward critical observations to other agents when needed\n\
        4. Be concise and practical — prefer bullet points over paragraphs\n\
        5. When you see HTTP errors, explain what they mean and suggest fixes\n\
        Respond in the same language as the user's question."
    }

    pub async fn chat(&self, context: &str, question: &str) -> Result<AiResponse, String> {
        let messages = json!([
            {"role": "system", "content": Self::system_prompt()},
            {"role": "user", "content": format!("Recent proxy logs:\n```\n{}\n```\n\nQuestion: {}", context, question)}
        ]);

        let body = json!({
            "model": self.model(),
            "messages": messages,
            "tools": Self::tools(),
            "tool_choice": "auto",
            "temperature": 0.3,
            "max_tokens": 4096
        });

        let resp = self
            .client
            .post(self.endpoint())
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("AI request failed: {e}"))?;

        let status = resp.status();
        let raw: Value = resp
            .json()
            .await
            .map_err(|e| format!("Invalid AI response: {e}"))?;

        if !status.is_success() {
            let err_msg = raw
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown AI error");
            return Err(format!("AI error ({status}): {err_msg}"));
        }

        let choice = raw["choices"]
            .as_array()
            .and_then(|a| a.first())
            .ok_or("No choices in AI response")?;

        let msg = &choice["message"];
        let text = msg["content"]
            .as_str()
            .unwrap_or("(no response)")
            .to_string();

        let tool_calls: Vec<ToolCall> = msg["tool_calls"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|tc| {
                        let func = tc.get("function")?;
                        Some(ToolCall {
                            name: func.get("name")?.as_str()?.to_string(),
                            arguments: serde_json::from_str(
                                func.get("arguments")?.as_str()?,
                            )
                            .unwrap_or(json!({})),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        let response_text = text.clone();
        *self.last_response.lock().unwrap() = Some(response_text);

        Ok(AiResponse { text, tool_calls })
    }

    pub fn last_response_text(&self) -> Option<String> {
        self.last_response.lock().unwrap().clone()
    }

    pub fn forwarding_enabled(&self) -> bool {
        self.config
            .forwarding
            .as_ref()
            .and_then(|f| f.enabled)
            .unwrap_or(false)
    }

    pub fn webhook_url(&self) -> Option<String> {
        self.config
            .forwarding
            .as_ref()
            .and_then(|f| f.webhook_url.clone())
    }

    pub fn is_configured(&self) -> bool {
        !self.api_key.is_empty()
    }

    pub async fn forward(&self, message: &str, urgency: &str) -> Result<(), String> {
        let url = self.webhook_url().ok_or("No webhook URL configured")?;
        self.client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&json!({
                "source": "debugproxy-ai",
                "message": message,
                "urgency": urgency,
                "timestamp": chrono::Utc::now().to_rfc3339()
            }))
            .send()
            .await
            .map_err(|e| format!("Forward failed: {e}"))?;
        Ok(())
    }
}
