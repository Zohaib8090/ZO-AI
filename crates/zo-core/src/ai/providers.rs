//! The providers Zohara Chat can talk to. Three wire protocols cover all of them; most are the OpenAI-compatible one.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Protocol {
    /// Anthropic Messages API (Claude).
    Anthropic,
    /// OpenAI chat completions, also spoken by most hosts and every local server.
    OpenAi,
    /// Google Gemini generateContent.
    Gemini,
    /// Not a web protocol: the Claude Code program the person installed and signed in to.
    ClaudeCode,
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
    Provider { id: "claude-code", name: "Claude Code (your subscription, experimental)", protocol: ClaudeCode, base_url: "", local: true, models: &["sonnet", "opus", "haiku"] },
    // any other OpenAI-compatible server: the person types the address
    Provider { id: "custom", name: "Custom endpoint", protocol: OpenAi, base_url: "http://127.0.0.1:8000/v1", local: false, models: &[] },
];

pub fn all() -> &'static [Provider] {
    ALL
}

/// The host part of an http(s) address, lowercase, without user name or port (`""` when there is none).
fn host_of(url: &str) -> String {
    let rest = url.trim().split("://").nth(1).unwrap_or("");
    let auth = rest.split(['/', '?', '#']).next().unwrap_or("");
    let auth = auth.rsplit('@').next().unwrap_or("");
    let host = if auth.starts_with('[') { auth.split(']').next().unwrap_or("") } else { auth.split(':').next().unwrap_or("") };
    host.to_ascii_lowercase()
}

impl Provider {
    /// May this provider's API key be sent to `base`? Only to the provider's own host: a different address is for a Custom endpoint,
    /// which has a key of its own. A local server and a custom endpoint may be anywhere (their key is typed for that address).
    pub fn key_fits(&self, base: &str) -> bool {
        let base = base.trim();
        base.is_empty() || self.local || self.id == "custom" || self.protocol == Protocol::ClaudeCode || (!host_of(base).is_empty() && host_of(base) == host_of(self.base_url))
    }
}

pub fn find(id: &str) -> Option<&'static Provider> {
    ALL.iter().find(|p| p.id == id)
}

/// Can this model look at pictures? Anthropic and Gemini models can; OpenAI's, apart from a few old and small ones. For the many
/// other hosts it depends on the model, so its name decides. A model that is not recognised is treated as text-only, and the
/// person can say otherwise in the model settings (`Selection::vision`).
pub fn can_see(p: &Provider, model: &str) -> bool {
    let m = model.to_lowercase();
    match p.id {
        "claude-code" => false,
        "anthropic" => true,
        "gemini" => !(m.contains("embedding") || m.contains("aqa")),
        "openai" => !["gpt-3.5", "o1-mini", "o3-mini", "gpt-oss", "embedding", "whisper", "tts", "davinci", "babbage", "moderation", "instruct"].iter().any(|bad| m.contains(bad)),
        _ => [
            "vision", "-vl", "vl-", "_vl", "gpt-4o", "gpt-4.1", "gpt-4.5", "gpt-5", "claude", "gemini", "gemma-3", "gemma3", "llama-3.2", "llama3.2",
            "llama-4", "llama4", "pixtral", "llava", "minicpm-v", "internvl", "glm-4v", "glm-4.5v", "glm-4.6v", "kimi-vl", "step-1v", "step-3",
            "mistral-small-3", "mistral-medium", "grok-4", "nova-", "phi-4-multimodal", "molmo", "omni", "multimodal",
        ].iter().any(|k| m.contains(k)),
    }
}
