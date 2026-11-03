//! Clips on disk. Each media file has a hidden `.replix.json` next to it with what
//! the file itself cannot say: the game, the names of the audio tracks, edits.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use chrono::Local;
use serde::{Deserialize, Serialize};

use crate::audio::TrackKind;
use crate::ffmpeg;
use crate::win;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    #[default]
    Clip,
    Session,
    Screenshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackMeta {
    pub name: String,
    pub kind: TrackKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackEdit {
    pub enabled: bool,
    pub volume: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    pub trim_start: f64,
    pub trim_end: f64,
    pub tracks: Vec<TrackEdit>,
    /// Ranges taken out between start and end.
    #[serde(default)]
    pub cuts: Vec<[f64; 2]>,
    /// Where the clip is split on the timeline, so the parts can be removed one by one.
    #[serde(default)]
    pub splits: Vec<f64>,
    #[serde(default)]
    pub vertical: bool,
    #[serde(default = "centre")]
    pub crop_x: f64,
}

fn centre() -> f64 {
    0.5
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Meta {
    pub kind: Kind,
    pub title: String,
    pub game: String,
    /// Epoch milliseconds.
    pub created: u64,
    pub duration: f64,
    pub width: u32,
    pub height: u32,
    pub tracks: Vec<TrackMeta>,
    pub draft: Option<Draft>,
    pub favorite: bool,
    pub seen: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub id: String,
    pub path: String,
    pub size: u64,
    pub thumb: Option<String>,
    #[serde(flatten)]
    pub meta: Meta,
}

const VIDEO: &[&str] = &["mp4", "mkv", "mov", "webm"];
const IMAGE: &[&str] = &["png", "jpg", "jpeg", "webp"];
/// Exports go here and stay out of the library.
pub const EXPORTS: &str = "Exports";

fn sidecar(path: &Path) -> PathBuf {
    path.with_extension(format!(
        "{}.replix.json",
        path.extension().and_then(|e| e.to_str()).unwrap_or("")
    ))
}

pub fn read_meta(path: &Path) -> Option<Meta> {
    let text = std::fs::read_to_string(sidecar(path)).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn write_meta(path: &Path, meta: &Meta) {
    let file = sidecar(path);
    // Hidden files cannot be overwritten in place on Windows; replace instead.
    let _ = std::fs::remove_file(&file);
    if let Ok(text) = serde_json::to_string_pretty(meta) {
        if std::fs::write(&file, text).is_ok() {
            win::hide_file(&file);
        }
    }
}

pub fn id_for(path: &Path) -> String {
    // FNV-1a: stable across runs, unlike std's hasher.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in path.to_string_lossy().to_lowercase().bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn kind_of(path: &Path) -> Option<Kind> {
    let ext = path.extension()?.to_str()?.to_lowercase();
    if VIDEO.contains(&ext.as_str()) {
        Some(Kind::Clip)
    } else if IMAGE.contains(&ext.as_str()) {
        Some(Kind::Screenshot)
    } else {
        None
    }
}

/// Media files in the clips folder and one level of game folders below it.
fn media_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else { return out };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == EXPORTS) {
                continue;
            }
            if let Ok(inner) = std::fs::read_dir(&path) {
                out.extend(inner.flatten().map(|e| e.path()).filter(|p| p.is_file() && kind_of(p).is_some()));
            }
        } else if kind_of(&path).is_some() {
            out.push(path);
        }
    }
    out
}

/// Bytes of every clip, screenshot and session, and of the exports.
pub fn usage(root: &Path) -> (u64, u64) {
    let size = |p: &PathBuf| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    let clips = media_files(root).iter().map(size).sum();
    let exports = std::fs::read_dir(root.join(EXPORTS))
        .map(|d| d.flatten().map(|e| e.metadata().map(|m| m.len()).unwrap_or(0)).sum())
        .unwrap_or(0);
    (clips, exports)
}

/// A file someone put into the folder without Replix: describe it once.
fn infer_meta(path: &Path, root: &Path) -> Meta {
    let kind = kind_of(path).unwrap_or_default();
    let created = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let game = path
        .parent()
        .filter(|p| *p != root)
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let info = ffmpeg::probe(path).unwrap_or_default();
    let tracks = info
        .audio_titles
        .iter()
        .enumerate()
        .map(|(i, t)| TrackMeta { name: t.clone().unwrap_or_else(|| format!("Track {}", i + 1)), kind: TrackKind::App })
        .collect();
    Meta {
        kind,
        title: path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
        game,
        created,
        duration: info.duration,
        width: info.width,
        height: info.height,
        tracks,
        seen: true,
        ..Default::default()
    }
}

pub fn scan(root: &Path, thumbs: &Path) -> Vec<Item> {
    let mut items: Vec<Item> = media_files(root)
        .into_iter()
        .map(|path| {
            let meta = read_meta(&path).unwrap_or_else(|| {
                let meta = infer_meta(&path, root);
                write_meta(&path, &meta);
                meta
            });
            let id = id_for(&path);
            let thumb = thumbs.join(format!("{id}.jpg"));
            Item {
                size: std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
                thumb: thumb.exists().then(|| thumb.to_string_lossy().into_owned()),
                path: path.to_string_lossy().into_owned(),
                id,
                meta,
            }
        })
        .collect();
    items.sort_by_key(|i| std::cmp::Reverse(i.meta.created));
    items
}

/// Creates the thumbnail of one item; returns its path.
pub fn make_thumb(item_path: &Path, kind: Kind, duration: f64, out: &Path) -> anyhow::Result<()> {
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut args: Vec<String> = Vec::new();
    if kind != Kind::Screenshot {
        args.extend(["-ss".into(), format!("{:.2}", (duration / 3.0).min(2.0))]);
    }
    args.extend([
        "-i".into(),
        ffmpeg::path_arg(item_path),
        "-frames:v".into(),
        "1".into(),
        "-vf".into(),
        "scale=480:-2".into(),
        "-q:v".into(),
        "4".into(),
        "-y".into(),
        ffmpeg::path_arg(out),
    ]);
    ffmpeg::run(&args)
}

fn safe_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if "<>:\"/\\|?*".contains(c) || c.is_control() { ' ' } else { c })
        .collect();
    let trimmed = cleaned.trim().trim_end_matches('.').trim();
    if trimmed.is_empty() { "Clips".to_string() } else { trimmed.chars().take(60).collect() }
}

/// `<clips>/<Game>/<Game> 2026-11-03 21-12-04.mp4`, unique.
pub fn new_path(root: &Path, game: &str, ext: &str, label: Option<&str>) -> PathBuf {
    let folder = safe_name(game);
    let dir = root.join(&folder);
    let _ = std::fs::create_dir_all(&dir);
    let stamp = Local::now().format("%Y-%m-%d %H-%M-%S");
    let base = match label {
        Some(l) => format!("{folder} {l} {stamp}"),
        None => format!("{folder} {stamp}"),
    };
    let mut path = dir.join(format!("{base}.{ext}"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{base} ({n}).{ext}"));
        n += 1;
    }
    path
}

pub fn export_path(root: &Path, title: &str, ext: &str) -> PathBuf {
    let dir = root.join(EXPORTS);
    let _ = std::fs::create_dir_all(&dir);
    let base = safe_name(title);
    let mut path = dir.join(format!("{base}.{ext}"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{base} ({n}).{ext}"));
        n += 1;
    }
    path
}

pub fn default_title(kind: Kind, game: &str) -> String {
    match kind {
        Kind::Session => format!("{game} session"),
        Kind::Screenshot => "Screenshot".into(),
        Kind::Clip => format!("{game} clip"),
    }
}

pub fn format_duration(seconds: f64) -> String {
    let s = seconds.round() as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// Deletes the oldest clips until the folder fits the limit. Favourites and drafts stay.
pub fn enforce_limit(root: &Path, limit_bytes: u64) {
    let mut files: Vec<(PathBuf, Meta, u64)> = media_files(root)
        .into_iter()
        .filter_map(|p| {
            let meta = read_meta(&p)?;
            let size = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
            Some((p, meta, size))
        })
        .collect();
    let mut total: u64 = files.iter().map(|f| f.2).sum();
    if total <= limit_bytes {
        return;
    }
    files.sort_by_key(|f| f.1.created);
    for (path, meta, size) in files {
        if total <= limit_bytes {
            break;
        }
        if meta.favorite || meta.draft.is_some() {
            continue;
        }
        if trash::delete(&path).is_ok() {
            let _ = std::fs::remove_file(sidecar(&path));
            total = total.saturating_sub(size);
            log::info!("storage limit: moved {} to the recycle bin", path.display());
        }
    }
}

pub fn delete(path: &Path) -> anyhow::Result<()> {
    trash::delete(path)?;
    let _ = std::fs::remove_file(sidecar(path));
    Ok(())
}

/// id → path for everything the last scan found.
#[derive(Default)]
pub struct Index {
    pub paths: HashMap<String, PathBuf>,
}
