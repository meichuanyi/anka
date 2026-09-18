//! AnkiWeb one-time migration: login + full collection download.
//!
//! Wire format ported from Anki's AGPL client (`rslib/src/sync`, protocol v11):
//! POST `https://sync.ankiweb.net/sync/<method>` with an `anki-sync` JSON header
//! and a zstd-compressed JSON body. Responses are zstd-compressed as well.
//!
//! Passwords live only in the caller's memory for the duration of the call.

use anyhow::{bail, Context, Result};
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

fn call(sync_key: &str, method: &str, payload: &[u8]) -> Result<Vec<u8>> {
    let header = json!({
        "v": SYNC_VERSION,
        "k": sync_key,
        "c": CLIENT_VERSION,
        "s": session_id(),
    });
    let url = format!("{ENDPOINT}sync/{method}");
    let resp = http_client()
        .post(url)
        .header("anki-sync", header.to_string())
        .header("content-type", "application/octet-stream")
        .body(zstd::stream::encode_all(payload, 0)?)
        .send()?;
    let status = resp.status();
    if status.as_u16() == 403 {
        bail!("AnkiWeb 账号或密码错误（403）");
    }
    if !status.is_success() {
        bail!("AnkiWeb 返回 {status}");
    }
    let body = resp.bytes()?;
    zstd::stream::decode_all(&body[..]).context("解压 AnkiWeb 响应失败")
}

/// Authenticate against AnkiWeb, returning the sync hkey.
pub fn login(username: &str, password: &str) -> Result<String> {
    #[derive(Deserialize)]
    struct HostKey {
        key: String,
    }
    let payload = json!({ "u": username, "p": password }).to_string().into_bytes();
    let body = call("", "hostKey", &payload)?;
    let hk: HostKey = serde_json::from_slice(&body).context("解析 hostKey 响应失败")?;
    if hk.key.is_empty() {
        bail!("AnkiWeb 返回了空的 hkey");
    }
    Ok(hk.key)
}

/// Download the full collection from AnkiWeb as raw `collection.anki2` bytes.
pub fn full_download(hkey: &str) -> Result<Vec<u8>> {
    call(hkey, "download", b"{}")
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
