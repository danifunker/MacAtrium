//! `atrium snd` — turn a host WAV clip into a classic Mac sound file's resource
//! fork: a single `snd ` resource (id 128, format 1, sampled-sound) the launcher
//! plays with `SndPlay` for its startup / shutdown chime.
//!
//! Input is PCM WAV (8-bit unsigned or 16-bit signed, mono or stereo); we fold
//! to 8-bit unsigned mono at the source sample rate (what the classic Sound
//! Manager wants) and cap the clip at 7 seconds. `atrium image` writes the
//! resulting resource fork onto the volume with `rb-cli setrsrc`.

use anyhow::{anyhow, bail, Context, Result};
use std::path::Path;

/// Hard cap on clip length — kept short so the chime never delays boot/shutdown
/// much and the baked resource stays small.
pub const MAX_SECS: f32 = 7.0;

// ---- minimal RIFF/WAVE PCM parser -----------------------------------------
fn le_u16(b: &[u8], o: usize) -> u16 { u16::from_le_bytes([b[o], b[o + 1]]) }
fn le_u32(b: &[u8], o: usize) -> u32 { u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) }

struct Wav {
    rate: u32,
    channels: u16,
    bits: u16,
    data: Vec<u8>,
}

fn parse_wav(b: &[u8]) -> Result<Wav> {
    if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        bail!("not a RIFF/WAVE file");
    }
    let (mut rate, mut channels, mut bits, mut fmt) = (0u32, 0u16, 0u16, 0u16);
    let mut data: Option<Vec<u8>> = None;
    let mut p = 12;
    while p + 8 <= b.len() {
        let id = &b[p..p + 4];
        let sz = le_u32(b, p + 4) as usize;
        let body = p + 8;
        if body + sz > b.len() {
            break; // truncated chunk; stop
        }
        if id == b"fmt " && sz >= 16 {
            fmt = le_u16(b, body);
            channels = le_u16(b, body + 2);
            rate = le_u32(b, body + 4);
            bits = le_u16(b, body + 14);
        } else if id == b"data" {
            data = Some(b[body..body + sz].to_vec());
        }
        p = body + sz + (sz & 1); // chunks are word-aligned
    }
    if fmt != 1 {
        bail!("only PCM WAV (format 1) is supported (got format {fmt})");
    }
    let data = data.ok_or_else(|| anyhow!("WAV has no data chunk"))?;
    if channels == 0 || rate == 0 || (bits != 8 && bits != 16) {
        bail!("unsupported WAV: {channels}ch {rate}Hz {bits}-bit (need 8/16-bit PCM)");
    }
    Ok(Wav { rate, channels, bits, data })
}

/// Fold PCM frames to 8-bit unsigned mono (offset-binary, 0x80 = silence) by
/// averaging channels — the sample format the classic Sound Manager plays.
fn to_mono_u8(w: &Wav) -> Vec<u8> {
    let ch = w.channels as usize;
    let mut out = Vec::new();
    if w.bits == 8 {
        // 8-bit WAV is already unsigned; average the channels of each frame.
        for frame in w.data.chunks_exact(ch) {
            let sum: u32 = frame.iter().map(|&s| s as u32).sum();
            out.push((sum / ch as u32) as u8);
        }
    } else {
        // 16-bit signed little-endian; average, then map to 8-bit unsigned.
        let bytes_per_frame = ch * 2;
        for frame in w.data.chunks_exact(bytes_per_frame) {
            let mut sum = 0i32;
            for c in 0..ch {
                sum += i16::from_le_bytes([frame[c * 2], frame[c * 2 + 1]]) as i32;
            }
            let avg = sum / ch as i32;
            out.push(((avg >> 8) + 128).clamp(0, 255) as u8);
        }
    }
    out
}

/// A format-1 `snd ` resource: one sampled-sound `bufferCmd` whose SoundHeader
/// carries the 8-bit samples at `rate_hz`.
fn snd_format1(samples: &[u8], rate_hz: u32) -> Vec<u8> {
    let mut s = Vec::with_capacity(42 + samples.len());
    s.extend_from_slice(&1u16.to_be_bytes());       // format = 1
    s.extend_from_slice(&1u16.to_be_bytes());       // number of data formats
    s.extend_from_slice(&5u16.to_be_bytes());       // sampledSynth (= 5)
    s.extend_from_slice(&0u32.to_be_bytes());       // init option
    s.extend_from_slice(&1u16.to_be_bytes());       // number of commands
    s.extend_from_slice(&0x8051u16.to_be_bytes());  // bufferCmd | dataOffsetFlag
    s.extend_from_slice(&0u16.to_be_bytes());       // param1
    s.extend_from_slice(&20u32.to_be_bytes());      // param2 = offset to SoundHeader
    // SoundHeader (standard, encode 0x00):
    s.extend_from_slice(&0u32.to_be_bytes());       // samplePtr (0 = samples follow)
    s.extend_from_slice(&(samples.len() as u32).to_be_bytes()); // length
    s.extend_from_slice(&(rate_hz << 16).to_be_bytes());        // sampleRate (Fixed)
    s.extend_from_slice(&0u32.to_be_bytes());       // loopStart
    s.extend_from_slice(&0u32.to_be_bytes());       // loopEnd
    s.push(0x00);                                   // encode = stdSH
    s.push(60);                                     // baseFrequency = middle C
    s.extend_from_slice(samples);
    s
}

/// Wrap one `snd ` resource (id 128) into a complete classic resource fork. Data
/// starts at the conventional 256-byte offset; the map holds a single type with
/// a single reference.
fn build_resfork(snd: &[u8]) -> Vec<u8> {
    const DATA_OFF: usize = 256;
    let data_len = 4 + snd.len();
    let map_off = DATA_OFF + data_len;

    // Resource map: 24-byte header area, then typeList/nameList offsets + lists.
    let mut tl = Vec::new();
    tl.extend_from_slice(&0u16.to_be_bytes());      // (number of types) - 1
    tl.extend_from_slice(b"snd ");                  // type
    tl.extend_from_slice(&0u16.to_be_bytes());      // (count of this type) - 1
    tl.extend_from_slice(&10u16.to_be_bytes());     // ref list offset from type-list start
    tl.extend_from_slice(&128i16.to_be_bytes());    // resource id 128
    tl.extend_from_slice(&0xFFFFu16.to_be_bytes()); // name offset (none)
    tl.push(0);                                     // attributes
    tl.extend_from_slice(&[0u8, 0, 0]);             // u24 data offset (first/only)
    tl.extend_from_slice(&0u32.to_be_bytes());      // reserved (handle)

    let type_list_off: u16 = 28; // from the start of the map
    let name_list_off = type_list_off as usize + tl.len();
    let mut map = vec![0u8; 24];
    map.extend_from_slice(&type_list_off.to_be_bytes());
    map.extend_from_slice(&(name_list_off as u16).to_be_bytes());
    map.extend_from_slice(&tl);

    let mut out = vec![0u8; DATA_OFF];
    out[0..4].copy_from_slice(&(DATA_OFF as u32).to_be_bytes());
    out[4..8].copy_from_slice(&(map_off as u32).to_be_bytes());
    out[8..12].copy_from_slice(&(data_len as u32).to_be_bytes());
    out[12..16].copy_from_slice(&(map.len() as u32).to_be_bytes());
    out.extend_from_slice(&(snd.len() as u32).to_be_bytes()); // data: resource length
    out.extend_from_slice(snd);
    out.extend_from_slice(&map);
    out
}

/// Build the resource fork bytes for a sound file from a WAV, plus the clip's
/// original duration in seconds (before the 7-second cap) so the caller can warn.
pub fn build_resfork_from_wav(wav: &Path) -> Result<(Vec<u8>, f32)> {
    let bytes = std::fs::read(wav).with_context(|| format!("reading {}", wav.display()))?;
    let w = parse_wav(&bytes)?;
    let mut samples = to_mono_u8(&w);
    let secs = samples.len() as f32 / w.rate as f32;
    let cap = (w.rate as f32 * MAX_SECS) as usize;
    if samples.len() > cap {
        samples.truncate(cap);
    }
    let snd = snd_format1(&samples, w.rate);
    Ok((build_resfork(&snd), secs))
}

/// `atrium snd`: bake a WAV into a sound file's resource fork on the host.
pub fn run(wav: &Path, out: &Path) -> Result<()> {
    let (rsrc, secs) = build_resfork_from_wav(wav)?;
    std::fs::write(out, &rsrc).with_context(|| format!("writing {}", out.display()))?;
    eprintln!(
        "snd: {:.1}s clip -> {} ({} bytes resource fork){}",
        secs,
        out.display(),
        rsrc.len(),
        if secs > MAX_SECS { format!("; truncated to {MAX_SECS:.0}s") } else { String::new() }
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny 16-bit mono WAV (one cycle) for round-trip structure checks.
    fn tiny_wav(rate: u32, frames: usize) -> Vec<u8> {
        let data_len = frames * 2;
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data_len as u32).to_le_bytes());
        b.extend_from_slice(b"WAVE");
        b.extend_from_slice(b"fmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes()); // PCM
        b.extend_from_slice(&1u16.to_le_bytes()); // mono
        b.extend_from_slice(&rate.to_le_bytes());
        b.extend_from_slice(&(rate * 2).to_le_bytes()); // byte rate
        b.extend_from_slice(&2u16.to_le_bytes()); // block align
        b.extend_from_slice(&16u16.to_le_bytes()); // bits
        b.extend_from_slice(b"data");
        b.extend_from_slice(&(data_len as u32).to_le_bytes());
        for i in 0..frames {
            let v = ((i as i32 % 100) * 200 - 10000) as i16;
            b.extend_from_slice(&v.to_le_bytes());
        }
        b
    }

    #[test]
    fn parses_and_folds_to_8bit() {
        let wav = tiny_wav(11025, 50);
        let w = parse_wav(&wav).unwrap();
        assert_eq!(w.rate, 11025);
        assert_eq!(w.bits, 16);
        let mono = to_mono_u8(&w);
        assert_eq!(mono.len(), 50);
    }

    #[test]
    fn caps_at_seven_seconds() {
        let rate = 11025u32;
        let wav = tiny_wav(rate, (rate as usize) * 10); // 10s of audio
        // Mirror build_resfork_from_wav on in-memory bytes (no temp file).
        let w = parse_wav(&wav).unwrap();
        let mut s = to_mono_u8(&w);
        let secs = s.len() as f32 / w.rate as f32;
        let cap = (w.rate as f32 * MAX_SECS) as usize;
        s.truncate(cap);
        let rsrc = build_resfork(&snd_format1(&s, w.rate));
        assert!(secs > MAX_SECS);          // original clip was over the cap
        assert_eq!(s.len(), cap);          // samples truncated to 7s
        // resource fork = 256 header + 4 (data length) + snd (42 + samples) + map.
        assert!(rsrc.len() >= 256 + 4 + 42 + cap);
    }

    #[test]
    fn resfork_header_offsets() {
        let rsrc = build_resfork(&snd_format1(&[0x80u8; 8], 11025));
        // header: dataOffset 256, then map after data.
        assert_eq!(u32::from_be_bytes([rsrc[0], rsrc[1], rsrc[2], rsrc[3]]), 256);
        let data_len = u32::from_be_bytes([rsrc[8], rsrc[9], rsrc[10], rsrc[11]]) as usize;
        let map_off = u32::from_be_bytes([rsrc[4], rsrc[5], rsrc[6], rsrc[7]]) as usize;
        assert_eq!(map_off, 256 + data_len);
        // type-list offset (map+24) is 28; the type four bytes in is "snd ".
        assert_eq!(u16::from_be_bytes([rsrc[map_off + 24], rsrc[map_off + 25]]), 28);
        assert_eq!(&rsrc[map_off + 30..map_off + 34], b"snd ");
    }
}
