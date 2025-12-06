//! AI client for communicating with OpenAI LLM services.
//!
//! This module handles the core interaction with the OpenAI API,
//! sending user queries along with context information to get
//! command suggestions and natural language explanations.

use anyhow::Result;
use async_openai::types::{
    ChatCompletionRequestUserMessageArgs, CreateChatCompletionRequestArgs,
};
use async_openai::{config::OpenAIConfig, Client};
use futures::StreamExt;
use serde::{Deserialize, Serialize};

use crate::context::ContextSnapshot;

use super::{parser, prompt};

/// Structured command suggestion returned from LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiCommandSuggestion {
    pub natural_language_explanation: String,
    pub suggested_command: String,
    pub alternatives: Vec<String>,
}

/// Client for interacting with OpenAI API
pub struct AiClient {
    client: Client<OpenAIConfig>,
    pub model: String,
}

impl AiClient {
    /// Create a new AI client with the specified model
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            client: Client::new(),
            model: model.into(),
        }
    }

    /// Create a client with custom OpenAI configuration
    pub fn with_config(config: OpenAIConfig, model: impl Into<String>) -> Self {
        Self {
            client: Client::with_config(config),
            model: model.into(),
        }
    }

    /// Suggest a command based on user query and system context (non-streaming)
    ///
    /// This is a simpler API that waits for the complete response before returning.
    /// For streaming responses, use the session manager instead.
    pub async fn suggest_command(
        &self,
        user_query: &str,
        ctx: &ContextSnapshot,
    ) -> Result<AiCommandSuggestion> {
        // Build prompt with context
        let prompt_text = prompt::build_prompt(user_query, ctx);

        // Create user message
        let user_msg = ChatCompletionRequestUserMessageArgs::default()
            .content(prompt_text)
            .build()?
            .into();

        // Build request
        let request = CreateChatCompletionRequestArgs::default()
            .model(&self.model)
            .messages(vec![user_msg])
            .build()?;

        // Send request
        let response = self.client.chat().create(request).await?;

        // Extract response text
        let response_text = response
            .choices
            .first()
            .and_then(|choice| choice.message.content.as_ref())
            .ok_or_else(|| anyhow::anyhow!("No response from AI"))?;

        // Parse command suggestion
        parser::parse_command_suggestion(response_text)
            .ok_or_else(|| anyhow::anyhow!("Failed to parse command suggestion from response"))
    }

    /// Stream a command suggestion (returns the full response text)
    ///
    /// This method streams the response and returns the accumulated text.
    /// The caller is responsible for parsing the response.
    pub async fn suggest_command_stream(
        &self,
        user_query: &str,
        ctx: &ContextSnapshot,
    ) -> Result<String> {
        // Build prompt with context
        let prompt_text = prompt::build_prompt(user_query, ctx);

        // Create user message
        let user_msg = ChatCompletionRequestUserMessageArgs::default()
            .content(prompt_text)
            .build()?
            .into();

        // Build request
        let request = CreateChatCompletionRequestArgs::default()
            .model(&self.model)
            .messages(vec![user_msg])
            .build()?;

        // Create stream
        let mut stream = self.client.chat().create_stream(request).await?;

        // Accumulate response
        let mut full_response = String::new();
        while let Some(result) = stream.next().await {
            let response = result?;
            for choice in response.choices {
                if let Some(content) = choice.delta.content {
                    full_response.push_str(&content);
                }
            }
        }

        Ok(full_response)
    }

    /// Get the underlying OpenAI client for advanced usage
    pub fn inner(&self) -> &Client<OpenAIConfig> {
        &self.client
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = AiClient::new("gpt-4");
        assert_eq!(client.model, "gpt-4");
    }
}
