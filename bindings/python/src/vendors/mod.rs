//! The vendors the module holds, one module each. A vendor's part is its
//! configuration, how its provider is built from it and the names `RoomKit`
//! knows the provider by: what `vendor::Vendor` asks. Every entry Python
//! calls, and everything a built provider does, is written once for all of
//! them, in `vendor` and in `provider`.
//!
//! Adding a provider adds its module here, and its line in [`VENDORS`]; an
//! image provider, its line in [`IMAGE_VENDORS`].

use super::image_vendor::ImageEntries;
use super::vendor::Entries;

mod anthropic;
mod azure;
mod cerebras;
mod deepseek;
mod gemini;
mod gemini_image;
mod kimi;
mod litellm;
mod llamacpp;
mod meta;
mod minimax;
mod mistral;
mod ollama;
mod openai;
mod openai_image;
mod openrouter;
mod openrouter_image;
mod polargrid;
mod qwen;
mod vertex;
mod vllm;
mod xai;

/// The entries of every vendor.
pub(crate) static VENDORS: &[Entries] = &[
    Entries::of::<anthropic::Anthropic>(),
    Entries::of::<gemini::Gemini>(),
    Entries::of::<vertex::GeminiVertex>(),
    Entries::of::<mistral::Mistral>(),
    Entries::of::<ollama::Ollama>(),
    Entries::of::<polargrid::PolarGrid>(),
    Entries::of::<openai::OpenAi>(),
    Entries::of::<llamacpp::LlamaCpp>(),
    Entries::of::<deepseek::DeepSeek>(),
    Entries::of::<kimi::Kimi>(),
    Entries::of::<minimax::MiniMax>(),
    Entries::of::<xai::Xai>(),
    Entries::of::<cerebras::Cerebras>(),
    Entries::of::<qwen::Qwen>(),
    Entries::of::<meta::Meta>(),
    Entries::of::<openrouter::OpenRouter>(),
    Entries::of::<litellm::LiteLlm>(),
    Entries::of::<azure::AzureAi>(),
    Entries::of::<vllm::Vllm>(),
];

/// The entries of every image vendor (RFC §25).
pub(crate) static IMAGE_VENDORS: &[ImageEntries] = &[
    ImageEntries::of::<openai_image::OpenAiImage>(),
    ImageEntries::of::<gemini_image::GeminiImage>(),
    ImageEntries::of::<openrouter_image::OpenRouterImage>(),
];
