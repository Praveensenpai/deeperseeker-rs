//! Token counting backed by the real DeepSeek tokenizer, with a
//! character-based heuristic fallback when the tokenizer asset is missing.

use crate::domain::openai::ChatMessage;
use crate::infra::assets::resolve_asset_dir;
use std::sync::Arc;
use tokenizers::Tokenizer as HfTokenizer;

/// Approximate characters per token used by the fallback counter.
const CHARS_PER_TOKEN: usize = 4;

/// DeepSeek token counter. Loaded once at startup and shared across requests.
pub struct Tokenizer {
    inner: Option<HfTokenizer>,
}

impl Tokenizer {
    /// Load `assets/tokenizer.json`. Falls back to the heuristic counter when
    /// the asset is absent or malformed so the server always starts.
    pub fn load() -> Arc<Self> {
        match Self::try_load() {
            Ok(tokenizer) => {
                tracing::info!("Loaded DeepSeek tokenizer from assets/tokenizer.json");
                Arc::new(tokenizer)
            }
            Err(e) => {
                tracing::warn!("Tokenizer asset unavailable ({e:#}); using heuristic counter");
                Arc::new(Self { inner: None })
            }
        }
    }

    fn try_load() -> anyhow::Result<Self> {
        let path = resolve_asset_dir("assets/tokenizer.json");
        let inner = HfTokenizer::from_file(&path)
            .map_err(|e| anyhow::anyhow!("failed to load {}: {e}", path.display()))?;
        Ok(Self { inner: Some(inner) })
    }

    /// Count tokens in `text`, returning 0 for empty input.
    pub fn count(&self, text: &str) -> u32 {
        if text.is_empty() {
            return 0;
        }
        match &self.inner {
            Some(tokenizer) => tokenizer
                .encode(text, false)
                .map(|encoding| encoding.get_ids().len() as u32)
                .unwrap_or_else(|_| heuristic(text)),
            None => heuristic(text),
        }
    }

    /// Count tokens, never returning less than 1 for non-empty text.
    pub fn count_min_one(&self, text: &str) -> u32 {
        self.count(text).max(1)
    }

    /// True when the real tokenizer is loaded (not the heuristic fallback).
    pub fn is_exact(&self) -> bool {
        self.inner.is_some()
    }
}

/// Tokenize every message's text content and sum the result (min 1).
pub fn count_message_tokens(tokenizer: &Tokenizer, messages: &[ChatMessage]) -> u32 {
    let total: u32 = messages
        .iter()
        .map(|m| tokenizer.count(&m.text_content()))
        .sum();
    total.max(1)
}

fn heuristic(text: &str) -> u32 {
    let chars = text.chars().count();
    std::cmp::max(1, (chars / CHARS_PER_TOKEN) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heuristic_counts_chars_over_four() {
        assert_eq!(heuristic("abcdefgh"), 2);
        assert_eq!(heuristic("a"), 1);
        assert_eq!(heuristic(""), 1);
    }
}
