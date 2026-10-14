//! The places a model can come from: the providers a key is of, and
//! the servers looked for on this computer.

use hamlet_model::config::Endpoint;

/// A place that serves models.
pub(super) struct Provider {
    /// The name the chief knows it by.
    pub name: &'static str,
    /// Its name on a button.
    pub lettering: &'static str,
    /// The name of its endpoint in the settings.
    pub endpoint: &'static str,
    pub base_url: &'static str,
    /// The place a key for it is made.
    pub keys: &'static str,
}

/// The providers a key can be of, each at the address its own
/// documentation gives for the OpenAI chat protocol.
pub(super) const PROVIDERS: [Provider; 6] = [
    Provider {
        name: "OpenRouter",
        lettering: "OPENROUTER",
        endpoint: "openrouter",
        base_url: "https://openrouter.ai/api/v1",
        keys: "openrouter.ai/keys",
    },
    Provider {
        name: "OpenAI",
        lettering: "OPENAI",
        endpoint: "openai",
        base_url: "https://api.openai.com/v1",
        keys: "platform.openai.com/api-keys",
    },
    Provider {
        name: "Google Gemini",
        lettering: "GOOGLE GEMINI",
        endpoint: "gemini",
        base_url:
            "https://generativelanguage.googleapis.com/v1beta/openai",
        keys: "aistudio.google.com/apikey",
    },
    Provider {
        name: "Groq",
        lettering: "GROQ",
        endpoint: "groq",
        base_url: "https://api.groq.com/openai/v1",
        keys: "console.groq.com/keys",
    },
    Provider {
        name: "DeepSeek",
        lettering: "DEEPSEEK",
        endpoint: "deepseek",
        base_url: "https://api.deepseek.com",
        keys: "platform.deepseek.com",
    },
    // Its address for use paid by the token. Its coding plan has
    // another, which it keeps for the tools it names.
    Provider {
        name: "Z.ai",
        lettering: "Z.AI",
        endpoint: "zai",
        base_url: "https://api.z.ai/api/paas/v4",
        keys: "z.ai",
    },
];

/// The place of OpenRouter in [`PROVIDERS`]: the one a chief can sign
/// in to with nothing to copy.
pub(super) const OPENROUTER: usize = 0;

/// The servers looked for on this computer, in order.
pub(super) const LOCAL: [Provider; 2] = [
    Provider {
        name: "Ollama",
        lettering: "",
        endpoint: "local",
        base_url: "http://localhost:11434/v1",
        keys: "",
    },
    Provider {
        name: "LM Studio",
        lettering: "",
        endpoint: "lm-studio",
        base_url: "http://localhost:1234/v1",
        keys: "",
    },
];

/// The name of the endpoint of an address the chief typed.
pub(super) const OTHER: &str = "custom";

pub(super) fn served(local: &Provider) -> Endpoint {
    Endpoint {
        name: local.endpoint.into(),
        base_url: local.base_url.into(),
        chatgpt: false,
        api_key: None,
    }
}
