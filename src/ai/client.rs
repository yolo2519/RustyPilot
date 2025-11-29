//! AI client for communicating with OpenAI LLM services.
//!
//! This module handles the core interaction with the OpenAI API,
//! sending user queries along with context information to get
//! command suggestions and natural language explanations.

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

        // TODO: this returns a dummy suggestion now
        Ok(AiCommandSuggestion {
            natural_language_explanation: format!("Based on your query [{user_query}] and context [{ctx:?}] I would suggest this command."),
            suggested_command: "echo 'hello world'".to_string(),
            alternatives: vec![],
        })
    }

    /// reserve getter func for other modules
    pub fn inner(&self) -> &Client<OpenAIConfig> {
        &self.client
    }
}
