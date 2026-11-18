//! Finishing the header of a Matroska file that broke off.
//!
//! FFmpeg writes a session's file as it goes, but leaves two things in its header open until
//! it closes the file: how long the recording is, and how its AAC sound is encoded. A file
//! that a crash or a power cut broke off has neither. Without the length the library shows
//! 0:00 and players cannot seek; without the description of the sound FFmpeg guesses a kind
//! of AAC (LTP) that many players cannot play, and an MP4 made from it carries that guess on.
//! FFmpeg keeps room for both in the header, so they are written into that room here, as
//! FFmpeg itself would have done on closing. Nothing else in the file is touched.

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

const EBML: u32 = 0x1A45_DFA3;
const SEGMENT: u32 = 0x1853_8067;
const INFO: u32 = 0x1549_A966;
const TRACKS: u32 = 0x1654_AE6B;
const CLUSTER: u32 = 0x1F43_B675;
const DURATION: u32 = 0x4489;
const TRACK_ENTRY: u32 = 0xAE;
const CODEC_ID: u32 = 0x86;
const CODEC_PRIVATE: u32 = 0x63A2;
const AUDIO: u32 = 0xE1;
const SAMPLING_FREQUENCY: u32 = 0xB5;
const CHANNELS: u32 = 0x9F;
const TIMESTAMP: u32 = 0xE7;
const SIMPLE_BLOCK: u32 = 0xA3;
const BLOCK_GROUP: u32 = 0xA0;
const BLOCK: u32 = 0xA1;
const VOID: u32 = 0xEC;
const CRC32: u32 = 0xBF;

/// How much of the start is read for the header; FFmpeg's takes a few kilobytes.
const HEAD: u64 = 1 << 20;
/// How much of the end is searched for the last cluster: several seconds even at a high bitrate.
const TAIL: u64 = 32 << 20;
/// A Duration element: its ID, its size and a 64-bit float.
const DURATION_LEN: usize = 11;

/// Writes the length of a file that broke off, and the description of its AAC tracks, into
/// the room FFmpeg kept for them. A file that was closed properly has both and is left as it
/// is. True when something was written.
pub(super) fn seal(file: &Path) -> std::io::Result<bool> {
    let mut f = std::fs::OpenOptions::new().read(true).write(true).open(file)?;
    let len = f.metadata()?.len();
    let mut head = vec![0; len.min(HEAD) as usize];
    f.read_exact(&mut head)?;
    let Some(mut plan) = plan(&head) else { return Ok(false) };
    if let (Some(room), Some(clusters)) = (plan.duration_room, plan.clusters) {
        let from = (clusters as u64).max(len.saturating_sub(TAIL));
        let mut tail = vec![0; (len - from) as usize];
        f.seek(SeekFrom::Start(from))?;
        f.read_exact(&mut tail)?;
        // Duration counts in the same units as the clusters' timestamps.
        if let Some(last) = last_timestamp(&tail) {
            let mut duration = vec![0x44, 0x89, 0x88];
            duration.extend_from_slice(&(last as f64).to_be_bytes());
            plan.writes.extend(fill(room, &duration));
        }
    }
    if plan.writes.is_empty() {
        return Ok(false);
    }
    for (at, bytes) in &plan.writes {
        f.seek(SeekFrom::Start(*at as u64))?;
        f.write_all(bytes)?;
    }
    f.sync_data()?;
    Ok(true)
}

/// An element of the file: its ID (with its length marker, as the specification writes it),
/// where it starts, where its content starts, and where it ends; no end for "unknown size".
#[derive(Debug, Clone, Copy)]
struct Element {
    id: u32,
    start: usize,
    data: usize,
    end: Option<usize>,
}

fn element(b: &[u8], at: usize) -> Option<Element> {
    let first = *b.get(at)?;
    let id_len = first.leading_zeros() as usize + 1;
    if id_len > 4 {
        return None;
    }
    let id = b.get(at..at + id_len)?.iter().fold(0u32, |v, &x| (v << 8) | u32::from(x));
    let (size, size_len) = vint(b, at + id_len)?;
    let data = at + id_len + size_len;
    let end = match size {
        Some(size) => Some(data.checked_add(usize::try_from(size).ok()?)?),
        None => None,
    };
    Some(Element { id, start: at, data, end })
}

/// A number of variable length and how many bytes it takes; no number for "unknown".
fn vint(b: &[u8], at: usize) -> Option<(Option<u64>, usize)> {
    let first = *b.get(at)?;
    let len = first.leading_zeros() as usize + 1;
    if len > 8 {
        return None;
    }
    let bytes = b.get(at..at + len)?;
    let value = bytes[1..].iter().fold(u64::from(first & (0xFF_u16 >> len) as u8), |v, &x| (v << 8) | u64::from(x));
    let unknown = value == (1u64 << (7 * len)) - 1;
    Some(((!unknown).then_some(value), len))
}

/// The elements inside `parent`, which has to lie wholly within `b`.
fn children(b: &[u8], parent: Element) -> Option<Vec<Element>> {
    let end = parent.end.filter(|end| *end <= b.len())?;
    let mut out = Vec::new();
    let mut at = parent.data;
    while at < end {
        let e = element(b, at)?;
        at = e.end.filter(|e_end| *e_end <= end)?;
        out.push(e);
    }
    Some(out)
}

fn content(b: &[u8], e: Element) -> Option<&[u8]> {
    b.get(e.data..e.end?)
}

fn uint(b: &[u8], e: Element) -> Option<u64> {
    let data = content(b, e).filter(|d| d.len() <= 8)?;
    Some(data.iter().fold(0, |v, &x| (v << 8) | u64::from(x)))
}

fn float(b: &[u8], e: Element) -> Option<f64> {
    let data = content(b, e)?;
    match data.len() {
        4 => Some(f64::from(f32::from_be_bytes(data.try_into().ok()?))),
        8 => Some(f64::from_be_bytes(data.try_into().ok()?)),
        _ => None,
    }
}

/// What to write where, and the room for the length.
#[derive(Default)]
struct Plan {
    writes: Vec<(usize, Vec<u8>)>,
    duration_room: Option<Element>,
    /// Where the first cluster starts: the recording itself.
    clusters: Option<usize>,
}

/// Reads the header at the start of `b` and plans what is missing from it.
fn plan(b: &[u8]) -> Option<Plan> {
    let ebml = element(b, 0).filter(|e| e.id == EBML)?;
    let segment = element(b, ebml.end?).filter(|e| e.id == SEGMENT)?;
    let mut plan = Plan::default();
    let mut at = segment.data;
    while let Some(e) = element(b, at) {
        match e.id {
            CLUSTER => {
                plan.clusters = Some(e.start);
                break;
            }
            INFO => plan_info(b, e, &mut plan),
            TRACKS => plan_tracks(b, e, &mut plan),
            _ => {}
        }
        match e.end {
            Some(end) => at = end,
            None => break,
        }
    }
    Some(plan)
}

fn plan_info(b: &[u8], info: Element, plan: &mut Plan) {
    let Some(parts) = children(b, info) else { return };
    // A checksum is only there once the file was closed; it would no longer match.
    if parts.iter().any(|p| p.id == CRC32 || p.id == DURATION) {
        return;
    }
    plan.duration_room = parts.iter().find(|p| p.id == VOID && p.end.is_some_and(|end| end - p.start >= DURATION_LEN)).copied();
}

fn plan_tracks(b: &[u8], tracks: Element, plan: &mut Plan) {
    let Some(entries) = children(b, tracks) else { return };
    if entries.iter().any(|e| e.id == CRC32) {
        return;
    }
    for entry in entries.iter().filter(|e| e.id == TRACK_ENTRY) {
        let Some(parts) = children(b, *entry) else { continue };
        let codec = parts.iter().find(|p| p.id == CODEC_ID).and_then(|p| content(b, *p));
        let aac = codec.is_some_and(|c| c.strip_suffix(&[0]).unwrap_or(c) == b"A_AAC");
        if !aac || parts.iter().any(|p| p.id == CODEC_PRIVATE) {
            continue;
        }
        // The defaults are the specification's.
        let audio = parts.iter().find(|p| p.id == AUDIO).and_then(|p| children(b, *p)).unwrap_or_default();
        let rate = audio.iter().find(|a| a.id == SAMPLING_FREQUENCY).and_then(|a| float(b, *a)).unwrap_or(8000.0);
        let channels = audio.iter().find(|a| a.id == CHANNELS).and_then(|a| uint(b, *a)).unwrap_or(1);
        let (Some(config), Some(room)) = (aac_config(rate, channels), parts.iter().find(|p| p.id == VOID)) else { continue };
        let mut private = vec![0x63, 0xA2, 0x80 | config.len() as u8];
        private.extend_from_slice(&config);
        plan.writes.extend(fill(*room, &private));
    }
}

/// The description of AAC-LC, which is what Replix records, at this rate and channel count.
fn aac_config(rate: f64, channels: u64) -> Option<[u8; 2]> {
    const RATES: [u32; 13] = [96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350];
    let index = RATES.iter().position(|r| f64::from(*r) == rate)? as u16;
    let layout = match channels {
        1..=6 => channels as u16,
        8 => 7,
        _ => return None,
    };
    // Object type 2 (LC), the rate's index, the channel layout, three bits left at zero.
    Some(((2 << 11) | (index << 7) | (layout << 3)).to_be_bytes())
}

/// `content` in the room a Void element takes, with what is left kept void. None if it does
/// not fit, or if one byte would be left: too little for an element.
fn fill(void: Element, content: &[u8]) -> Option<(usize, Vec<u8>)> {
    let room = void.end? - void.start;
    let rest = room.checked_sub(content.len())?;
    let mut out = content.to_vec();
    match rest {
        0 => {}
        1 => return None,
        2..=128 => out.extend([0xEC, 0x80 | (rest - 2) as u8]),
        _ => {
            out.extend([0xEC, 0x01]);
            out.extend_from_slice(&((rest - 9) as u64).to_be_bytes()[1..]);
        }
    }
    out.resize(room, 0);
    Some((void.start, out))
}

/// The timestamp of the last frame in `b`, the end of the file: from its last cluster that
/// holds a frame. One that the break cut off counts up to its last whole frame.
fn last_timestamp(b: &[u8]) -> Option<u64> {
    let mut before = b.len();
    while let Some(at) = b[..before].windows(4).rposition(|w| w == [0x1F, 0x43, 0xB6, 0x75]) {
        if let Some(time) = cluster_end(b, at) {
            return Some(time);
        }
        before = at;
    }
    None
}

/// The latest frame's timestamp in the cluster at `at`, if there is a cluster there (the
/// bytes may also just happen to look like the start of one) and it holds a frame.
fn cluster_end(b: &[u8], at: usize) -> Option<u64> {
    let cluster = element(b, at).filter(|e| e.id == CLUSTER)?;
    let end = cluster.end.map_or(b.len(), |end| end.min(b.len()));
    // The cluster's own timestamp comes before its frames; FFmpeg puts a checksum first.
    let mut base = None;
    let mut latest: Option<i64> = None;
    let mut pos = cluster.data;
    while pos < end {
        let Some(e) = element(b, pos) else { break };
        let Some(e_end) = e.end.filter(|x| *x <= end) else { break };
        match e.id {
            TIMESTAMP => base = uint(b, e),
            _ if base.is_none() => {}
            SIMPLE_BLOCK => latest = latest.max(block_time(b, e)),
            BLOCK_GROUP => {
                for block in children(b, e).unwrap_or_default().into_iter().filter(|k| k.id == BLOCK) {
                    latest = latest.max(block_time(b, block));
                }
            }
            _ => {}
        }
        pos = e_end;
    }
    u64::try_from(i64::try_from(base?).ok()?.checked_add(latest?)?).ok()
}

/// A block's timestamp, which counts from its cluster's.
fn block_time(b: &[u8], block: Element) -> Option<i64> {
    let (_, track_len) = vint(b, block.data)?;
    let at = block.data + track_len;
    let time = b.get(at..at + 2)?;
    Some(i64::from(i16::from_be_bytes([time[0], time[1]])))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_of_variable_length_are_read() {
        assert_eq!(vint(&[0x81], 0), Some((Some(1), 1)));
        assert_eq!(vint(&[0x40, 0x02], 0), Some((Some(2), 2)));
        assert_eq!(vint(&[0x01, 0, 0, 0, 0, 0, 0x01, 0x3B], 0), Some((Some(0x13B), 8)));
        assert_eq!(vint(&[0xFF], 0), Some((None, 1)), "unknown");
        assert_eq!(vint(&[0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF], 0), Some((None, 8)));
        assert_eq!(vint(&[0x00], 0), None);
        assert_eq!(vint(&[0x40], 0), None, "cut off");
    }

    #[test]
    fn aac_lc_is_described_by_rate_and_channels() {
        assert_eq!(aac_config(48000.0, 2), Some([0x11, 0x90]));
        assert_eq!(aac_config(44100.0, 1), Some([0x12, 0x08]));
        assert_eq!(aac_config(48000.0, 8), Some([0x11, 0xB8]));
        assert_eq!(aac_config(12345.0, 2), None);
        assert_eq!(aac_config(48000.0, 7), None);
    }

    /// What goes into a Void fills it exactly, and what is left reads as a Void again.
    #[test]
    fn filling_a_void_keeps_its_size() {
        for room in [5, 7, 11, 40, 130, 140, 324] {
            let void = Element { id: VOID, start: 0, data: 9, end: Some(room) };
            let content = [0x63, 0xA2, 0x82, 0x11, 0x90];
            match fill(void, &content) {
                Some((at, bytes)) => {
                    assert_eq!((at, bytes.len()), (0, room));
                    assert_eq!(&bytes[..5], &content);
                    if room > 5 {
                        let rest = element(&bytes, 5).unwrap();
                        assert_eq!((rest.id, rest.end), (VOID, Some(room)), "room {room}");
                    }
                }
                None => assert_eq!(room, 6, "only one byte left over cannot be filled"),
            }
        }
        let void = Element { id: VOID, start: 0, data: 2, end: Some(6) };
        assert!(fill(void, &[0x63, 0xA2, 0x82, 0x11, 0x90]).is_none());
        assert!(fill(Element { end: Some(4), ..void }, &[0; 5]).is_none());
    }

    /// The last cluster counts, also one cut off in the middle, with or without a checksum
    /// first; bytes that only look like the start of a cluster do not, nor do the zeros a
    /// power cut can leave at the end of a file.
    #[test]
    fn the_end_comes_from_the_last_cluster() {
        let block = |time: i16| {
            let [hi, lo] = time.to_be_bytes();
            vec![0xA3, 0x86, 0x81, hi, lo, 0x80, 0xAA, 0xBB]
        };
        let cluster = |base: u8, blocks: &[i16]| {
            let checksum = vec![0xBF, 0x84, 1, 2, 3, 4];
            let stamp = vec![0xE7, 0x81, base];
            let body: Vec<u8> = [checksum, stamp].into_iter().chain(blocks.iter().map(|t| block(*t))).flatten().collect();
            [vec![0x1F, 0x43, 0xB6, 0x75, 0x80 | body.len() as u8], body].concat()
        };
        let whole = [cluster(0, &[0, 20, 40]), cluster(100, &[0, 20, 33])].concat();
        assert_eq!(last_timestamp(&whole), Some(133));
        // A cluster that broke off after its first block.
        let cut = cluster(200, &[5, 30]);
        assert_eq!(last_timestamp(&[whole.clone(), cut[..cut.len() - 4].to_vec()].concat()), Some(205));
        // One cut off before any block leaves the one before.
        assert_eq!(last_timestamp(&[whole.clone(), cluster(250, &[0])[..14].to_vec()].concat()), Some(133));
        // Something that looks like a cluster but is not one, and zeros.
        assert_eq!(last_timestamp(&[whole.clone(), vec![0x1F, 0x43, 0xB6, 0x75, 0x10, 0x00]].concat()), Some(133));
        assert_eq!(last_timestamp(&[whole, vec![0; 4096]].concat()), Some(133));
        assert_eq!(last_timestamp(&[0; 64]), None);
    }
}
