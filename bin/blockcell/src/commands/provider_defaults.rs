pub(crate) fn default_model_for_provider(provider: &str) -> &'static str {
    match provider.trim().to_lowercase().as_str() {
        "deepseek" => "deepseek-v4-pro",
        "openai" => "gpt-5.5",
        "anthropic" | "claude" => "claude-opus-4-8",
        "kimi" | "moonshot" => "kimi-k2.6",
        "gemini" => "gemini-3.1-pro-preview",
        "zhipu" | "glm" => "glm-5.2",
        "xai" | "grok" => "grok-4.3",
        "mistral" => "mistral-medium-2604",
        "minimax" => "MiniMax-M3",
        "qwen" => "qwen3.7-max",
        "groq" => "openai/gpt-oss-120b",
        "siliconflow" => "deepseek-ai/DeepSeek-V4-Pro",
        "openrouter" => "openai/gpt-5.5",
        "ollama" => "qwen3.6",
        _ => "gpt-5.5",
    }
}
