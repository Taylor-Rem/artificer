use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::io::{self, Write};

mod traits;
mod toolbelts;
mod registry;
mod artificer;

use artificer::Artificer;
use crate::traits::{Agent, ToolCall, ToolCaller};

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
    let tools = registry::get_tools();

    let mut messages = vec![Message {
        role: "system".to_string(),
        content: Some(artificer.system_prompt().to_string()),
        tool_calls: None,
    }];

    println!("Artificer is ready. Type 'quit' to exit.\n");
    println!("Available tools: {}", tools.iter().map(|t| t.function.name.as_str()).collect::<Vec<_>>().join(", "));
    println!();

    loop {
        let input = wait_for_user_input()?;
        if input.eq_ignore_ascii_case("quit") {
            println!("Goodbye!");
            break;
        }
        if input.is_empty() {
            continue;
        }

        messages.push(Message {
            role: "user".to_string(),
            content: Some(input),
            tool_calls: None,
        });

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