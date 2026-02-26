use artificer_shared::events::ChatEvent;
use crate::client::ApiClient;
use anyhow::Result;
use std::io::{self, Write};

pub async fn single_message(
    client: ApiClient,
    device_id: i64,
    device_key: String,
    message: String,
) -> Result<()> {
    match client
        .chat(device_id, device_key.clone(), None, message, |event| {
            handle_event(&event)
        })
        .await
    {
        Ok(_) => {
            println!();
        }
        Err(e) => {
            eprintln!("Error: {}", e);
        }
    }
    Ok(())
}

pub async fn interactive_chat(client: ApiClient, device_id: i64, device_key: String) -> Result<()> {
    println!("Envoy chat started. Type 'quit' to exit.\n");

    let mut conversation_id: Option<u64> = None;

    loop {
        print!("You: ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim();

        if input.eq_ignore_ascii_case("quit") {
            println!("Goodbye!");
            break;
        }

        if input.is_empty() {
            continue;
        }

        println!(); // Blank line before response

        match client.chat(
            device_id,
            device_key.clone(),
            conversation_id,
            input.to_string(),
            |event| handle_event(&event),
        ).await {
            Ok(conv_id) => {
                conversation_id = Some(conv_id);
                println!("\n"); // Blank line after response
            }
            Err(e) => {
                eprintln!("Error: {}\n", e);
            }
        }
    }

    Ok(())
}

fn handle_event(event: &ChatEvent) {
    match event {
        ChatEvent::TaskSwitch { from, to } => {
            println!("\n⚡ Switching: {} → {}", from, to);
        }
        ChatEvent::ToolCall { task, tool, .. } => {
            println!("🔧 [{}] Calling: {}", task, tool);
        }
        ChatEvent::ToolResult { tool: _, result, truncated, .. } => {
            if *truncated {
                println!("   ✓ {} [truncated]", result.lines().next().unwrap_or(""));
            } else {
                println!("   ✓ {}", result);
            }
        }
        ChatEvent::StreamChunk { content } => {
            print!("{}", content);
            io::stdout().flush().ok();
        }
        ChatEvent::Done { .. } => {
            // Response complete, nothing to print
        }
        ChatEvent::Error { message } => {
            eprintln!("\n❌ Error: {}", message);
        }
        _ => {}
    }
}
