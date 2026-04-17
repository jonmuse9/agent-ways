use anyhow::Result;
use std::path::PathBuf;

use agent_fmt::{Align, Table};
use crate::util::xdg_cache_dir;

/// Embedding-only match (ADR-125).
pub fn run(query: String, corpus: Option<String>) -> Result<()> {
    let corpus_path = corpus
        .unwrap_or_else(|| default_corpus_path().to_string_lossy().to_string());

    let embed_results = super::scan::batch_embed_score(&query);

    let Some(results) = embed_results else {
        eprintln!("ERROR: embedding engine unavailable.");
        eprintln!("       Run: cd ~/.claude && make setup");
        std::process::exit(1);
    };

    if results.is_empty() {
        eprintln!("no matches above threshold");
        std::process::exit(1);
    }

    let mut best: std::collections::HashMap<&str, f64> = std::collections::HashMap::new();
    for (id, score) in &results {
        let entry = best.entry(id.as_str()).or_insert(0.0);
        if *score > *entry {
            *entry = *score;
        }
    }
    let mut scored: Vec<(&&str, &f64)> = best.iter().collect();
    scored.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));

    let en_corpus = xdg_cache_dir().join("claude-ways/user/ways-corpus-en.jsonl");
    let descriptions = load_descriptions(en_corpus.to_str().unwrap_or(&corpus_path));

    let mut t = Table::new(&["Way", "Score", "Description"]);
    t.align(1, Align::Right);
    t.max_width(0, 38);
    t.max_width(2, 52);

    for (id, score) in scored {
        let desc = descriptions.get(*id).cloned().unwrap_or_default();
        t.add_owned(vec![
            id.to_string(),
            format!("{score:.4}"),
            desc,
        ]);
    }

    println!();
    t.print();
    println!();
    Ok(())
}

fn load_descriptions(corpus_path: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    if let Ok(content) = std::fs::read_to_string(corpus_path) {
        for line in content.lines() {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                if let (Some(id), Some(desc)) = (
                    v.get("id").and_then(|v| v.as_str()),
                    v.get("description").and_then(|v| v.as_str()),
                ) {
                    map.entry(id.to_string()).or_insert_with(|| desc.to_string());
                }
            }
        }
    }
    map
}

fn default_corpus_path() -> PathBuf {
    xdg_cache_dir().join("claude-ways/user/ways-corpus.jsonl")
}
