//! AnkiWeb one-time migration: login + full collection download.
//!
//! Wire format ported from Anki's AGPL client (`rslib/src/sync`, protocol v11):
//! POST `https://sync.ankiweb.net/sync/<method>` with an `anki-sync` JSON header
//! and a zstd-compressed JSON body. Responses are zstd-compressed as well.
//!
//! Passwords live only in the caller's memory for the duration of the call.

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use std::path::Path;
use serde::Deserialize;
use serde_json::json;
use std::io::Write;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const ENDPOINT: &str = "https://sync.ankiweb.net/";
const SYNC_VERSION: u8 = 11;
const CLIENT_VERSION: &str = concat!("anka,", env!("CARGO_PKG_VERSION"));

fn http_client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .user_agent(concat!("anka/", env!("CARGO_PKG_VERSION")))
        // AnkiWeb answers with its own Location-based host redirect; we retry manually
        .redirect(reqwest::redirect::Policy::none())
        // The load balancer pins the session to a concrete sync host via cookies
        .cookie_store(true)
        .timeout(Duration::from_secs(600))
        .build()
        .expect("reqwest client")
}

/// Short random tag so AnkiWeb can tell concurrent sessions apart.
fn session_id() -> String {
    const TABLE: &[u8; 62] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .subsec_nanos() as u64
        | ((std::process::id() as u64) << 32);
    let mut out = Vec::new();
    while n > 0 {
        out.push(TABLE[(n % 62) as usize]);
        n /= 62;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A session with AnkiWeb: one HTTP client (cookies preserved), one base
/// endpoint that follows load-balancer redirects, one hkey.
pub struct AnkiWebClient {
    client: reqwest::blocking::Client,
    base: String,
    hkey: String,
}

impl AnkiWebClient {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: http_client(),
            base: ENDPOINT.to_string(),
            hkey: String::new(),
        })
    }

    /// Authenticate against AnkiWeb, storing (and returning) the sync hkey.
    pub fn login(&mut self, username: &str, password: &str) -> Result<String> {
        #[derive(Deserialize)]
        struct HostKey {
            key: String,
        }
        let payload = json!({ "u": username, "p": password }).to_string().into_bytes();
        let body = self.call("hostKey", &payload)?;
        let hk: HostKey = serde_json::from_slice(&body).context("解析 hostKey 响应失败")?;
        if hk.key.is_empty() {
            bail!("AnkiWeb 返回了空的 hkey");
        }
        self.hkey = hk.key;
        Ok(self.hkey.clone())
    }

    /// Download the full collection from AnkiWeb as raw `collection.anki2` bytes.
    /// NOTE: the official client calls download directly (no meta handshake).
    pub fn full_download(&mut self) -> Result<Vec<u8>> {
        self.call("download", b"{}")
    }

    fn call(&mut self, method: &str, payload: &[u8]) -> Result<Vec<u8>> {
        let header = json!({
            "v": SYNC_VERSION,
            "k": self.hkey,
            "c": CLIENT_VERSION,
            "s": session_id(),
        });
        let body = zstd::stream::encode_all(payload, 0)?;
        // Redirects carry a new base endpoint (host root), not the full method URL.
        let mut base = self.base.clone();
        let mut url = format!("{base}sync/{method}");
        let mut visited: Vec<String> = Vec::new();
        let mut transient_retries = 0u32;
        loop {
            if visited.iter().any(|u| u == &url) {
                bail!("AnkiWeb 重定向循环（{url}）");
            }

            let resp = self
                .client
                .post(&url)
                .header("anki-sync", header.to_string())
                .header("content-type", "application/octet-stream")
                .body(body.clone())
                .send()?;
            let status = resp.status();
            if status.as_u16() == 403 {
                bail!(
                    "AnkiWeb 拒绝了请求（403）：通常是账号或密码错误；若确认无误，可能是请求过于频繁，请几分钟后重试"
                );
            }
            if status.is_redirection() {
                match resp.headers().get("location") {
                    Some(location) => {
                        visited.push(url.clone());
                        let location = location.to_str()?.to_string();
                        base = if location.starts_with("http") {
                            location.clone()
                        } else {
                            format!("{ENDPOINT}{location}")
                        };
                        if !base.ends_with('/') {
                            base.push('/');
                        }
                        url = format!("{base}sync/{method}");
                    }
                    None => {
                        // 303 without Location = AnkiWeb throttle; back off and
                        // retry the same URL (not counted as a redirect hop).
                        transient_retries += 1;
                        if transient_retries > 5 {
                            bail!("AnkiWeb 持续限流（303 无 Location），请几分钟后重试");
                        }
                        std::thread::sleep(Duration::from_millis(800 * u64::from(transient_retries)));
                    }
                }
                continue;
            }
            if !status.is_success() {
                let raw = resp.bytes().unwrap_or_default();
                let detail = zstd::stream::decode_all(&raw[..]).unwrap_or_else(|_| raw.to_vec());
                let text = String::from_utf8_lossy(&detail);
                bail!("AnkiWeb 返回 {status}: {}", &text[..text.len().min(300)]);
            }
            self.base = base;
            return zstd::stream::decode_all(&resp.bytes()?[..])
                .context("解压 AnkiWeb 响应失败");
        }
        bail!("跟随 AnkiWeb 重定向超过 10 次")
    }
}

/// Convenience wrapper: one session, login only.
pub fn login(username: &str, password: &str) -> Result<String> {
    AnkiWebClient::new()?.login(username, password)
}

/// Convenience wrapper: login + full download in one session.
pub fn pull(username: &str, password: &str) -> Result<Vec<u8>> {
    let mut c = AnkiWebClient::new()?;
    c.login(username, password)?;
    c.full_download()
}

/// Wrap a bare `collection.anki2` into a minimal `.apkg` so the existing
/// `anka-apkg` importer can process it, then import into `col`.
pub fn import_into_collection(
    data: &[u8],
    col: &mut anka_core::Collection,
) -> Result<anka_apkg::ImportReport> {
    let tmp = tempfile::NamedTempFile::new()?;
    {
        let mut file = tmp.reopen()?;
        let mut zip = zip::ZipWriter::new(&mut file);
        let opts = zip::write::SimpleFileOptions::default();
        zip.start_file("collection.anki2", opts)?;
        zip.write_all(data)?;
        zip.start_file("media", opts)?;
        zip.write_all(b"{}")?;
        zip.finish()?;
    }
    anka_apkg::import_apkg(tmp.path(), col).map_err(|e| e.into())
}

/// Merge a bare agent `collection.anki2` into an Anka collection without
/// duplicating: notes already known via `anki_id_map` get their fields/tags
/// updated, unknown notes are created.
pub fn merge_agent_into_collection(
    agent_path: &Path,
    col: &mut anka_core::Collection,
) -> Result<anka_apkg::ImportReport> {
    let data = std::fs::read(agent_path)?;
    let tmp = tempfile::NamedTempFile::new()?;
    {
        let mut file = tmp.reopen()?;
        let mut zip = zip::ZipWriter::new(&mut file);
        let opts = zip::write::SimpleFileOptions::default();
        zip.start_file("collection.anki2", opts)?;
        zip.write_all(&data)?;
        zip.start_file("media", opts)?;
        zip.write_all(b"{}")?;
        zip.finish()?;
    }
    anka_apkg::merge_apkg(tmp.path(), col).map_err(|e| e.into())
}

/// Merge scheduling state from the agent collection into Anka cards
/// (download direction: grades made in Anki/AnkiDroid/AnkiWeb flow in).
/// Only cards carrying an FSRS memory state (`data` JSON with s/d) are
/// applied; everything else is left as-is to avoid corrupting scheduling.
pub fn merge_scheduling_from_agent(
    agent_path: &Path,
    col: &mut anka_core::Collection,
) -> Result<(u32, u32)> {
    use rusqlite::OpenFlags;

    let conn = rusqlite::Connection::open_with_flags(
        agent_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    let crt: i64 = conn.query_row("SELECT crt FROM col", [], |r| r.get(0))?;
    let mut stmt = conn.prepare(
        "SELECT id, due, ivl, reps, lapses, type, queue, data FROM cards WHERE reps > 0",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, i64>(3)?,
            r.get::<_, i64>(4)?,
            r.get::<_, i64>(5)?,
            r.get::<_, i64>(6)?,
            r.get::<_, String>(7).unwrap_or_default(),
        ))
    })?;

    let now = chrono::Utc::now();
    let (mut updated, mut skipped) = (0u32, 0u32);
    for row in rows {
        let (anki_id, due, _ivl, reps, lapses, ctype, queue, data) = row?;
        let Some(anka_card) = col.anka_id_for_anki("card", &anki_id.to_string())? else {
            skipped += 1;
            continue;
        };
        // FSRS memory state lives in card.data for v3-scheduler cards
        let (stability, difficulty) = serde_json::from_str::<serde_json::Value>(&data)
            .ok()
            .and_then(|v| {
                let s = v.get("s").and_then(|x| x.as_f64())?;
                let d = v.get("d").and_then(|x| x.as_f64())?;
                Some((s as f32, d as f32))
            })
            .unwrap_or((0.0, 0.0));
        if stability <= 0.0 {
            skipped += 1;
            continue;
        }
        // due: learning cards use epoch seconds, review/relearning use day
        // numbers relative to the collection creation timestamp.
        let due_at = if ctype == 1 || queue == 1 || queue == 3 {
            chrono::TimeZone::timestamp_opt(&chrono::Utc, due, 0)
                .single()
                .unwrap_or_else(chrono::Utc::now)
        } else {
            chrono::TimeZone::timestamp_opt(&chrono::Utc, crt + due * 86400, 0)
                .single()
                .unwrap_or_else(chrono::Utc::now)
        };
        let state = anka_core::CardState {
            due_at,
            stability,
            difficulty,
            reps: reps as u32,
            lapses: lapses as u32,
            last_review_at: None,
        };
        col.apply_synced_schedule(anka_card, &state, now)?;
        updated += 1;
    }
    Ok((updated, skipped))
}
