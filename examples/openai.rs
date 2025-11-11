use std::error::Error;
use std::io::{self, stdout, Write};

use async_openai::types::{ChatCompletionRequestUserMessageArgs, ChatCompletionRequestAssistantMessageArgs, CreateChatCompletionRequestArgs, ChatCompletionRequestMessage};
use async_openai::Client;
use futures::StreamExt;

async fn chat_with_gpt_streaming(
    client: &Client<async_openai::config::OpenAIConfig>,
    conversation_history: &mut Vec<ChatCompletionRequestMessage>,
    user_message: &str,
) -> Result<String, Box<dyn Error>> {
    // Add user message to conversation history
    let user_msg = ChatCompletionRequestUserMessageArgs::default()
        .content(user_message)
        .build()?
        .into();
    conversation_history.push(user_msg);
    println!("[DEBUG] conversation_history length: {:?}", conversation_history.len());
    let request = CreateChatCompletionRequestArgs::default()
        .model("gpt-5-nano")
        .messages(conversation_history.as_slice())
        .build()?;

    let mut stream = client.chat().create_stream(request).await?;
    let mut ai_response = String::new();

    // Stream the response
    let mut lock = stdout().lock();
    while let Some(result) = stream.next().await {
        match result {
            Ok(response) => {
                response.choices.iter().for_each(|chat_choice| {
                    if let Some(ref content) = chat_choice.delta.content {
                        write!(lock, "{}", content).unwrap();
                        ai_response.push_str(&content);
                    }
                });
            }
            Err(err) => {
                writeln!(lock, "error: {err}").unwrap();
                break;
            }
        }
        stdout().flush()?;
    }

    // Add AI response to conversation history
    if !ai_response.trim().is_empty() {
        let assistant_msg = ChatCompletionRequestAssistantMessageArgs::default()
            .content(ai_response.trim())
            .build()?
            .into();
        conversation_history.push(assistant_msg);
    }

    Ok(ai_response.trim().to_string())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    println!("NOTE: Please make sure you have set your OPENAI_API_KEY environment variable.");
    println!("Hello AI! Starting ChatGPT REPL with streaming...");
    println!("Type your messages and press Enter. Press Ctrl-C to exit.");
    println!("The AI will remember our conversation history!");
    println!("----------------------------------------");

    let client = Client::new();
    let mut conversation_history: Vec<ChatCompletionRequestMessage> = Vec::new();

    loop {
        // Print prompt
        print!("You: ");
        io::stdout().flush()?;

        // Read user input
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim();

        // Check for exit conditions
        if input.is_empty() {
            continue;
        }

        if input.eq_ignore_ascii_case("exit") || input.eq_ignore_ascii_case("quit") {
            println!("Goodbye!");
            break;
        }

        // Special command to clear conversation history
        if input.eq_ignore_ascii_case("clear") {
            conversation_history.clear();
            println!("Conversation history cleared!");
            continue;
        }

        // Send to ChatGPT and get streaming response
        if let Err(e) = chat_with_gpt_streaming(&client, &mut conversation_history, input).await {
            eprintln!("Error communicating with ChatGPT: {}", e);
            continue;
        }
        println!(); // Add blank line for readability
    }

    Ok(())
}