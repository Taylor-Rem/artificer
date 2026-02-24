use anyhow::Result;
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;
use artificer_shared::rusqlite;
use artificer_shared::db::Db;

use crate::pool::GpuHandle;
use crate::specialist::Specialist;
use crate::Message;

#[derive(Deserialize, Debug)]
struct ExtractedMemory {
    key: String,
    value: String,
    memory_type: String,
    confidence: f64,
}

#[derive(Deserialize, Debug)]
struct ExtractionResult {
    memories: Vec<ExtractedMemory>,
    keywords: Vec<String>,
}

pub async fn execute(
    db: &Db,
    gpu: &GpuHandle,
    client: &Client,
    args: &Value,
    context_messages: Option<&[Value]>,
) -> Result<String> {
    let conversation_id = args["conversation_id"].as_i64()
        .ok_or_else(|| anyhow::anyhow!("Missing conversation_id"))?;

    let messages = context_messages
        .ok_or_else(|| anyhow::anyhow!("MemoryExtraction requires conversation context"))?;

    let conversation_text: String = messages.iter()
        .map(|m| format!("{}: {}",
            m["role"].as_str().unwrap_or(""),
            m["message"].as_str().unwrap_or("")))
        .collect::<Vec<_>>()
        .join("\n");

    let extraction_prompt = format!(
        "Conversation:\n{}\n\n\
        Extract two types of information from this conversation:\n\n\
        1. MEMORIES - Key information classified as:\n\
        - FACT: Objective, verifiable information (OS, paths, shared, project details)\n\
        - PREFERENCE: User's subjective choices (style, tone, workflow preferences)\n\
        - CONTEXT: Current/temporary situation (what they're working on now)\n\n\
        2. KEYWORDS - Important terms, topics, and concepts that characterize this conversation.\n\
        Keywords should be:\n\
        - Single words or short phrases (1-3 words)\n\
        - Lowercase\n\
        - Descriptive of the conversation's content\n\
        - Technical terms, project names, important concepts\n\
        - Examples: 'rust', 'database design', 'memory system', 'api development'\n\n\
        Return a JSON object with this structure:\n\
        {{\n  \
          \"memories\": [\n    \
            {{\"key\": \"operating_system\", \"value\": \"Ubuntu 22.04\", \"memory_type\": \"fact\", \"confidence\": 1.0}}\n  \
          ],\n  \
          \"keywords\": [\"rust\", \"database\", \"memory extraction\", \"ollama\"]\n\
        }}\n\n\
        Rules:\n\
        - Facts should have high confidence (0.9-1.0)\n\
        - Preferences should have medium confidence (0.6-0.9)\n\
        - Context should vary based on how recent/relevant (0.5-0.9)\n\
        - Only extract information that will be useful later\n\
        - Extract 3-10 keywords that best describe this conversation\n\
        - Ignore ephemeral chat content",
        conversation_text
    );

    let specialist = Specialist::find("memory_extraction")
        .ok_or_else(|| anyhow::anyhow!("memory_extraction specialist not found"))?;

    let llm_messages = specialist.build_messages(
        vec![Message {
            role: "user".to_string(),
            content: Some(extraction_prompt),
            tool_calls: None,
        }],
        None,
    );

    let response = specialist.execute(gpu, llm_messages, None, client).await?;
    let extraction_json = response.content.unwrap_or_default();
    let extraction: ExtractionResult = serde_json::from_str(&extraction_json)?;

    // Get device_id from conversation
    let device_id: i64 = db.query_row_optional(
        "SELECT device_id FROM conversations WHERE id = ?1",
        rusqlite::params![conversation_id],
        |row| row.get(0),
    )?.ok_or_else(|| anyhow::anyhow!("Conversation not found"))?;

    // Store memories
    for memory in &extraction.memories {
        if memory.key.is_empty() || memory.value.is_empty() {
            continue;
        }

        if !matches!(memory.memory_type.as_str(), "fact" | "preference" | "context") {
            eprintln!("Warning: Invalid memory_type '{}', skipping", memory.memory_type);
            continue;
        }

        let target_task_id: Option<i64> = None;

        db.upsert_memory(
            device_id,
            target_task_id,
            &memory.key,
            &memory.value,
            &memory.memory_type,
            memory.confidence,
        )?;
    }

    // Store keywords
    db.attach_conversation_keywords(
        conversation_id as u64,
        &extraction.keywords,
    )?;

    Ok(format!(
        "Extracted {} memories ({} facts, {} preferences, {} context) and {} keywords",
        extraction.memories.len(),
        extraction.memories.iter().filter(|m| m.memory_type == "fact").count(),
        extraction.memories.iter().filter(|m| m.memory_type == "preference").count(),
        extraction.memories.iter().filter(|m| m.memory_type == "context").count(),
        extraction.keywords.len()
    ))
}

