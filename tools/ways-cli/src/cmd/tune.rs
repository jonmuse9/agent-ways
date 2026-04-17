//! Locale alias fidelity audit (ADR-125).
//!
//! For each way with locale stubs, measure how well each locale alias
//! sits in the same embedding-space neighborhood as its peer aliases.
//! Low fidelity → stub is a poor translation of the same intent; fix
//! by re-authoring, not by adjusting thresholds.
//!
//! Fidelity per (way, lang) = min cosine(this_alias, peer_alias) across
//! all peer locale stubs on the same way. The minimum is the tightest
//! cross-lingual positive: if any peer disagrees, fidelity drops.
//!
//! Parallelized: one way per thread, n_cores - 4 workers.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use walkdir::WalkDir;

use crate::frontmatter;
use crate::util::{home_dir, xdg_cache_dir};

#[derive(Clone)]
struct FidelityResult {
    way_id: String,
    lang: String,
    /// Min cosine against peer aliases on the same way (tightest cross-lingual positive)
    min_peer: f64,
    /// Mean cosine across peer aliases
    mean_peer: f64,
    /// How many peer aliases scored
    peer_count: usize,
}

pub fn run(
    ways_dir: Option<String>,
    way_filter: Option<String>,
    fidelity_threshold: f64,
    json_output: bool,
) -> Result<()> {
    let global_dir = ways_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".claude/hooks/ways"));
    let xdg_way = xdg_cache_dir().join("claude-ways/user");

    let multi_corpus = xdg_way.join("ways-corpus-multi.jsonl");
    let multi_model = xdg_way.join("multilingual-minilm-l12-v2-q8.gguf");

    if !multi_corpus.is_file() {
        bail!("Multilingual corpus not found. Run `ways corpus` first.");
    }
    if !multi_model.is_file() {
        bail!("Multilingual model not found. Run `make setup` first.");
    }

    let embed_bin = find_way_embed()
        .context("way-embed binary not found. Run `make setup` to install.")?;

    let excluded = crate::util::load_excluded_segments();
    let locale_files = collect_locale_files(&global_dir, way_filter.as_deref(), &excluded)?;

    let n_cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    let n_workers = n_cores.saturating_sub(4).max(1);

    eprintln!(
        "Measuring alias fidelity for {} ways across {} threads...",
        locale_files.len(),
        n_workers
    );

    let total = locale_files.len();
    let queue: Arc<Mutex<Vec<(String, PathBuf)>>> = Arc::new(Mutex::new(locale_files));
    let completed: Arc<Mutex<usize>> = Arc::new(Mutex::new(0));
    let failed: Arc<Mutex<usize>> = Arc::new(Mutex::new(0));
    let results: Arc<Mutex<Vec<FidelityResult>>> = Arc::new(Mutex::new(Vec::new()));

    let mut handles = Vec::new();
    for _ in 0..n_workers {
        let queue = Arc::clone(&queue);
        let results = Arc::clone(&results);
        let completed = Arc::clone(&completed);
        let failed = Arc::clone(&failed);
        let embed_bin = embed_bin.clone();
        let multi_corpus = multi_corpus.clone();
        let multi_model = multi_model.clone();

        handles.push(std::thread::spawn(move || loop {
            let item = { queue.lock().unwrap().pop() };
            let (way_id, locale_path) = match item {
                Some(x) => x,
                None => break,
            };

            match measure_way(&way_id, &locale_path, &embed_bin, &multi_corpus, &multi_model) {
                Ok(mut ws) => {
                    let mut r = results.lock().unwrap();
                    r.append(&mut ws);
                }
                Err(e) => {
                    eprintln!("\nERROR measuring {way_id}: {e}");
                    *failed.lock().unwrap() += 1;
                }
            }

            let done = {
                let mut c = completed.lock().unwrap();
                *c += 1;
                *c
            };
            eprint!("\r  {done}/{total} ways");
        }));
    }

    for h in handles {
        h.join().unwrap();
    }
    eprintln!();

    let fail_count = *failed.lock().unwrap();
    if fail_count > 0 {
        eprintln!("WARNING: {fail_count} ways failed to measure");
    }

    let mut all_results = Arc::try_unwrap(results)
        .map_err(|_| anyhow::anyhow!("failed to unwrap results"))?
        .into_inner()
        .unwrap();
    all_results.sort_by(|a, b| a.way_id.cmp(&b.way_id).then(a.lang.cmp(&b.lang)));

    if json_output {
        emit_json(&all_results, fidelity_threshold)?;
    } else {
        emit_report(&all_results, fidelity_threshold);
    }

    Ok(())
}

fn collect_locale_files(
    global_dir: &Path,
    way_filter: Option<&str>,
    excluded: &[String],
) -> Result<Vec<(String, PathBuf)>> {
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    for entry in WalkDir::new(global_dir).follow_links(true).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if !path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with(".locales.jsonl")) {
            continue;
        }
        if crate::util::is_excluded_path(path, excluded) {
            continue;
        }

        let parent = path.parent().unwrap_or(Path::new(""));
        let rel = parent.strip_prefix(global_dir).unwrap_or(parent);
        let way_id = rel.display().to_string();

        if let Some(filter) = way_filter {
            if !way_id.contains(filter) {
                continue;
            }
        }
        files.push((way_id, path.to_path_buf()));
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));

    if files.is_empty() {
        if way_filter.is_some() {
            bail!("No .locales.jsonl files matched filter");
        }
        bail!("No .locales.jsonl files found");
    }
    Ok(files)
}

/// Measure fidelity for every locale alias on a single way.
fn measure_way(
    way_id: &str,
    locale_path: &Path,
    embed_bin: &Path,
    multi_corpus: &Path,
    multi_model: &Path,
) -> Result<Vec<FidelityResult>> {
    let entries: Vec<frontmatter::LocaleEntry> = frontmatter::parse_locales_jsonl(locale_path)?
        .into_iter()
        .filter(|e| crate::agents::is_language_active(&e.lang))
        .collect();

    let mut out = Vec::with_capacity(entries.len());

    for entry in &entries {
        let query = format!(
            "{} {}",
            entry.description,
            entry.vocabulary.as_deref().unwrap_or("")
        );

        let output = Command::new(embed_bin)
            .args([
                "match",
                "--corpus", multi_corpus.to_str().unwrap(),
                "--model", multi_model.to_str().unwrap(),
                "--query", &query,
                "--threshold", "0.0",
            ])
            .output()
            .with_context(|| format!("way-embed match for {way_id}/{lang}", lang = entry.lang))?;

        if !output.status.success() {
            continue;
        }

        // Collect scores of peer aliases on the same way (excluding self).
        let mut peer_scores: Vec<f64> = Vec::new();
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let mut parts = line.split('\t');
            let id = match parts.next() {
                Some(s) => s,
                None => continue,
            };
            let score: f64 = match parts.next().and_then(|s| s.parse().ok()) {
                Some(s) => s,
                None => continue,
            };
            if id != way_id {
                continue;
            }
            // This is a peer alias entry on the same way.
            // Skip self-match: the query embedding is ~1.0 against its own corpus row.
            if score > 0.999 {
                continue;
            }
            peer_scores.push(score);
        }

        let (min_peer, mean_peer) = if peer_scores.is_empty() {
            (f64::NAN, f64::NAN)
        } else {
            let min = peer_scores.iter().cloned().fold(f64::INFINITY, f64::min);
            let mean = peer_scores.iter().sum::<f64>() / peer_scores.len() as f64;
            (min, mean)
        };

        out.push(FidelityResult {
            way_id: way_id.to_string(),
            lang: entry.lang.clone(),
            min_peer,
            mean_peer,
            peer_count: peer_scores.len(),
        });
    }

    Ok(out)
}

fn emit_report(results: &[FidelityResult], fidelity_threshold: f64) {
    use agent_fmt::{Align, Table};

    let low: Vec<&FidelityResult> = results
        .iter()
        .filter(|r| r.peer_count > 0 && r.min_peer < fidelity_threshold)
        .collect();

    println!("Locale Alias Fidelity");
    println!("=====================");
    println!();

    let total = results.iter().filter(|r| r.peer_count > 0).count();
    println!(
        "{}/{} entries below fidelity threshold {:.2}",
        low.len(),
        total,
        fidelity_threshold
    );
    println!();
    println!("Fidelity = min cosine against peer aliases on the same way (multilingual embedding).");
    println!("Low fidelity means the locale stub embeds far from its peers — re-author the stub.");
    println!();

    if low.is_empty() {
        println!("All aliases clear threshold. No re-authoring needed.");
        return;
    }

    let mut t = Table::new(&["Way", "Lang", "Min peer", "Mean peer", "Peers"]);
    t.align(2, Align::Right);
    t.align(3, Align::Right);
    t.align(4, Align::Right);

    for r in &low {
        t.add_owned(vec![
            r.way_id.clone(),
            r.lang.clone(),
            format!("{:.4}", r.min_peer),
            format!("{:.4}", r.mean_peer),
            format!("{}", r.peer_count),
        ]);
    }

    t.print();
    println!();
}

fn emit_json(results: &[FidelityResult], fidelity_threshold: f64) -> Result<()> {
    let rows: Vec<serde_json::Value> = results
        .iter()
        .map(|r| {
            serde_json::json!({
                "way": r.way_id,
                "lang": r.lang,
                "min_peer": nan_to_null(r.min_peer),
                "mean_peer": nan_to_null(r.mean_peer),
                "peer_count": r.peer_count,
                "below_threshold": r.peer_count > 0 && r.min_peer < fidelity_threshold,
            })
        })
        .collect();
    let out = serde_json::json!({
        "fidelity_threshold": fidelity_threshold,
        "entries": rows,
    });
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}

fn nan_to_null(x: f64) -> serde_json::Value {
    if x.is_nan() {
        serde_json::Value::Null
    } else {
        serde_json::json!(x)
    }
}

fn find_way_embed() -> Option<PathBuf> {
    let xdg = xdg_cache_dir().join("claude-ways/user/way-embed");
    if xdg.is_file() {
        return Some(xdg);
    }
    let bin = home_dir().join(".claude/bin/way-embed");
    if bin.is_file() {
        return Some(bin);
    }
    None
}
