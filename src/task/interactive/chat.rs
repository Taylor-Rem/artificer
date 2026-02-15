use anyhow::Result;
use std::io::{self, Write};

use crate::Message;
use crate::memory::Db;
use crate::task::conversation::Conversation;
use crate::task::Task;
use crate::state::AppState;

pub async fn execute(state: AppState) -> Result<()> {
    let db = Db::default();
    let device_id = state.device_id().await;
    let conversation = Conversation::new(device_id);
    let current_task = state.current_task().await;

    // Preload model into VRAM
    println!("Loading model into memory...");
    if let Err(e) = preload_model(&current_task, &db).await {
        eprintln!("Warning: Failed to preload model: {}", e);
        eprintln!("First response may be slower.");
    } else {
        println!("Model loaded. Ready!\n");
    }

    let mut conversation_id: Option<u64> = None;
    let mut task_history_id: Option<u64> = None;
    let mut first_interaction = true;
    let mut message_count = 0;
    let mut messages = vec![];

    loop {
        // Get user input
        let input = wait_for_user_input()?;

        // Handle quit
        if input.eq_ignore_ascii_case("quit") {
            if let Some(conv_id) = conversation_id {
                // Mark current task as completed
                if let Some(th_id) = task_history_id {
                    if let Err(e) = conversation.complete_task(th_id) {
                        eprintln!("Warning: Failed to mark task complete: {}", e);
                    }
                }

                // Queue summarization and memory extraction
                if let Err(e) = conversation.summarize(conv_id) {
                    eprintln!("Warning: Failed to queue summarization: {}", e);
                }
                if let Err(e) = conversation.extract_memory(conv_id) {
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
            match conversation.init(&user_message, &current_task).await {
                Ok((conv_id, th_id)) => {
                    conversation_id = Some(conv_id);
                    task_history_id = Some(th_id);
                }
                Err(e) => {
                    eprintln!("Warning: Failed to create conversation - history will not be saved.");
                    eprintln!("   Error: {}", e);
                }
            }
        }

        // Save user message
        if let Err(e) = conversation.add_message(
            conversation_id,
            "user",
            &input,
            &mut message_count
        ) {
            if conversation_id.is_some() {
                eprintln!("Warning: Failed to save user message: {}", e);
            }
        }

        messages.push(user_message);

        // Execute chat task with agentic loop
        let response = current_task
            .execute_with_prompt(messages.clone(), &db, device_id, true)
            .await?;

        // Update messages with response
        messages.push(response.to_message());

        // Save assistant response
        if let Some(content) = &response.content {
            if let Err(e) = conversation.add_message(
                conversation_id,
                "assistant",
                content,
                &mut message_count
            ) {
                if conversation_id.is_some() {
                    eprintln!("Warning: Failed to save assistant message: {}", e);
                }
            }
        }

        println!("\n");
    }

    Ok(())
}

async fn preload_model(task: &Task, _db: &Db) -> Result<()> {
    // Create a minimal message to load the model
    let warmup_messages = vec![
        Message {
            role: "system".to_string(),
            content: Some(task.instructions().to_string()),
            tool_calls: None,
        },
        Message {
            role: "user".to_string(),
            content: Some(".".to_string()),
            tool_calls: None,
        },
    ];

    // Execute with streaming=false to avoid printing the response
    let _ = task.execute(warmup_messages, false).await?;

    Ok(())
}

fn wait_for_user_input() -> Result<String> {
    print!("You: ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_string())
}