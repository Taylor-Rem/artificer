use anyhow::Result;
use std::io::{self, Write};

use crate::Message;
use crate::memory::Db;
use crate::task::current_task::CurrentTask;
use crate::task::Task;

pub async fn execute() -> Result<()> {
    let db = Db::default();
    let conversation = CurrentTask::default();

    let mut th_id: Option<u64> = None;
    let mut first_interaction = true;
    let mut message_count = 0;
    let mut messages = vec![];

    loop {
        // Get user input
        let input = wait_for_user_input()?;

        // Handle quit
        if input.eq_ignore_ascii_case("quit") {
            if let Some(id) = th_id {
                // Queue summarization and memory extraction
                if let Err(e) = conversation.summarize(id) {
                    eprintln!("Warning: Failed to queue summarization: {}", e);
                }
                if let Err(e) = conversation.extract_memory(id) {
                    eprintln!("Warning: Failed to queue memory extraction: {}", e);
                }
            }
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

        // Initialize conversation on first message
        if first_interaction {
            first_interaction = false;
            match conversation.init(user_message.clone(), "").await {
                Ok(id) => th_id = Some(id),
                Err(e) => {
                    eprintln!("Warning: Failed to create conversation - history will not be saved.");
                    eprintln!("   Error: {}", e);
                }
            }
        }

        // Save user message
        if let Err(e) = conversation.create_message(
            th_id,
            "user",
            &input,
            &mut message_count
        ) {
            if th_id.is_some() {
                eprintln!("Warning: Failed to save user message: {}", e);
            }
        }

        messages.push(user_message);

        // Execute chat task with agentic loop
        let response = Task::Chat
            .execute_with_prompt(messages.clone(), &db, true)
            .await?;

        // Update messages with response
        messages.push(response.to_message());

        // Save assistant response
        if let Some(content) = &response.content {
            if let Err(e) = conversation.create_message(
                th_id,
                "assistant",
                content,
                &mut message_count
            ) {
                if th_id.is_some() {
                    eprintln!("Warning: Failed to save assistant message: {}", e);
                }
            }
        }

        println!("\n");
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