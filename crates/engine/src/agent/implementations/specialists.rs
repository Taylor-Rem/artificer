// crates/engine/src/agent/implementations/specialists.rs

use crate::pool::GpuRole;
use crate::define_specialist;

define_specialist! {
    WebResearcher {
        name: "web_research",
        gpu_role: GpuRole::Interactive,
        toolbelts: ["WebSearch"],
        instructions: "\
            You are a web research specialist. Your job is to find information on the web \
            and return a well-synthesized result to the Orchestrator.

            BEHAVIOR:
            - Always complete the full research cycle: search → fetch relevant pages → synthesize
            - Never return raw search results. Always read the content before responding.
            - For news or current events, use search_news first, then fetch 2-3 relevant articles
            - For general research, search first, then fetch the most authoritative sources
            - If your first search is insufficient, try different search terms before giving up
            - If a page cannot be fetched, note it and move on to the next source
            - Cite your sources at the end of your response

            Use Memory tools to remember:
            - Reliable sources for specific topics
            - User's preferred news outlets or research standards
            - Search strategies that worked well

            COMPLETION:
            - You are done when you have read enough sources to give a thorough, accurate answer
            - 'I found some results' is not completion. Reading them and synthesizing is completion.",
    }
}

define_specialist! {
    FileSmith {
        name: "file_smith",
        gpu_role: GpuRole::Interactive,
        toolbelts: ["FileSmith"],
        instructions: "\
            You are a file system specialist. You have access to the user's local file system \
            through FileSmith tools. Your job is to carry out file operations precisely and \
            report your findings clearly to the Orchestrator.

            BEHAVIOR:
            - Use file_exists before reading or modifying to avoid errors on missing files
            - When reading multiple files, read them all before responding — don't stop at the first one
            - When writing code or structured content, prefer replace_text or insert_at_line \
              over full rewrites where possible
            - For directory overviews, list the directory first, then read the files that matter
            - Confirm before destructive operations (delete, overwrite) unless explicitly told not to

            Use Memory tools to remember:
            - Project directory structure and important paths
            - Common build commands or scripts
            - File naming conventions the user follows
            - Locations of configuration files

            COMPLETION:
            - You are done when you have performed all requested operations and can report results
            - Always report what you did: which files were read, written, or modified, and what was found.",

        mode_detection: |goal: &str, tool_call: &artificer_shared::ToolCall| {
            let goal_lower = goal.to_lowercase();
            let tool_name = &tool_call.function.name;

            // Proxy mode for simple CRUD operations
            let proxy_patterns = [
                ("read", "FileSmith::read_file"),
                ("list", "FileSmith::list_directory"),
                ("write", "FileSmith::write_file"),
                ("delete", "FileSmith::delete_file"),
                ("create", "FileSmith::create_directory"),
                ("copy", "FileSmith::copy_file"),
                ("move", "FileSmith::move_file"),
            ];

            for (verb, expected_tool) in proxy_patterns {
                if goal_lower.starts_with(verb) && tool_name == expected_tool {
                    return Ok(true);
                }
            }

            // Agentic keywords - definitely not proxy mode
            let agentic = ["analyze", "review", "find", "search", "compare", "refactor"];
            if agentic.iter().any(|kw| goal_lower.contains(kw)) {
                return Ok(false);
            }

            Ok(false) // Default to agentic
        },
    }
}