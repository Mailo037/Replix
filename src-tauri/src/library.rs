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
    /// The file could not be read when the sidecar was written (a busy drive, say); a
    /// later scan reads it and fills in its length and size.
    pub unread: bool,
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

/// Held around reading, changing and writing a sidecar, so two changes to one item at
/// the same time cannot lose either. Every write takes it too, so a caller that already
/// holds it for a read, change and write may write inside.
pub static META_LOCK: parking_lot::ReentrantMutex<()> = parking_lot::const_reentrant_mutex(());

/// Numbers the temp files sidecars are written to.
static TEMP_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn write_meta(path: &Path, meta: &Meta) -> std::io::Result<()> {
    let file = sidecar(path);
    let text = serde_json::to_string_pretty(meta)?;
    let _held = META_LOCK.lock();
    // Hidden files cannot be overwritten in place on Windows. The new one is written next
    // to it and renamed over it, so a reader never finds the sidecar missing and a failed
    // write leaves the old one as it was. The temp file is always created new, never opened
    // over an old one: a name already taken, say by a file left from an earlier run with
    // the same process id, moves on to the next.
    let (temp, mut handle) = loop {
        let id = TEMP_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let temp = file.with_extension(format!("json.{}-{id}.tmp", std::process::id()));
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&temp) {
            Ok(handle) => break (temp, handle),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    };
    let written = std::io::Write::write_all(&mut handle, text.as_bytes()).and_then(|()| {
        drop(handle);
        win::hide_file(&temp);
        std::fs::rename(&temp, &file)
    });
    if written.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    written
}

pub fn remove_meta(path: &Path) {
    let _ = std::fs::remove_file(sidecar(path));
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

/// Media files in the clips folder and one level of game folders below it, with their sizes.
/// On Windows the directory listing already carries type and size, so this costs no extra
/// file system call per file.
fn media_files(root: &Path) -> Vec<(PathBuf, u64)> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else { return out };
    // A link has to be followed to see what it points to.
    let metadata = |entry: &std::fs::DirEntry| match entry.metadata() {
        Ok(m) if m.is_symlink() => std::fs::metadata(entry.path()),
        other => other,
    };
    let media = |entry: std::fs::DirEntry| {
        let meta = metadata(&entry).ok().filter(|m| m.is_file())?;
        let path = entry.path();
        kind_of(&path).map(|_| (path, meta.len()))
    };
    for entry in entries.flatten() {
        if metadata(&entry).is_ok_and(|m| m.is_dir()) {
            if entry.file_name() == EXPORTS {
                continue;
            }
            if let Ok(inner) = std::fs::read_dir(entry.path()) {
                out.extend(inner.flatten().filter_map(media));
            }
        } else if let Some(file) = media(entry) {
            out.push(file);
        }
    }
    out
}

/// Bytes of every clip, screenshot and session, and of the exports.
pub fn usage(root: &Path) -> (u64, u64) {
    let clips = media_files(root).iter().map(|f| f.1).sum();
    let exports = std::fs::read_dir(root.join(EXPORTS))
        .map(|d| d.flatten().map(|e| e.metadata().map(|m| m.len()).unwrap_or(0)).sum())
        .unwrap_or(0);
    (clips, exports)
}

/// What a file is from its name and place alone, for when it cannot be read now.
fn stand_in(path: &Path, root: &Path) -> Meta {
    let kind = kind_of(path).unwrap_or_default();
    let game = path
        .parent()
        .filter(|p| *p != root)
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let created = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    Meta {
        kind,
        title: path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
        game,
        created,
        seen: true,
        unread: true,
        ..Default::default()
    }
}

/// Reads a file's header during a scan. That is optional work like thumbnails: it takes
/// the shared media turn, one file at a time whatever the number of scan threads, gives
/// way to the editor and stops while Replix is hidden or in the background.
fn probe_in_turn(path: &Path) -> anyhow::Result<ffmpeg::MediaInfo> {
    if PROBE_FAILURES.recent(path) {
        anyhow::bail!("could not be read a moment ago");
    }
    let probed = crate::media::run(&crate::media::Demand::new(crate::media::AHEAD), |go| ffmpeg::probe_while(path, go)).result();
    match &probed {
        Err(e) if crate::media::is_stopped(e) => {
            DEFERRED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        Err(_) => PROBE_FAILURES.note(path),
        Ok(_) => PROBE_FAILURES.forget(path),
    }
    probed
}

/// Files some work failed on, with their size and change time then and when it was. A
/// broken file is tried again after a while, or as soon as it changes, not on every scan.
pub struct Failures(parking_lot::Mutex<std::collections::BTreeMap<PathBuf, (u64, Option<std::time::SystemTime>, std::time::Instant)>>);
const RETRY_AFTER: std::time::Duration = std::time::Duration::from_secs(10 * 60);

impl Failures {
    const fn new() -> Self {
        Self(parking_lot::const_mutex(std::collections::BTreeMap::new()))
    }

    pub fn note(&self, path: &Path) {
        let (size, modified) = stamp(path);
        self.0.lock().insert(path.to_path_buf(), (size, modified, std::time::Instant::now()));
    }

    pub fn forget(&self, path: &Path) {
        self.0.lock().remove(path);
    }

    pub fn recent(&self, path: &Path) -> bool {
        let failed = self.0.lock();
        let Some(&(size, modified, at)) = failed.get(path) else { return false };
        at.elapsed() < RETRY_AFTER && stamp(path) == (size, modified)
    }
}

static PROBE_FAILURES: Failures = Failures::new();
pub static THUMB_FAILURES: Failures = Failures::new();

fn stamp(path: &Path) -> (u64, Option<std::time::SystemTime>) {
    std::fs::metadata(path).map(|m| (m.len(), m.modified().ok())).unwrap_or((0, None))
}


/// Counts files a scan left unread because its reading gave way.
static DEFERRED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn tracks_of(info: &ffmpeg::MediaInfo) -> Vec<TrackMeta> {
    info.audio_titles
        .iter()
        .enumerate()
        .map(|(i, t)| TrackMeta { name: t.clone().unwrap_or_else(|| format!("Track {}", i + 1)), kind: TrackKind::App })
        .collect()
}

/// Reads a file whose sidecar says it could not be read before, and fills its details in.
fn repair(path: &Path) -> Option<Meta> {
    let info = probe_in_turn(path).ok()?;
    let _held = META_LOCK.lock();
    let mut meta = read_meta(path)?;
    if !meta.unread {
        return Some(meta);
    }
    meta.duration = info.duration;
    meta.width = info.width;
    meta.height = info.height;
    if meta.tracks.is_empty() {
        meta.tracks = tracks_of(&info);
    }
    meta.unread = false;
    write_meta(path, &meta).ok()?;
    Some(meta)
}

/// A file someone put into the folder without Replix: describe it once. The second value
/// says whether the file could be read; if not (a slow drive, or a scan that gave way), the
/// description is marked unread, and a later scan reads the file. It is read in the shared
/// media turn, like other optional work.
fn infer_meta(path: &Path, root: &Path) -> (Meta, bool) {
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
    // Only the header is read, within a time limit.
    let probed = probe_in_turn(path);
    let read = probed.is_ok();
    let info = probed.unwrap_or_default();
    let tracks = tracks_of(&info);
    let meta = Meta {
        kind,
        title: path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
        game,
        created,
        duration: info.duration,
        width: info.width,
        height: info.height,
        tracks,
        seen: true,
        unread: !read,
        ..Default::default()
    };
    (meta, read)
}

/// What the sidecar says, or, without one, what the file is from its name and place, so a
/// change made to it keeps its kind, title and date instead of starting from nothing. The
/// file itself is not read here: the caller holds `META_LOCK`, which every other change
/// waits for. It is marked unread, and a later scan fills in length and size.
pub fn meta_or_describe(path: &Path, root: &Path) -> Meta {
    read_meta(path).unwrap_or_else(|| stand_in(path, root))
}

/// Every item, newest first, and whether reading some files had to wait: they are listed
/// as unread, and a scan once Replix is free again completes them.
pub fn scan(root: &Path, thumbs: &Path) -> (Vec<Item>, bool) {
    let deferred_before = DEFERRED.load(std::sync::atomic::Ordering::Relaxed);
    let files = media_files(root);
    // One listing of the thumbnail folder instead of a lookup per item.
    let made: std::collections::HashSet<std::ffi::OsString> = std::fs::read_dir(thumbs)
        .map(|d| d.flatten().map(|e| e.file_name()).collect())
        .unwrap_or_default();
    let describe = |(path, size): &(PathBuf, u64)| {
        let sidecar = read_meta(path);
        // A file just described without success is not read a second time in this scan.
        let had_sidecar = sidecar.is_some();
        let mut meta = sidecar.unwrap_or_else(|| {
            // Without a sidecar the file is described from itself. While Replix is hidden
            // only from its name and place; the next scan reads the file.
            if !crate::page_visible() {
                DEFERRED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                return stand_in(path, root);
            }
            let (meta, read) = infer_meta(path, root);
            let _held = META_LOCK.lock();
            // A command may have written one meanwhile; that one wins.
            read_meta(path).unwrap_or_else(|| {
                if read {
                    let _ = write_meta(path, &meta);
                }
                meta
            })
        });
        if had_sidecar && meta.unread {
            if crate::page_visible() {
                meta = repair(path).unwrap_or(meta);
            } else {
                // Left for later: the scan says so, and the next one repairs it.
                DEFERRED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
        let id = id_for(path);
        let name = format!("{id}.jpg");
        Item {
            size: *size,
            thumb: made.contains(std::ffi::OsStr::new(&name)).then(|| thumbs.join(&name).to_string_lossy().into_owned()),
            path: path.to_string_lossy().into_owned(),
            id,
            meta,
        }
    };
    // Reading the sidecars, and probing files Replix has not seen, waits on the disk; a few
    // threads overlap that. Small libraries are not worth the threads.
    let workers = std::thread::available_parallelism().map_or(2, |n| n.get()).clamp(1, 4);
    // They take the next file in turn; once Replix is hidden or in the background, all
    // but one stop, and that one finishes the list alone.
    let mut items: Vec<Item> = if files.len() < 64 || workers == 1 {
        files.iter().map(describe).collect()
    } else {
        let next = std::sync::atomic::AtomicUsize::new(0);
        let work = |helper: bool| {
            let mut done = Vec::new();
            while !(helper && !(crate::page_visible() && crate::app_focused())) {
                let Some(file) = files.get(next.fetch_add(1, std::sync::atomic::Ordering::Relaxed)) else { break };
                done.push(describe(file));
            }
            done
        };
        std::thread::scope(|s| {
            let helpers: Vec<_> = (1..workers).map(|_| s.spawn(|| work(true))).collect();
            let mut all = work(false);
            all.extend(helpers.into_iter().flat_map(|h| h.join().unwrap_or_default()));
            all
        })
    };
    items.sort_by_key(|i| std::cmp::Reverse(i.meta.created));
    (items, DEFERRED.load(std::sync::atomic::Ordering::Relaxed) != deferred_before)
}

/// Creates the thumbnail of one item; returns its path.
pub fn make_thumb(item_path: &Path, kind: Kind, duration: f64, out: &Path, go: &(dyn Fn() -> bool + Sync)) -> anyhow::Result<()> {
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let part = out.with_extension("part.jpg");
    // One thread and only the keyframe at the seek point: a new clip's thumbnail is made
    // while the game is still running and should not cost it a frame.
    let mut args: Vec<String> = vec!["-threads".into(), "1".into()];
    if kind != Kind::Screenshot {
        args.extend(["-ss".into(), format!("{:.2}", (duration / 3.0).min(2.0)), "-noaccurate_seek".into()]);
        args.extend(["-skip_frame".into(), "nokey".into()]);
    }
    args.extend([
        "-filter_threads".into(),
        "1".into(),
        "-i".into(),
        ffmpeg::path_arg(item_path),
        "-frames:v".into(),
        "1".into(),
        "-vf".into(),
        "scale=480:-2".into(),
        "-q:v".into(),
        "4".into(),
        "-threads".into(),
        "1".into(),
        "-y".into(),
        ffmpeg::path_arg(&part),
    ]);
    // Made under another name and renamed when whole: a thumbnail's file is taken as ready
    // as soon as it exists, so a stopped one must not leave half a picture behind.
    let made = ffmpeg::run_while(&args, go).and_then(|()| Ok(std::fs::rename(&part, out)?));
    if made.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    made
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

/// A free name in the exports folder, taken at once with an empty file, so two exports of
/// the same title never pick the same one, however long either takes.
pub fn export_path(root: &Path, title: &str, ext: &str) -> PathBuf {
    let dir = root.join(EXPORTS);
    let _ = std::fs::create_dir_all(&dir);
    let base = safe_name(title);
    let mut path = dir.join(format!("{base}.{ext}"));
    let mut n = 2;
    loop {
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(_) => return path,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            // Not writable: FFmpeg will say why when it tries.
            Err(_) => return path,
        }
        path = dir.join(format!("{base} ({n}).{ext}"));
        n += 1;
    }
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
pub fn enforce_limit(root: &Path, limit_bytes: u64) -> bool {
    let all = media_files(root);
    // Runs after every save; the sizes alone usually show there is nothing to do. Every
    // file takes up room, but only one Replix knows (it has a sidecar) is ever deleted.
    let mut total: u64 = all.iter().map(|f| f.1).sum();
    if total <= limit_bytes {
        return false;
    }
    let mut files: Vec<(PathBuf, Meta, u64)> =
        all.into_iter().filter_map(|(p, size)| read_meta(&p).map(|m| (p, m, size))).collect();
    files.sort_by_key(|f| f.1.created);
    let mut changed = false;
    for (path, meta, size) in files {
        if total <= limit_bytes {
            break;
        }
        if meta.favorite || meta.draft.is_some() {
            continue;
        }
        // Someone may have starred it or saved a draft since it was read; look again while
        // no change can come in between.
        let _held = META_LOCK.lock();
        if read_meta(&path).is_some_and(|m| m.favorite || m.draft.is_some()) {
            continue;
        }
        if trash::delete(&path).is_ok() {
            changed = true;
            let _ = std::fs::remove_file(sidecar(&path));
            total = total.saturating_sub(size);
            log::info!("storage limit: moved {} to the recycle bin", path.display());
        }
    }
    changed
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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("replix-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn sidecar_is_replaced_while_hidden_and_survives_a_stale_temp_file() {
        let dir = temp_dir("meta");
        let clip = dir.join("clip.mp4");
        std::fs::write(&clip, b"").unwrap();
        write_meta(&clip, &Meta { title: "one".into(), ..Default::default() }).unwrap();
        // A crash between writing and renaming leaves a hidden temp file behind, here on
        // exactly the names the next writes would pick.
        let next = TEMP_ID.load(std::sync::atomic::Ordering::Relaxed);
        let taken = |n: u64| sidecar(&clip).with_extension(format!("json.{}-{n}.tmp", std::process::id()));
        for n in next..next + 3 {
            std::fs::write(taken(n), b"{}").unwrap();
            win::hide_file(&taken(n));
        }
        write_meta(&clip, &Meta { title: "two".into(), ..Default::default() }).unwrap();
        assert_eq!(read_meta(&clip).unwrap().title, "two");
        let temps = std::fs::read_dir(&dir).unwrap().flatten().filter(|e| e.path().to_string_lossy().ends_with(".tmp")).count();
        assert_eq!(temps, 3, "only the stale files stay behind");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Starring a screenshot Replix has no sidecar for yet keeps what it is.
    #[test]
    fn a_change_to_an_undescribed_file_keeps_its_kind_title_and_date() {
        let dir = temp_dir("describe");
        std::fs::create_dir_all(dir.join("Game")).unwrap();
        let shot = dir.join("Game").join("Big moment.png");
        std::fs::write(&shot, b"").unwrap();
        let started = std::time::Instant::now();
        let mut meta = meta_or_describe(&shot, &dir);
        // Described from its name at once, without reading it: a later scan reads it.
        assert!(started.elapsed() < std::time::Duration::from_millis(200), "no probe under the lock");
        assert!(meta.unread);
        meta.favorite = true;
        write_meta(&shot, &meta).unwrap();
        let kept = read_meta(&shot).unwrap();
        assert_eq!(kept.kind, Kind::Screenshot);
        assert_eq!(kept.title, "Big moment");
        assert_eq!(kept.game, "Game");
        assert!(kept.created > 0 && kept.favorite);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A clip whose sidecar was written while the file could not be read gets its details
    /// on a later scan.
    #[test]
    fn an_unread_clip_is_filled_in_later() {
        let ok = std::process::Command::new("ffmpeg").arg("-version").stdout(std::process::Stdio::null()).status().is_ok_and(|s| s.success());
        if !ok {
            return;
        }
        let dir = temp_dir("repair");
        let clip = dir.join("clip.mp4");
        let made = std::process::Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", "testsrc2=size=320x180:rate=30:d=1", "-y"])
            .arg(&clip)
            .status()
            .unwrap();
        assert!(made.success());
        write_meta(&clip, &Meta { title: "Mine".into(), unread: true, ..Default::default() }).unwrap();
        // Repairs are made ahead of time, which needs Replix in use.
        let (_rounds, _) = crate::media::test_rounds();
        crate::FOCUSED.store(true, std::sync::atomic::Ordering::Relaxed);
        let fixed = repair(&clip);
        crate::FOCUSED.store(false, std::sync::atomic::Ordering::Relaxed);
        let fixed = fixed.unwrap();
        assert_eq!((fixed.width, fixed.height, fixed.unread, fixed.title.as_str()), (320, 180, false, "Mine"));
        assert!(fixed.duration > 0.9);
        assert!(!read_meta(&clip).unwrap().unread);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn two_exports_of_one_title_get_two_names() {
        let dir = temp_dir("exports");
        let a = export_path(&dir, "Ace", "mp4");
        let b = export_path(&dir, "Ace", "mp4");
        assert_ne!(a, b);
        assert!(a.exists() && b.exists(), "each name is taken at once");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A file that could not be read is not read again on the next scan, unless it changed.
    #[test]
    fn a_broken_file_is_not_probed_on_every_scan() {
        let dir = temp_dir("backoff");
        let clip = dir.join("broken.mp4");
        std::fs::write(&clip, b"x").unwrap();
        PROBE_FAILURES.note(&clip);
        assert!(PROBE_FAILURES.recent(&clip));
        std::fs::write(&clip, b"changed").unwrap();
        assert!(!PROBE_FAILURES.recent(&clip), "a changed file is tried again");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_finds_media_with_sizes_and_skips_exports() {
        let dir = temp_dir("scan");
        std::fs::create_dir_all(dir.join("Game")).unwrap();
        std::fs::create_dir_all(dir.join(EXPORTS)).unwrap();
        std::fs::write(dir.join("Game").join("a.mp4"), vec![0u8; 10]).unwrap();
        std::fs::write(dir.join("b.png"), vec![0u8; 3]).unwrap();
        std::fs::write(dir.join(EXPORTS).join("c.mp4"), vec![0u8; 5]).unwrap();
        std::fs::write(dir.join("notes.txt"), b"x").unwrap();
        let mut found: Vec<(String, u64)> =
            media_files(&dir).into_iter().map(|(p, s)| (p.file_name().unwrap().to_string_lossy().into_owned(), s)).collect();
        found.sort();
        assert_eq!(found, vec![("a.mp4".to_string(), 10), ("b.png".to_string(), 3)]);
        assert_eq!(usage(&dir), (13, 5));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
