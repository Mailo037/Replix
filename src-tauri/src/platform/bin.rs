//! The recycle bin (the Trash on Linux and macOS): moving clips there and back.

use std::path::{Path, PathBuf};

pub use self::imp::{put, restore};

/// What to tell when nothing came back.
pub fn gone() -> String {
    if cfg!(windows) {
        crate::i18n::tr("It's no longer in the recycle bin.")
    } else {
        crate::i18n::tr("It's no longer in the trash.")
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    //! Windows and Linux: the system can list what is in the bin and restore it.

    use std::collections::HashMap;

    use super::{Path, PathBuf};

    pub fn put(path: &Path) -> anyhow::Result<()> {
        trash::delete(path)?;
        Ok(())
    }

    /// Takes `wanted` back out, the newest copy of each, when one of `clips` is among them.
    /// Says how many of `clips` came back.
    pub fn restore(clips: &[PathBuf], wanted: &[PathBuf]) -> anyhow::Result<usize> {
        let mut newest: HashMap<PathBuf, trash::TrashItem> = HashMap::new();
        for item in trash::os_limited::list()? {
            let original = item.original_path();
            if !wanted.contains(&original) || original.exists() {
                continue;
            }
            if newest.get(&original).is_none_or(|known| known.time_deleted < item.time_deleted) {
                newest.insert(original, item);
            }
        }
        let back = clips.iter().filter(|p| newest.contains_key(*p)).count();
        if back > 0 {
            trash::os_limited::restore_all(newest.into_values())?;
        }
        Ok(back)
    }
}

#[cfg(target_os = "macos")]
mod imp {
    //! macOS: Finder's own Trash cannot be listed or restored from by other apps,
    //! so Replix moves files into ~/.Trash itself and remembers where each went: Ctrl+Z brings
    //! them back while Replix runs, and Finder shows them in the Trash as usual.

    use std::collections::HashMap;
    use std::sync::OnceLock;

    use parking_lot::Mutex;

    use super::{Path, PathBuf};

    fn moved() -> &'static Mutex<HashMap<PathBuf, PathBuf>> {
        static MOVED: OnceLock<Mutex<HashMap<PathBuf, PathBuf>>> = OnceLock::new();
        MOVED.get_or_init(Default::default)
    }

    pub fn put(path: &Path) -> anyhow::Result<()> {
        let trash = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join(".Trash");
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let (stem, ext) = match name.rsplit_once('.') {
            Some((s, e)) => (s.to_string(), format!(".{e}")),
            None => (name.clone(), String::new()),
        };
        let mut target = trash.join(&name);
        let mut n = 2;
        while target.exists() {
            target = trash.join(format!("{stem} {n}{ext}"));
            n += 1;
        }
        // Another drive has a Trash of its own, which only Finder knows how to reach.
        if std::fs::rename(path, &target).is_err() {
            trash::delete(path)?;
            return Ok(());
        }
        moved().lock().insert(path.to_path_buf(), target);
        Ok(())
    }

    /// Takes `wanted` back out, when one of `clips` is among them. Says how many of `clips`
    /// came back.
    pub fn restore(clips: &[PathBuf], wanted: &[PathBuf]) -> anyhow::Result<usize> {
        let mut moved = moved().lock();
        let there = |moved: &HashMap<PathBuf, PathBuf>, p: &PathBuf| !p.exists() && moved.get(p).is_some_and(|at| at.exists());
        let back = clips.iter().filter(|p| there(&moved, p)).count();
        if back == 0 {
            return Ok(0);
        }
        for original in wanted {
            if !there(&moved, original) {
                continue;
            }
            if let Some(at) = moved.remove(original) {
                std::fs::rename(at, original)?;
            }
        }
        Ok(back)
    }
}
