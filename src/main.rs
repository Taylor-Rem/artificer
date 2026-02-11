use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::io::{self, Write};

mod registry;
mod agents;
pub mod toolbelts;
pub mod traits;
pub mod functionality;
pub mod db;

use agents::artificer::Artificer;
use toolbelts::archivist::Archivist;
use traits::{Agent, ToolCall, ToolCaller};

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Message {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let artificer = Artificer;
    let archivist = Archivist::default();
    let tools = registry::get_tools();

    let mut messages = vec![Message {
        role: "system".to_string(),
        content: Some(artificer.system_prompt().to_string()),
        tool_calls: None,
    }];
    let mut conversation_id: Option<u64> = None;
    let mut first_loop = true;

    println!("Artificer is ready. Type 'quit' to exit.\n");
    println!("Available tools: {}", tools.iter().map(|t| t.function.name.as_str()).collect::<Vec<_>>().join(", "));
    println!();

    let mut message_count = 0;
    loop {
        let input = wait_for_user_input()?;
        if input.eq_ignore_ascii_case("quit") {
            println!("Goodbye!");
            break;
        }
        if input.is_empty() {
            continue;
        }

        let user_message = Message {
            role: "user".to_string(),
            content: Some(input.clone()),
            tool_calls: None,
        };

        if first_loop {
            first_loop = false;
            match archivist.initialize_conversation(user_message.clone(), "").await {
                Ok(id) => conversation_id = Some(id),
                Err(e) => {
                    eprintln!("Warning: Failed to create conversation - history will not be saved.");
                    eprintln!("   Error: {}", e);
                }
            }
        }
        if let Err(e) = archivist.create_message(conversation_id, "user", &input, &mut message_count) {
            if conversation_id.is_some() {
                eprintln!("Warning: Failed to save user message to history.");
                eprintln!("   Error: {}", e);
            }
        }
        messages.push(user_message);

        // Chat loop - handles tool calls until we get a final response
        loop {
            let response = artificer.make_request(&messages, Some(tools.clone())).await?;

            // Add assistant message to history
            messages.push(response.to_message());

            // Check if the model wants to call tools
            if let Some(tool_calls) = &response.tool_calls {
                for tool_call in tool_calls {
                    let tool_name = &tool_call.function.name;
                    let args = &tool_call.function.arguments;

                    println!("[Calling tool: {} with args: {}]", tool_name, args);

                    let result = artificer.use_tool(tool_name, args)
                        .unwrap_or_else(|e| format!("Error: {}", e));

                    println!("[Tool result: {}]", result);

                    // Add tool result to messages
                    messages.push(Message {
                        role: "tool".to_string(),
                        content: Some(json!({
                            "name": tool_name,
                            "result": result
                        }).to_string()),
                        tool_calls: None,
                    });
                }
                // Continue loop to let model process tool results
            } else {
                // No tool calls - print response and break inner loop
                let content = response.content.unwrap_or_default();
                if let Err(e) = archivist.create_message(conversation_id, "assistant", &content, &mut message_count) {
                    if conversation_id.is_some() {
                        eprintln!("Warning: Failed to save assistant message to history.");
                        eprintln!("   Error: {}", e);
                    }
                }
                println!("\nArtificer: {}\n", content);
                break;
            }
        }
    }
    Ok(())
}

fn wait_for_user_input() -> Result<String> {
    print!("You: ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_string())
}
