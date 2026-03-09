// crates/engine/src/agent/implementations/background.rs

use crate::define_background_agent;

define_background_agent! {
    TitleGeneration {
        name: "title_generation",
        instructions: "\
            Generate a concise, descriptive title (3-5 words) for this conversation or task.
            Use underscores instead of spaces.
            Use only alphanumeric characters and underscores.
            Return ONLY the title with no explanation, punctuation, or quotes.",
    }
}

define_background_agent! {
    Summarization {
        name: "summarization",
        instructions: "\
            You are a conversation archivist. Write a compact factual summary that will help \
            an AI assistant quickly understand what was discussed without reading the full transcript.

            Write 2-4 sentences maximum. Use plain prose — no markdown, no headers, no bullet points.

            Focus on:
            - Topics discussed
            - Key facts the user shared (names, places, objects, preferences)
            - Any decisions made or questions answered

            Do NOT include:
            - The assistant's responses or advice
            - Emotional commentary or filler phrases
            - Any formatting whatsoever",
    }
}

define_background_agent! {
    MemoryExtraction {
        name: "memory_extraction",
        instructions: "\
            Review this conversation and extract information worth remembering for future sessions.

            Classify each memory as:
            - fact: Objective, verifiable information (OS, paths, project names, hardware)
            - preference: How the user likes things done (style, tone, workflow)
            - context: What the user is currently working on (may change over time)

            Return a JSON object with this exact structure:
            {
              \"memories\": [
                {\"key\": \"operating_system\", \"value\": \"Ubuntu 22.04\", \"memory_type\": \"fact\", \"confidence\": 1.0}
              ],
              \"keywords\": [\"rust\", \"database\", \"memory extraction\"]
            }

            Rules:
            - Facts: confidence 0.9-1.0
            - Preferences: confidence 0.6-0.9
            - Context: confidence 0.5-0.9 based on how current it is
            - Keywords: 3-10 lowercase terms that characterize the conversation
            - Only extract information useful across future sessions
            - Return valid JSON only — no explanation, no markdown fences",
    }
}