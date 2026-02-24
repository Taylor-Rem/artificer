const SPECIALIST_BASE: &str = "
You are a specialist working as part of a larger AI system.

Your role is to execute specific tasks assigned by the orchestrator with precision and completeness.

# How You Work

When you receive a request, determine your mode of operation:

**TOOL PROXY MODE** (simple, direct operations):
- Single action with clear tool mapping
- Examples: 'read file.txt', 'search for X', 'list directory'
- Behavior: Extract parameters, execute tool, return raw result
- No planning, no iteration, just execute and respond

**AGENTIC MODE** (complex, multi-step work):
- Analysis, synthesis, exploratory work
- Examples: 'review the codebase', 'research topic X thoroughly'
- Behavior: Create plan, execute steps, evaluate results, iterate until complete
- Use your full capabilities to achieve the goal

# Standards

- Be precise and thorough
- If in agentic mode, work until the goal is genuinely achieved
- Report clearly what you did and what you found
- Don't stop at partial results
";

pub fn build_specialist_prompt(
    specialist_name: &str,
    custom_instructions: &str,
    toolbelts: &[&str],
    memory_context: Option<&str>,
) -> String {
    let mut prompt = String::new();

    // Base specialist instructions
    prompt.push_str(SPECIALIST_BASE.trim());
    prompt.push_str("\n\n");

    // Custom specialist instructions
    prompt.push_str("# Your Specific Role\n\n");
    prompt.push_str(custom_instructions.trim());
    prompt.push_str("\n\n");

    // Available tools
    if !toolbelts.is_empty() {
        prompt.push_str("# Available Tools\n\n");
        let schemas = get_tool_schemas_for(toolbelts);
        for schema in schemas {
            prompt.push_str(&format!("## {}\n{}\n", schema.name, schema.description));
            if !schema.parameters.is_empty() {
                prompt.push_str("Parameters:\n");
                for param in &schema.parameters {
                    prompt.push_str(&format!(
                        "- `{}` ({}{}): {}\n",
                        param.name,
                        param.type_name,
                        if param.required { ", required" } else { ", optional" },
                        param.description
                    ));
                }
            }
            prompt.push_str("\n");
        }
    }

    // Memory context
    if let Some(memory) = memory_context {
        if !memory.trim().is_empty() {
            prompt.push_str("# What You Know About This User\n\n");
            prompt.push_str(memory);
            prompt.push_str("\n");
        }
    }

    prompt
}