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
        Ok(_conv_id) => {

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
        ChatEvent::ToolCall { task, tool, args } => {
            println!("🔧 [{}] Calling: {}", task, tool);
            let args_str = if args.is_null() || args == &serde_json::Value::Object(Default::default()) {
                "(no args)".to_string()
            } else {
                let compact = serde_json::to_string(args).unwrap_or_default();
                if compact.len() > 300 {
                    format!("{}… ({} chars)", &compact[..300], compact.len())
                } else {
                    compact
                }
            };
            println!("   args: {}", args_str);
        }
        ChatEvent::ToolResult { task, tool, result, truncated } => {
            let lines: Vec<&str> = result.lines().collect();
            let line_count = lines.len();
            let char_count = result.len();
            let preview: String = if result.len() > 400 {
                format!("{}…", &result[..400])
            } else {
                result.clone()
            };
            let trunc_flag = if *truncated { " [TRUNCATED BY SERVER]" } else { "" };
            println!(
                "   ✓ [{}] {} → {} lines, {} chars{}\n   {}",
                task, tool, line_count, char_count, trunc_flag, preview
            );
        }
        ChatEvent::ResponseComplete { content } => {
            println!("\n📨 ResponseComplete ({} chars): {}", content.len(),
                     if content.len() > 200 { format!("{}…", &content[..200]) } else { content.clone() }
            );
        }
        ChatEvent::StreamChunk { content } => {
            print!("{}", content);
            io::stdout().flush().ok();
        }
        ChatEvent::Done { conversation_id } => {
            println!("\n✅ Done (conv_id={})", conversation_id);
        }
        ChatEvent::Error { message } => {
            eprintln!("\n❌ Error: {}", message);
        }
        ChatEvent::Reasoning { task, content } => {
            print!("\x1b[2m\x1b[90m💭 [{}] {}\x1b[0m", task, content);
            io::stdout().flush().ok();
        }
    }
}