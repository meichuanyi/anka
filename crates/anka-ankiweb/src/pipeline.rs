//! The Anka ↔ AnkiWeb sync pipeline — single source of truth.
//!
//! Flow: bootstrap (first run pulls AnkiWeb down) → merge down (dedup by
//! anki_id_map) → push unmapped Anka notes into the agent collection →
//! two-way sync with AnkiWeb → record mappings → scheduling + content
//! merge down again → media copy.
//!
//! Used by the CLI, the desktop app (IPC) and the web server endpoint,
//! so all three surfaces always behave identically.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use anka_core::Collection;

#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct PipelineReport {
    pub pushed: usize,
    pub sched_updated: u32,
    pub sched_skipped: u32,
    pub notes_created: u32,
    pub notes_updated: u32,
    pub cards: u32,
    pub media_copied: u32,
}

/// Run the full sync pipeline.
///
/// * `agent_bin` — path to the `anka-sync-agent` binary
/// * `agent_path` — hidden Anki-format agent collection (created on bootstrap)
/// * `media_dir` — Anka's media directory (agent media is copied into it)
/// * `col` — the Anka collection being synced
/// * `hkey` — AnkiWeb session key (from `login`)
pub fn run_pipeline(
    agent_bin: &Path,
    agent_path: &Path,
    media_dir: &Path,
    col: &mut Collection,
    hkey: &str,
) -> Result<PipelineReport> {
    run_pipeline_with_progress(agent_bin, agent_path, media_dir, col, hkey, &|_, _| {})
}

/// Same pipeline, reporting (percent, phase message) through `on_progress`.
pub fn run_pipeline_with_progress(
    agent_bin: &Path,
    agent_path: &Path,
    media_dir: &Path,
    col: &mut Collection,
    hkey: &str,
    on_progress: &(dyn Fn(u8, &str) + Sync),
) -> Result<PipelineReport> {
    let to_str = |p: &Path| p.to_string_lossy().as_ref().to_string();
    let mut report = PipelineReport::default();

    let run_agent = |args: &[&str]| -> Result<String> {
        let output = std::process::Command::new(agent_bin)
            .args(args)
            .output()
            .map_err(|e| anyhow::anyhow!("anka-sync-agent 未找到: {e}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!(
                if stderr.trim().is_empty() {
                    "同步代理执行失败".to_string()
                } else {
                    stderr.trim().to_string()
                }
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    };

    on_progress(5, "准备同步…");
    // 1. bootstrap: first run replaces the empty agent collection with the
    //    AnkiWeb copy (establishes sync state for subsequent normal_syncs).
    if !agent_path.exists() {
        on_progress(10, "首次同步：下载 AnkiWeb 收藏…");
        run_agent(&["pull", "--agent", &to_str(agent_path), "--hkey", hkey])?;
    }

    on_progress(30, "合并 AnkiWeb 变更…");
    // 2. merge down: AnkiWeb copy -> Anka (dedup by anki_id_map)
    {
        crate::merge_agent_into_collection(agent_path, col)?;
    }

    // 3. collect unmapped Anka notes -> batch
    let mut batch: Vec<serde_json::Value> = Vec::new();
    let mut pairs: Vec<(anka_core::Id, anka_core::Id)> = Vec::new();
    {
        let mapped: std::collections::HashSet<String> =
            col.mapped_note_ids()?.into_iter().collect();
        let all_notes = col.all_notes()?;
        let all_cards = col.all_cards()?;
        let decks_list = col.list_decks()?;
        for note in &all_notes {
            if mapped.contains(&note.id.to_string()) {
                continue;
            }
            let deck_name = decks_list
                .iter()
                .find(|d| d.id == note.deck_id)
                .map(|d| d.name.clone())
                .unwrap_or_else(|| "Default".into());
            let first_card = all_cards.iter().find(|c| c.note_id == note.id).map(|c| c.id);
            let (front, back) = anka_core::front_back(&note.fields);
            batch.push(serde_json::json!({
                "deck": deck_name,
                "fields": [front, back],
                "tags": note.tags,
            }));
            if let Some(card_id) = first_card {
                pairs.push((note.id, card_id));
            }
        }
    }

    on_progress(45, "收集并上传本机新笔记…");
    // 4. push batch into the agent collection + two-way sync with AnkiWeb
    let batch_file = agent_path.with_extension("batch.json");
    let map_file = agent_path.with_extension("map.json");
    if !batch.is_empty() {
        std::fs::write(&batch_file, serde_json::to_vec(&batch)?)?;
        run_agent(&[
            "add-batch",
            "--agent",
            &to_str(agent_path),
            "--file",
            &to_str(&batch_file),
            "--map-out",
            &to_str(&map_file),
        ])?;
        let _ = std::fs::remove_file(&batch_file);

        // record mappings — failing here must be loud, or the next run
        // would push the same notes again (duplicate cascade)
        let map_raw = std::fs::read_to_string(&map_file)
            .map_err(|e| anyhow::anyhow!("读取 map-out 失败: {e}"))?;
        let entries: Vec<serde_json::Value> =
            serde_json::from_str(&map_raw).context("解析 map-out 失败")?;
        for entry in &entries {
            let idx = entry["index"].as_u64().unwrap_or(0) as usize;
            if let Some((note_id, card_id)) = pairs.get(idx) {
                if let Some(anki_note) = entry["noteId"].as_i64() {
                    col.put_anki_id("note", &anki_note.to_string(), *note_id)?;
                }
                if let Some(anki_card) = entry["cardIds"].as_array().and_then(|a| a.first()) {
                    if let Some(c) = anki_card.as_i64() {
                        col.put_anki_id("card", &c.to_string(), *card_id)?;
                    }
                }
            }
        }
        let _ = std::fs::remove_file(&map_file);
        report.pushed = pairs.len();

        on_progress(70, "与 AnkiWeb 双向同步…");
        run_agent(&["sync", "--agent", &to_str(agent_path), "--hkey", hkey])?;
    }

    on_progress(85, "合并调度状态与内容…");
    // 5. scheduling state (download direction, newer-wins) + final content merge
    {
        let (su, sk) = crate::merge_scheduling_from_agent(agent_path, col)?;
        report.sched_updated = su;
        report.sched_skipped = sk;
        let r = crate::merge_agent_into_collection(agent_path, col)?;
        report.notes_created = r.notes;
        report.notes_updated = r.notes_updated;
        report.cards = r.cards;
    }

    // 6. media: copy new files from the agent media dir (name-based)
    if let Some(agent_media) = agent_path.parent().map(|p| p.join("sync-agent.media")) {
        if agent_media.is_dir() {
            std::fs::create_dir_all(media_dir)?;
            for entry in std::fs::read_dir(&agent_media)?.flatten() {
                let target = media_dir.join(entry.file_name());
                if !target.exists() {
                    std::fs::copy(entry.path(), &target)?;
                    report.media_copied += 1;
                }
            }
        }
    }

    on_progress(100, "同步完成");
    Ok(report)
}
