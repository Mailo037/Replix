//! Several clips joined into one. Each is first exported as the editor would export it
//! (its draft, or its tracks as they start out), then the parts play one after another at
//! the first one's size, with one mixed sound track. The result is a new clip in the
//! library, with a marker named after each clip where it begins.

use std::path::{Path, PathBuf};

use anyhow::{bail, Result};

use crate::editor::{self, ExportRequest, Preset};
use crate::encoder::Encoder;
use crate::ffmpeg;
use crate::library::{Draft, Marker, Meta, TrackEdit};
use crate::settings::TrackDefault;

/// One clip of the montage.
pub struct Part {
    pub source: PathBuf,
    pub meta: Meta,
}

/// How a part is exported: its draft, or else the whole clip with its tracks as the settings
/// say they start (the same rule as the editor's page).
pub fn request_for(meta: &Meta, defaults: &[TrackDefault]) -> ExportRequest {
    let draft = meta.draft.clone().unwrap_or_else(|| Draft {
        trim_start: 0.0,
        trim_end: meta.duration,
        tracks: meta.tracks.iter().map(|t| starting_edit(defaults, t)).collect(),
        cuts: Vec::new(),
        splits: Vec::new(),
        vertical: false,
        crop_x: 0.5,
        crop_y: 0.5,
        crop_scale: 1.0,
        master_volume: 1.0,
        speeds: Vec::new(),
        texts: Vec::new(),
    });
    ExportRequest {
        trim_start: draft.trim_start,
        trim_end: if draft.trim_end > draft.trim_start { draft.trim_end } else { meta.duration },
        tracks: draft.tracks,
        preset: Preset::Original,
        cuts: draft.cuts,
        vertical: draft.vertical,
        crop_x: draft.crop_x,
        crop_y: draft.crop_y,
        crop_scale: draft.crop_scale,
        master_volume: draft.master_volume,
        speeds: draft.speeds,
        texts: draft.texts,
    }
}

/// A track's default: its program's own, else its kind's, else as recorded.
fn starting_edit(defaults: &[TrackDefault], track: &crate::library::TrackMeta) -> TrackEdit {
    let kind = serde_json::to_value(track.kind).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default();
    let program = match kind.as_str() {
        "mic" | "desktop" => None,
        _ => track.exe.as_deref().map(|p| crate::platform::exe_name(p).to_lowercase()),
    };
    let found = program.and_then(|p| defaults.iter().find(|d| d.key == p)).or_else(|| defaults.iter().find(|d| d.key == kind));
    TrackEdit {
        enabled: found.is_none_or(|d| d.enabled),
        volume: found.map_or(1.0, |d| d.volume),
        start: None,
        end: None,
        removed: false,
    }
}

/// Where the clip's own markers land in its export: their place within the parts that are
/// kept, or nowhere if they were cut away.
fn markers_in(request: &ExportRequest, markers: &[Marker]) -> Vec<Marker> {
    let kept = editor::kept(request);
    markers
        .iter()
        .filter_map(|m| {
            let at = m.at - request.trim_start.max(0.0);
            let mut before = 0.0;
            for (a, b) in &kept {
                if at >= *a && at <= *b {
                    return Some(Marker { at: before + at - a, ..m.clone() });
                }
                before += b - a;
            }
            None
        })
        .collect()
}

/// What the montage is made of, once each part is exported: its file, length and markers.
struct Made {
    file: PathBuf,
    info: ffmpeg::MediaInfo,
    markers: Vec<Marker>,
}

/// Joins `parts` into `out`. `work` is a folder for the parts' own exports, emptied by the
/// caller. Says how long the montage is and where its markers are.
pub fn combine(
    parts: &[Part],
    defaults: &[TrackDefault],
    encoder: &Encoder,
    work: &Path,
    out: &Path,
    on_progress: impl Fn(f64),
    cancelled: &dyn Fn() -> bool,
) -> Result<(f64, Vec<Marker>)> {
    if parts.len() < 2 {
        bail!(crate::i18n::tr("Pick at least two videos to combine."));
    }
    std::fs::create_dir_all(work)?;
    // Each part is one step, the joining the last.
    let steps = parts.len() as f64 + 1.0;
    let mut made = Vec::new();
    for (i, part) in parts.iter().enumerate() {
        let request = request_for(&part.meta, defaults);
        let file = work.join(format!("part{i}.mp4"));
        let names: Vec<String> = part.meta.tracks.iter().map(|t| t.name.clone()).collect();
        editor::export(&part.source, &request, &names, encoder, &file, |p| on_progress((i as f64 + p) / steps), cancelled)?;
        let info = ffmpeg::probe(&file)?;
        let mut markers = vec![Marker { at: 0.0, name: part.meta.title.clone(), icon: String::new() }];
        markers.extend(markers_in(&request, &part.meta.markers));
        made.push(Made { file, info, markers });
    }

    // The first part decides the size; the frame rate is the highest, so nothing gets choppier.
    let (width, height) = (made[0].info.width.max(2) & !1, made[0].info.height.max(2) & !1);
    let fps = made.iter().map(|m| m.info.fps).fold(0.0, f64::max).clamp(24.0, 120.0).round();
    let length: f64 = made.iter().map(|m| m.info.duration).sum();

    let mut args: Vec<String> = ["-hide_banner", "-loglevel", "error", "-nostdin", "-progress", "pipe:1"].map(String::from).into();
    args.extend(["-threads".into(), editor::export_threads().to_string()]);
    for m in &made {
        args.extend(["-i".into(), ffmpeg::path_arg(&m.file)]);
    }
    // A part without sound (every track muted) gets silence of its length, as joining needs
    // sound from each.
    let mut next_input = made.len();
    let mut filters = Vec::new();
    let mut joined = String::new();
    for (i, m) in made.iter().enumerate() {
        filters.push(format!(
            "[{i}:v]scale={width}:{height}:force_original_aspect_ratio=decrease,pad={width}:{height}:(ow-iw)/2:(oh-ih)/2,setsar=1,fps={fps},format={}[v{i}]",
            encoder.export_pix_fmt
        ));
        let sound = if m.info.audio_channels.is_empty() {
            args.extend(["-f".into(), "lavfi".into(), "-t".into(), format!("{:.3}", m.info.duration), "-i".into(), "anullsrc=r=48000:cl=stereo".into()]);
            next_input += 1;
            format!("{}:a", next_input - 1)
        } else {
            format!("{i}:a:0")
        };
        filters.push(format!("[{sound}]aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo[a{i}]"));
        joined.push_str(&format!("[v{i}][a{i}]"));
    }
    filters.push(format!("{joined}concat=n={}:v=1:a=1[v][a]", made.len()));
    args.extend(["-filter_complex".into(), filters.join(";"), "-map".into(), "[v]".into(), "-map".into(), "[a]".into()]);
    args.extend(crate::encoder::args(encoder, 16_000));
    args.extend(["-c:a", "aac", "-b:a", "192k", "-movflags", "+faststart", "-y"].map(String::from));
    args.push(ffmpeg::path_arg(out));
    editor::run(&args, length, out, |p| on_progress((parts.len() as f64 + p) / steps), cancelled)?;

    let mut markers = Vec::new();
    let mut offset = 0.0;
    for m in &made {
        markers.extend(m.markers.iter().map(|k| Marker { at: offset + k.at, ..k.clone() }));
        offset += m.info.duration;
    }
    Ok((length, markers))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::TrackMeta;
    use std::process::Command;

    fn clip(dir: &Path, name: &str, size: &str, sound: bool, seconds: u32) -> PathBuf {
        let path = dir.join(name);
        let mut args: Vec<String> = ["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i"].map(String::from).into();
        args.push(format!("testsrc2=size={size}:rate=30"));
        if sound {
            args.extend(["-f", "lavfi", "-i", "sine=f=440:r=48000"].map(String::from));
        }
        args.extend(["-t".into(), seconds.to_string(), "-c:v".into(), "libx264".into(), "-preset".into(), "ultrafast".into()]);
        if sound {
            args.extend(["-c:a", "aac"].map(String::from));
        }
        args.push(path.to_string_lossy().into_owned());
        assert!(Command::new("ffmpeg").args(&args).status().unwrap().success());
        path
    }

    fn meta(title: &str, seconds: f64, sound: bool, draft: Option<Draft>) -> Meta {
        Meta {
            title: title.into(),
            duration: seconds,
            tracks: if sound { vec![TrackMeta { name: "Game".into(), kind: crate::audio::TrackKind::Game, exe: None }] } else { Vec::new() },
            draft,
            markers: vec![Marker::at(1.5)],
            ..Default::default()
        }
    }

    /// Clips of different sizes, one silent and one trimmed, join into one at the first
    /// clip's size, as long as what each exports, with a marker where each begins.
    #[test]
    fn clips_join_at_the_first_size() {
        if ffmpeg::missing_for_test() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("replix-montage-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let trimmed = Draft {
            trim_start: 1.0,
            trim_end: 3.0,
            tracks: vec![TrackEdit { enabled: true, volume: 1.0, start: None, end: None, removed: false }],
            cuts: Vec::new(),
            splits: Vec::new(),
            vertical: false,
            crop_x: 0.5,
            crop_y: 0.5,
            crop_scale: 1.0,
            master_volume: 1.0,
            speeds: Vec::new(),
            texts: Vec::new(),
        };
        let parts = [
            Part { source: clip(&dir, "a.mp4", "320x180", true, 3), meta: meta("First", 3.0, true, None) },
            Part { source: clip(&dir, "b.mp4", "160x120", false, 2), meta: meta("Second", 2.0, false, None) },
            Part { source: clip(&dir, "c.mp4", "320x180", true, 4), meta: meta("Third", 4.0, true, Some(trimmed)) },
        ];
        let out = dir.join("montage.mp4");
        let (length, markers) =
            combine(&parts, &[], crate::encoder::software(), &dir.join("work"), &out, |_| {}, &|| false).unwrap();
        let info = ffmpeg::probe(&out).unwrap();
        assert_eq!((info.width, info.height), (320, 180));
        assert_eq!(info.audio_channels.len(), 1);
        assert!((info.duration - 7.0).abs() < 0.3, "{}", info.duration);
        assert!((length - 7.0).abs() < 0.3);
        let names: Vec<(String, f64)> = markers.iter().map(|m| (m.name.clone(), (m.at * 10.0).round() / 10.0)).collect();
        // Each clip's start, and each clip's own marker at 1.5 s where it was kept: the
        // trimmed one starts at 1.0, so its marker lands 0.5 s into it.
        assert_eq!(
            names,
            [("First".into(), 0.0), ("".into(), 1.5), ("Second".into(), 3.0), ("".into(), 4.5), ("Third".into(), 5.0), ("".into(), 5.5)]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn markers_follow_trim_and_cuts() {
        let request = ExportRequest { cuts: vec![[2.0, 4.0]], ..request_for(&meta("x", 10.0, false, None), &[]) };
        let moved = markers_in(&request, &[Marker::at(1.0), Marker::at(3.0), Marker::at(5.0)]);
        assert_eq!(moved.iter().map(|m| m.at).collect::<Vec<_>>(), [1.0, 3.0]);
    }
}
