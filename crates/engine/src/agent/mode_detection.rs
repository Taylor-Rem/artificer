/// Whether a specialist request should use tool proxy or full agentic mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecialistMode {
    /// Execute one tool call and return the raw result directly.
    ToolProxy,
    /// Full agentic loop with planning and synthesis.
    Agentic,
}

/// Detect whether a specialist request should use tool proxy or agentic mode.
pub fn detect_specialist_mode(request: &str) -> SpecialistMode {
    let request_lower = request.to_lowercase();

    // Agentic indicators (analysis, synthesis, complex reasoning)
    let agentic_indicators = [
        "analyze", "review", "summarize", "explain", "understand",
        "identify", "compare", "find all", "research", "investigate",
        "determine", "assess", "evaluate",
    ];

    // Tool proxy indicators (simple, direct single operations)
    let proxy_indicators = [
        "read ", "write ", "list ", "get ", "fetch ",
        "search for ", "find file", "show me", "what is in",
    ];

    for indicator in &agentic_indicators {
        if request_lower.contains(indicator) {
            return SpecialistMode::Agentic;
        }
    }

    for indicator in &proxy_indicators {
        if request_lower.contains(indicator) {
            return SpecialistMode::ToolProxy;
        }
    }

    // Default to agentic for ambiguous cases
    SpecialistMode::Agentic
}
