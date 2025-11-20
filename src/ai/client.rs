use anyhow::Result;
use async_openai::{Client, config::OpenAIConfig};
use serde::{Deserialize, Serialize};

use crate::context::ContextSnapshot;

/// structured content returned from LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiCommandSuggestion {
    pub natural_language_explanation: String,
    pub suggested_command: String,
    pub alternatives: Vec<String>,
}

pub struct AiClient {
    // OpenAIConfig
    client: Client<OpenAIConfig>,
    pub model: String,
}

impl AiClient {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            client: Client::new(),
            model: model.into(),
        }
    }

    pub async fn suggest_command(
        &self,
        user_query: &str,
        ctx: &ContextSnapshot,
    ) -> Result<AiCommandSuggestion> {
        // TODO:
        // 1. use prompt::build_prompt(user_query, ctx) to build prompt
        // 2. self.client.chat().create(...) / create_stream(...)
        // 3. parse to AiCommandSuggestion

        // 先返回 dummy，保证架子能编译跑起来
        Ok(AiCommandSuggestion {
            natural_language_explanation: "Dummy explanation".to_string(),
            suggested_command: "echo 'hello world'".to_string(),
            alternatives: vec![],
        })
    }

    /// reserve getter func for other modules
    pub fn inner(&self) -> &Client<OpenAIConfig> {
        &self.client
    }
}
