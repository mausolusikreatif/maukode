//! Token prices and the cost of a session's usage.
//!
//! Built-in prices cover Anthropic's current Claude models (first-party API rates, October 2026).
//! Other models, or different rates (a gateway, Bedrock, Vertex AI), are set per model in the
//! configuration's `models.<id>.pricing`. Without a price only token counts are shown.

use maukode_protocol::TokenUsage;
use serde::{Deserialize, Serialize};

/// US dollars per million tokens.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ModelPricing {
    pub input: f64,
    pub output: f64,
    /// Cached input read back; 10% of `input` when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read: Option<f64>,
    /// Input written to the cache (5-minute TTL); 125% of `input` when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write: Option<f64>,
}

const fn price(input: f64, output: f64, cache_read: f64) -> ModelPricing {
    ModelPricing {
        input,
        output,
        cache_read: Some(cache_read),
        cache_write: Some(input * 1.25),
    }
}

/// Anthropic first-party prices, by model id.
const CLAUDE_PRICES: &[(&str, ModelPricing)] = &[
    ("claude-fable-5-1", price(10.0, 50.0, 0.25)),
    ("claude-mythos-5-1", price(10.0, 50.0, 0.25)),
    ("claude-fable-5", price(10.0, 50.0, 1.0)),
    ("claude-opus-5-5", price(4.0, 20.0, 0.20)),
    ("claude-opus-5", price(5.0, 25.0, 0.50)),
    ("claude-opus-4-8", price(5.0, 25.0, 0.50)),
    ("claude-opus-4-7", price(5.0, 25.0, 0.50)),
    ("claude-opus-4-6", price(5.0, 25.0, 0.50)),
    ("claude-sonnet-5-5", price(2.0, 10.0, 0.20)),
    ("claude-sonnet-5", price(2.0, 10.0, 0.20)),
    ("claude-sonnet-4-6", price(3.0, 15.0, 0.30)),
    ("claude-haiku-5-5", price(0.10, 0.50, 0.01)),
    ("claude-haiku-4-5", price(1.0, 5.0, 0.10)),
];

/// Claude Haiku 5.5 costs more for prompts above this size.
const HAIKU_5_5_LONG_PROMPT: u64 = 100_000;
const HAIKU_5_5_LONG: ModelPricing = price(0.50, 2.50, 0.05);

/// The built-in price of `model`, accepting gateway prefixes (`anthropic/claude-…`) and dated
/// snapshots (`claude-haiku-4-5-20251001`).
pub fn builtin_pricing(model: &str) -> Option<ModelPricing> {
    let id = model.rsplit('/').next().unwrap_or(model);
    CLAUDE_PRICES.iter().find_map(|(name, pricing)| {
        let rest = id.strip_prefix(name)?;
        let dated = rest
            .strip_prefix('-')
            .is_some_and(|date| date.len() == 8 && date.chars().all(|c| c.is_ascii_digit()));
        (rest.is_empty() || dated).then_some(*pricing)
    })
}

/// What one response cost, in US dollars. `TokenUsage::input_tokens` includes the cached part.
pub fn cost(pricing: &ModelPricing, model: &str, usage: &TokenUsage) -> f64 {
    let pricing = if builtin_pricing(model) == Some(*pricing)
        && model.contains("claude-haiku-5-5")
        && usage.input_tokens > HAIKU_5_5_LONG_PROMPT
    {
        &HAIKU_5_5_LONG
    } else {
        pricing
    };
    let read = usage.cache_read_tokens.unwrap_or(0);
    let write = usage.cache_write_tokens.unwrap_or(0);
    let uncached = usage.input_tokens.saturating_sub(read + write);
    let per_token = |tokens: u64, dollars_per_million: f64| tokens as f64 * dollars_per_million;
    (per_token(uncached, pricing.input)
        + per_token(read, pricing.cache_read.unwrap_or(pricing.input * 0.1))
        + per_token(write, pricing.cache_write.unwrap_or(pricing.input * 1.25))
        + per_token(usage.output_tokens, pricing.output))
        / 1_000_000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_prices_match_ids_snapshots_and_gateway_prefixes() {
        assert_eq!(builtin_pricing("claude-opus-5-5").map(|p| p.input), Some(4.0));
        // `claude-opus-5` must not take the price of `claude-opus-5-5`, or the reverse.
        assert_eq!(builtin_pricing("claude-opus-5").map(|p| p.input), Some(5.0));
        assert_eq!(
            builtin_pricing("claude-haiku-4-5-20251001").map(|p| p.output),
            Some(5.0)
        );
        assert_eq!(
            builtin_pricing("anthropic/claude-sonnet-5-5").map(|p| p.input),
            Some(2.0)
        );
        assert!(builtin_pricing("claude-opus-5-5-preview").is_none());
        assert!(builtin_pricing("gpt-5").is_none());
    }

    #[test]
    fn cost_counts_cached_input_at_cache_rates() {
        let pricing = builtin_pricing("claude-opus-5-5").unwrap();
        let usage = TokenUsage {
            input_tokens: 1_000_000,
            output_tokens: 100_000,
            cache_read_tokens: Some(600_000),
            cache_write_tokens: Some(200_000),
        };
        // 200k uncached at $4, 600k read at $0.20, 200k written at $5, 100k out at $20.
        let expected = 0.8 + 0.12 + 1.0 + 2.0;
        assert!((cost(&pricing, "claude-opus-5-5", &usage) - expected).abs() < 1e-9);
    }

    #[test]
    fn long_haiku_prompts_use_the_higher_rate() {
        let pricing = builtin_pricing("claude-haiku-5-5").unwrap();
        let usage = |input_tokens| TokenUsage {
            input_tokens,
            output_tokens: 0,
            cache_read_tokens: None,
            cache_write_tokens: None,
        };
        assert!((cost(&pricing, "claude-haiku-5-5", &usage(100_000)) - 0.01).abs() < 1e-9);
        assert!((cost(&pricing, "claude-haiku-5-5", &usage(200_000)) - 0.1).abs() < 1e-9);
    }
}
