//! The providers ZO-AI can talk to. Three wire protocols cover all of them; most are the OpenAI-compatible one.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Protocol {
    /// Anthropic Messages API (Claude).
    Anthropic,
    /// OpenAI chat completions, also spoken by most hosts and every local server.
    OpenAi,
    /// Google Gemini generateContent.
    Gemini,
}

pub struct Provider {
    pub id: &'static str,
    pub name: &'static str,
    pub protocol: Protocol,
    /// Default base URL; the person can change it in the settings.
    pub base_url: &'static str,
    /// Runs on this computer: no key, works offline.
    pub local: bool,
    /// Suggestions shown before the live model list is fetched.
    pub models: &'static [&'static str],
}

const fn p(id: &'static str, name: &'static str, protocol: Protocol, base_url: &'static str, models: &'static [&'static str]) -> Provider {
    Provider { id, name, protocol, base_url, local: false, models }
}

const fn local(id: &'static str, name: &'static str, base_url: &'static str) -> Provider {
    Provider { id, name, protocol: Protocol::OpenAi, base_url, local: true, models: &[] }
}

use Protocol::*;

static ALL: &[Provider] = &[
    p("anthropic", "Anthropic (Claude)", Anthropic, "https://api.anthropic.com/v1",
        &["claude-sonnet-5-5", "claude-opus-5-5", "claude-haiku-5-5", "claude-fable-5-1"]),
    p("openai", "OpenAI", OpenAi, "https://api.openai.com/v1", &[]),
    p("gemini", "Google Gemini", Gemini, "https://generativelanguage.googleapis.com/v1beta", &[]),
    p("openrouter", "OpenRouter", OpenAi, "https://openrouter.ai/api/v1", &[]),
    p("deepseek", "DeepSeek", OpenAi, "https://api.deepseek.com/v1", &[]),
    p("fireworks", "Fireworks AI", OpenAi, "https://api.fireworks.ai/inference/v1", &[]),
    p("groq", "Groq", OpenAi, "https://api.groq.com/openai/v1", &[]),
    p("mistral", "Mistral", OpenAi, "https://api.mistral.ai/v1", &[]),
    p("together", "Together AI", OpenAi, "https://api.together.xyz/v1", &[]),
    p("cerebras", "Cerebras", OpenAi, "https://api.cerebras.ai/v1", &[]),
    p("perplexity", "Perplexity", OpenAi, "https://api.perplexity.ai/v1", &[]),
    p("novita", "NovitaAI", OpenAi, "https://api.novita.ai/openai/v1", &[]),
    p("gmi", "GMI Cloud", OpenAi, "https://api.gmi-serving.com/v1", &[]),
    p("stepfun", "StepFun", OpenAi, "https://api.stepfun.com/v1", &[]),
    p("kimi", "Kimi / Moonshot", OpenAi, "https://api.moonshot.ai/v1", &[]),
    p("zai", "z.ai / GLM", OpenAi, "https://api.z.ai/api/paas/v4", &[]),
    p("qwen", "Qwen (Alibaba DashScope)", OpenAi, "https://dashscope-intl.aliyuncs.com/compatible-mode/v1", &[]),
    p("minimax", "MiniMax", OpenAi, "https://api.minimax.io/v1", &[]),
    p("nvidia", "NVIDIA Build", OpenAi, "https://integrate.api.nvidia.com/v1", &[]),
    p("huggingface", "Hugging Face", OpenAi, "https://router.huggingface.co/v1", &[]),
    local("ollama", "Ollama (this computer)", "http://127.0.0.1:11434/v1"),
    local("lmstudio", "LM Studio (this computer)", "http://127.0.0.1:1234/v1"),
    local("llamacpp", "llama.cpp (this computer)", "http://127.0.0.1:8080/v1"),
    local("vllm", "vLLM (this computer)", "http://127.0.0.1:8000/v1"),
    local("sglang", "SGLang (this computer)", "http://127.0.0.1:30000/v1"),
    // any other OpenAI-compatible server: the person types the address
    Provider { id: "custom", name: "Custom endpoint", protocol: OpenAi, base_url: "http://127.0.0.1:8000/v1", local: false, models: &[] },
];

pub fn all() -> &'static [Provider] {
    ALL
}

pub fn find(id: &str) -> Option<&'static Provider> {
    ALL.iter().find(|p| p.id == id)
}
