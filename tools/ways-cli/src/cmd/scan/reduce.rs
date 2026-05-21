//! Sentence-salience input reduction for embed matching (ADR-130).
//!
//! When a hook payload exceeds the embedding model's working window (MiniLM's
//! 128 position embeddings), we'd previously SIGABRT inside way-embed. The
//! interim fix capped inputs by character count in the hook scripts (PRs
//! #94/#95/#96); this module replaces that with a structural fix that keeps
//! the *high-signal* prose instead of clamping at an arbitrary char index.
//!
//! Algorithm: split into sentences → softmax over token frequencies across
//! the whole input → score each sentence by average softmax weight of its
//! tokens → keep top-N sentences within a token budget → reassemble in
//! document order. Inputs that already fit pass through unchanged.
//!
//! Failure modes are deliberate:
//! - Non-prose input (one sentence detected) → fall back to bag-of-words
//!   top-K selection on tokens directly (the Shape A path from the design
//!   conversation).
//! - Any internal error → return the input unchanged. The truncation caps
//!   in the hook scripts are the belt-and-suspenders fallback.
//!
//! No corpus involvement here. This is input preparation only; ADR-125's
//! embedding-only matcher contract is untouched.

use std::collections::HashMap;

/// Approximate-tokens-per-char ratio used for sizing. MiniLM's WordPiece
/// segments ~4 chars/token on English prose; we round down for headroom.
const CHARS_PER_TOKEN: usize = 4;

/// Minimal scaffold-aware stopword list. Standard English fillers plus
/// the words that dominate agent dispatch prompts in observed crash
/// payloads (`agent`, `task`, `prompt`, `context`, etc.). Kept small;
/// grows empirically based on observed scaffold dominance.
const STOPWORDS: &[&str] = &[
    // Standard English
    "the", "a", "an", "and", "or", "but", "is", "are", "was", "were", "be",
    "been", "being", "have", "has", "had", "do", "does", "did", "will",
    "would", "should", "could", "may", "might", "must", "can", "to", "of",
    "in", "on", "at", "by", "for", "with", "as", "from", "into", "about",
    "this", "that", "these", "those", "it", "its", "if", "then", "than",
    "you", "your", "we", "our", "they", "their", "i", "me", "my", "not",
    "no", "yes", "so", "up", "down", "out", "all", "any", "some", "each",
    "such", "only", "own", "same", "other", "more", "most", "very", "just",
    // Scaffold tokens common in agent dispatch / hook prompts
    "agent", "task", "prompt", "context", "return", "description",
    "subagent", "tool", "tools", "use", "using", "user", "claude",
    "session", "project", "file", "files", "code", "please",
];

/// Reduce `input` to fit within `budget_tokens` approximate tokens, using
/// sentence-salience scoring when possible. Returns the input unchanged
/// when it already fits or when reduction can't be applied meaningfully.
pub fn reduce_for_embed(input: &str, budget_tokens: usize) -> String {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    if approx_tokens(trimmed) <= budget_tokens {
        return trimmed.to_string();
    }

    let sentences = split_sentences(trimmed);

    // Non-prose fallback: degrade to bag-of-words token selection.
    if sentences.len() <= 1 {
        return reduce_by_tokens(trimmed, budget_tokens);
    }

    reduce_by_sentences(trimmed, &sentences, budget_tokens)
}

fn approx_tokens(s: &str) -> usize {
    // Cheap upper bound: count whitespace-separated runs, but also clamp
    // against char-budget for inputs without spaces (e.g. concatenated
    // identifiers). Choose the larger of the two estimates so we never
    // under-budget.
    let by_words = s.split_whitespace().count();
    let by_chars = s.chars().count() / CHARS_PER_TOKEN;
    by_words.max(by_chars)
}

fn tokenize(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
        .filter(|t| !t.is_empty())
        .map(|t| t.to_lowercase())
        .filter(|t| !STOPWORDS.contains(&t.as_str()))
        .collect()
}

/// Split on sentence-ending punctuation followed by whitespace, plus
/// double-newline boundaries. Cheap and good enough for prose; falls
/// through to single-element on code/JSON/heredoc inputs (which the
/// caller handles via the fallback path).
fn split_sentences(s: &str) -> Vec<&str> {
    let mut sentences = Vec::new();
    let bytes = s.as_bytes();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        let is_sentence_end = (c == b'.' || c == b'!' || c == b'?')
            && i + 1 < bytes.len()
            && bytes[i + 1].is_ascii_whitespace();
        let is_paragraph_break = c == b'\n'
            && i + 1 < bytes.len()
            && bytes[i + 1] == b'\n';
        if is_sentence_end || is_paragraph_break {
            let end = if is_paragraph_break { i } else { i + 1 };
            let chunk = s[start..end].trim();
            if !chunk.is_empty() {
                sentences.push(chunk);
            }
            start = end + 1;
            i += if is_paragraph_break { 2 } else { 1 };
            continue;
        }
        i += 1;
    }
    let tail = s[start..].trim();
    if !tail.is_empty() {
        sentences.push(tail);
    }
    sentences
}

fn softmax(counts: &HashMap<String, f64>) -> HashMap<String, f64> {
    if counts.is_empty() {
        return HashMap::new();
    }
    // Subtract max for numerical stability — standard softmax trick.
    let max = counts.values().cloned().fold(f64::NEG_INFINITY, f64::max);
    let exps: HashMap<String, f64> = counts
        .iter()
        .map(|(k, v)| (k.clone(), (v - max).exp()))
        .collect();
    let sum: f64 = exps.values().sum();
    if sum == 0.0 {
        return HashMap::new();
    }
    exps.into_iter().map(|(k, v)| (k, v / sum)).collect()
}

fn count_frequencies(tokens: &[String]) -> HashMap<String, f64> {
    let mut counts: HashMap<String, f64> = HashMap::new();
    for t in tokens {
        *counts.entry(t.clone()).or_insert(0.0) += 1.0;
    }
    counts
}

fn reduce_by_sentences(input: &str, sentences: &[&str], budget_tokens: usize) -> String {
    let all_tokens = tokenize(input);
    let counts = count_frequencies(&all_tokens);
    let weights = softmax(&counts);

    let mut scored: Vec<(usize, &str, f64)> = sentences
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let tokens = tokenize(s);
            let n = tokens.len().max(1) as f64;
            let salience: f64 = tokens.iter().filter_map(|t| weights.get(t)).sum::<f64>() / n;
            (i, *s, salience)
        })
        .collect();

    // Descending salience for selection.
    scored.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

    let mut keep: Vec<(usize, &str)> = Vec::new();
    let mut accumulated = 0usize;
    for (i, s, _) in &scored {
        let s_tokens = approx_tokens(s);
        if accumulated + s_tokens > budget_tokens {
            continue;
        }
        keep.push((*i, *s));
        accumulated += s_tokens;
    }

    // If nothing fit (single sentence longer than budget), degrade to token
    // selection on the highest-scored sentence rather than emitting empty.
    if keep.is_empty() {
        let best = scored.first().map(|(_, s, _)| *s).unwrap_or(input);
        return reduce_by_tokens(best, budget_tokens);
    }

    // Document order restores prose coherence for the embedder.
    keep.sort_by_key(|(i, _)| *i);
    keep.into_iter()
        .map(|(_, s)| s)
        .collect::<Vec<_>>()
        .join(" ")
}

fn reduce_by_tokens(input: &str, budget_tokens: usize) -> String {
    let tokens = tokenize(input);
    if tokens.is_empty() {
        // Pathological: nothing tokenizable left after stopwords. Fall
        // back to a hard char-budget slice so we never return empty on
        // non-empty input.
        let max_chars = budget_tokens * CHARS_PER_TOKEN;
        return input.chars().take(max_chars).collect();
    }
    let counts = count_frequencies(&tokens);
    let weights = softmax(&counts);

    // Walk tokens in document order, keep the highest-weight ones up to
    // budget. This preserves local ordering (not perfect — duplicates of
    // a high-weight token all survive — but it keeps the embed input
    // looking like a phrase rather than a sorted bag).
    let mut indexed: Vec<(usize, &String)> = tokens.iter().enumerate().collect();
    indexed.sort_by(|a, b| {
        let wa = weights.get(a.1).copied().unwrap_or(0.0);
        let wb = weights.get(b.1).copied().unwrap_or(0.0);
        wb.partial_cmp(&wa).unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut keep_indices: Vec<usize> = indexed
        .into_iter()
        .take(budget_tokens)
        .map(|(i, _)| i)
        .collect();
    keep_indices.sort_unstable();

    keep_indices
        .into_iter()
        .map(|i| tokens[i].as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passthrough_when_under_budget() {
        let input = "short user question about the codebase";
        let out = reduce_for_embed(input, 100);
        assert_eq!(out, input);
    }

    #[test]
    fn empty_input_returns_empty() {
        assert_eq!(reduce_for_embed("", 100), "");
        assert_eq!(reduce_for_embed("   \n\t  ", 100), "");
    }

    #[test]
    fn prose_reduces_to_high_salience_sentences() {
        // Repeated reference to "deploy" should make the deployment
        // sentences more salient than the unrelated ones.
        let input = "We need to deploy the new build. \
                     My cat is asleep on the keyboard. \
                     The deploy pipeline runs on staging first. \
                     Coffee is good in the morning. \
                     Each deploy step is logged. \
                     There is a thunderstorm tonight.";
        let out = reduce_for_embed(input, 20);
        // Output must be shorter than input
        assert!(out.len() < input.len());
        // Salient term must survive
        assert!(out.to_lowercase().contains("deploy"));
        // At least one unrelated sentence should be dropped
        let unrelated_count = ["cat", "coffee", "thunderstorm"]
            .iter()
            .filter(|w| out.contains(*w))
            .count();
        assert!(unrelated_count < 3, "expected some unrelated sentences dropped; got: {out}");
    }

    #[test]
    fn document_order_preserved() {
        let input = "Alpha sentence about deploy. \
                     Beta sentence is unrelated. \
                     Gamma sentence also about deploy and build. \
                     Delta sentence is unrelated. \
                     Epsilon sentence about deploy pipeline.";
        let out = reduce_for_embed(input, 30);
        let alpha = out.find("Alpha");
        let gamma = out.find("Gamma");
        let epsilon = out.find("Epsilon");
        if let (Some(a), Some(g)) = (alpha, gamma) {
            assert!(a < g, "Alpha must come before Gamma in output");
        }
        if let (Some(g), Some(e)) = (gamma, epsilon) {
            assert!(g < e, "Gamma must come before Epsilon in output");
        }
    }

    #[test]
    fn non_prose_input_falls_back_to_token_selection() {
        // A bash heredoc-style payload — no sentence boundaries. The
        // fallback's contract is "bound the size, provide some signal" —
        // not "extract topicality from a structureless blob," which
        // bag-of-words cannot promise without repetition of the
        // topical terms themselves.
        let input = "gh pr create --title \"fix(scope): description\" \
                     --body \"$(cat <<EOF this is a very long body \
                     without any sentence punctuation at all just words \
                     and more words about the deploy pipeline staging \
                     production rollout EOF)\"";
        let out = reduce_for_embed(input, 15);
        assert!(out.len() < input.len(), "should be reduced");
        assert!(!out.is_empty(), "fallback must not return empty");
        assert!(approx_tokens(&out) <= 30, "should respect budget approximately");
    }

    #[test]
    fn non_prose_with_repeated_topical_terms_keeps_them() {
        // When topical terms ARE the repeated content (which is what
        // happens in real bash commands — `gh pr create` repeated across
        // a session, or `deploy` appearing in multiple flags), the
        // bag-of-words fallback does the right thing.
        let input = "deploy deploy deploy deploy build build build build \
                     scaffold scaffold scaffold scaffold scaffold";
        let out = reduce_for_embed(input, 5);
        assert!(out.contains("deploy") || out.contains("build") || out.contains("scaffold"),
                "expected repeated term to survive: {out}");
    }

    #[test]
    fn budget_is_respected() {
        let input = "Repeating the deploy. Repeating the deploy. \
                     Repeating the deploy. Repeating the deploy. \
                     Repeating the deploy. Repeating the deploy.";
        let out = reduce_for_embed(input, 5);
        assert!(approx_tokens(&out) <= 10, "expected ≤10 tokens, got {} from: {out}", approx_tokens(&out));
    }

    #[test]
    fn single_long_sentence_does_not_return_empty() {
        // One sentence longer than budget — must degrade to token
        // selection on it rather than returning empty.
        let input = "this is a single very long sentence about deploying the pipeline to staging and then to production with all the various steps that happen along the way to make sure everything goes smoothly without any incidents";
        let out = reduce_for_embed(input, 10);
        assert!(!out.is_empty());
        assert!(approx_tokens(&out) <= 20);
    }

    #[test]
    fn stopwords_filtered_from_tokenization() {
        let tokens = tokenize("the agent will return the prompt context");
        // All of those are in STOPWORDS — should be empty after filter.
        assert!(tokens.is_empty(), "expected all stopwords filtered, got: {tokens:?}");
    }

    #[test]
    fn tokenize_handles_punctuation_and_case() {
        let tokens = tokenize("Deploy-Pipeline, staging/production: rollout!");
        // Stopwords removed; hyphen and underscore preserved inside identifiers.
        assert!(tokens.contains(&"deploy-pipeline".to_string()));
        assert!(tokens.contains(&"staging".to_string()));
        assert!(tokens.contains(&"production".to_string()));
        assert!(tokens.contains(&"rollout".to_string()));
    }

    #[test]
    fn split_sentences_basic() {
        let sentences = split_sentences("First sentence. Second sentence! Third one?");
        assert_eq!(sentences.len(), 3);
    }

    #[test]
    fn split_sentences_paragraph_break() {
        let sentences = split_sentences("First paragraph here\n\nSecond paragraph now");
        assert_eq!(sentences.len(), 2);
    }

    #[test]
    fn split_sentences_returns_one_on_non_prose() {
        let sentences = split_sentences("no-punctuation-and-no-paragraph-breaks");
        assert_eq!(sentences.len(), 1);
    }

    #[test]
    fn softmax_distribution_sums_to_one() {
        let mut counts = HashMap::new();
        counts.insert("a".to_string(), 3.0);
        counts.insert("b".to_string(), 1.0);
        counts.insert("c".to_string(), 1.0);
        let weights = softmax(&counts);
        let sum: f64 = weights.values().sum();
        assert!((sum - 1.0).abs() < 1e-9, "softmax should sum to 1.0, got {sum}");
        // High count gets high weight.
        assert!(weights["a"] > weights["b"]);
    }
}
