use anyhow::Result;
use std::io::{self, Write};
use crate::client::ApiClient;

pub async fn interactive_chat(client: ApiClient, device_id: i64, device_key: String) -> Result<()> {
    println!("Envoy chat started. Type 'quit' to exit.\n");

    let mut conversation_id: Option<u64> = None;

    loop {
        // Get user input
        print!("You: ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim();

        // Handle quit
        if input.eq_ignore_ascii_case("quit") {
            if let Some(conv_id) = conversation_id {
                println!("Queueing background processing...");
                let _ = client.queue_summarization(device_id, device_key.clone(), conv_id).await;
                let _ = client.queue_memory_extraction(device_id, device_key.clone(), conv_id).await;
            }
            println!("Goodbye!");
            break;
        }

        if input.is_empty() {
            continue;
        }

        // Send to artificer
        match client.chat(device_id, device_key.clone(), conversation_id, input.to_string()).await {
            Ok(response) => {
                conversation_id = Some(response.conversation_id);
                println!("\nAssistant: {}\n", response.content);
            }
            Err(e) => {
                eprintln!("Error: {}", e);
            }
        }
    }
    Ok(())
}

pub async fn single_message(client: ApiClient, device_id: i64, device_key: String, message: String) -> Result<()> {
    match client.chat(device_id, device_key, None, message).await {
        Ok(response) => {
            println!("{}", response.content);
        }
        Err(e) => {
            eprintln!("Error: {}", e);
        }
    }

    Ok(())
}
