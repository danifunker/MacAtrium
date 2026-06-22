//! `atrium pict` — convert PNG/JPEG artwork to classic-Mac **PICT** (docs/06).
//!
//! QuickDraw can `DrawPicture` a PICT directly; there's no PNG/JPEG decoder on
//! 68k. We emit **PICT v2** (a 512-byte file header + the picture data):
//!   • 1/4/8-bit → indexed `PackBitsRect` (0x0098) with an embedded colour table
//!     (PackBits-compressed rows),
//!   • 16-bit → `DirectBitsRect` (0x009A), 1-5-5-5 "thousands" pixels, and
//!   • 24-bit → `DirectBitsRect` (0x009A), 8-8-8 "millions" in 32-bit pixels.
//! Indexed depths use adaptive median-cut palettes (B/W for 1-bit via an ordered
//! Bayer dither). A single higher-depth file can serve shallower screens too —
//! QuickDraw down-converts at draw time (the launcher picks the best variant; see
//! docs/15) — so a 24-bit master + a 1-bit raw covers everything.

use anyhow::{Context, Result};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Depth {
    One,
    Four,
    Eight,
    Sixteen,
    /// "Millions": a 32-bit-pixelSize DirectBits pixmap carrying 24 bits of real
    /// colour (cmpCount 3 × cmpSize 8; the high byte is unused). Exposed as "24".
    TwentyFour,
}

impl Depth {
    pub fn parse(s: &str) -> Result<Depth> {
        Ok(match s {
            "1" => Depth::One,
            "4" => Depth::Four,
            "8" => Depth::Eight,
            "16" => Depth::Sixteen,
            "24" | "32" => Depth::TwentyFour,
            _ => anyhow::bail!("depth must be 1, 4, 8, 16, or 24"),
        })
    }
    /// Logical depth used for labels and the `<id>.<bits>.pict` variant suffix.
    pub fn bits(self) -> u16 {
        match self {
            Depth::One => 1,
            Depth::Four => 4,
            Depth::Eight => 8,
            Depth::Sixteen => 16,
            Depth::TwentyFour => 24,
        }
    }
    /// 1/4/8-bit are indexed (PackBitsRect + colour table); 16/24-bit are direct.
    fn is_indexed(self) -> bool {
        matches!(self, Depth::One | Depth::Four | Depth::Eight)
    }
    /// PixMap `pixelSize` — note 24-bit "Millions" is stored in 32-bit pixels.
    fn pixel_size(self) -> u16 {
        match self {
            Depth::TwentyFour => 32,
            d => d.bits(),
        }
    }
    /// PixMap (cmpCount, cmpSize): indexed = (1, bits); 16-bit = (3, 5) for
    /// 1-5-5-5; 24-bit = (3, 8) for 8-8-8.
    fn components(self) -> (u16, u16) {
        match self {
            Depth::Sixteen => (3, 5),
            Depth::TwentyFour => (3, 8),
            d => (1, d.bits()),
        }
    }
}

pub struct Stats {
    pub width: u16,
    pub height: u16,
    pub depth: u16,
    pub colors: usize,
    pub bytes: usize,
}

// ---- big-endian helpers ----------------------------------------------------
fn be16(v: u16) -> [u8; 2] {
    v.to_be_bytes()
}
fn be32(v: u32) -> [u8; 4] {
    v.to_be_bytes()
}
fn rect(out: &mut Vec<u8>, t: u16, l: u16, b: u16, r: u16) {
    out.extend(be16(t));
    out.extend(be16(l));
    out.extend(be16(b));
    out.extend(be16(r));
}

// ---- palettes --------------------------------------------------------------
const PAL_1: [(u8, u8, u8); 2] = [(255, 255, 255), (0, 0, 0)];

fn luma(r: u8, g: u8, b: u8) -> u32 {
    (30 * r as u32 + 59 * g as u32 + 11 * b as u32) / 100
}

fn nearest(palette: &[(u8, u8, u8)], r: u8, g: u8, b: u8) -> u8 {
    let mut best = 0usize;
    let mut bestd = u32::MAX;
    for (i, &(pr, pg, pb)) in palette.iter().enumerate() {
        let dr = pr as i32 - r as i32;
        let dg = pg as i32 - g as i32;
        let db = pb as i32 - b as i32;
        let d = (dr * dr + dg * dg + db * db) as u32;
        if d < bestd {
            bestd = d;
            best = i;
        }
    }
    best as u8
}

/// 4×4 ordered (Bayer) dither matrix, normalised to 0..255 thresholds.
const BAYER4: [[u8; 4]; 4] = [
    [0, 8, 2, 10],
    [12, 4, 14, 6],
    [3, 11, 1, 9],
    [15, 7, 13, 5],
];

// ---- PackBits --------------------------------------------------------------
fn packbits(src: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(src.len());
    let n = src.len();
    let mut i = 0;
    while i < n {
        let mut run = 1;
        while i + run < n && src[i + run] == src[i] && run < 128 {
            run += 1;
        }
        if run >= 3 {
            out.push(((257 - run) & 0xFF) as u8); // -(run-1)
            out.push(src[i]);
            i += run;
        } else {
            let start = i;
            let mut lit = 0;
            while i < n && lit < 128 {
                // stop if a run of >=3 begins here
                let mut r = 1;
                while i + r < n && src[i + r] == src[i] && r < 3 {
                    r += 1;
                }
                if r >= 3 {
                    break;
                }
                i += 1;
                lit += 1;
            }
            out.push((lit - 1) as u8);
            out.extend_from_slice(&src[start..start + lit]);
        }
    }
    out
}

/// Pack a row of palette indices into `rowbytes` bytes at `bits` per pixel
/// (MSB-first within each byte).
fn pack_row(indices: &[u8], w: usize, bits: u16, rowbytes: usize) -> Vec<u8> {
    let mut row = vec![0u8; rowbytes];
    match bits {
        8 => row[..w].copy_from_slice(&indices[..w]),
        4 => {
            for x in 0..w {
                let byte = x / 2;
                if x % 2 == 0 {
                    row[byte] |= (indices[x] & 0x0F) << 4;
                } else {
                    row[byte] |= indices[x] & 0x0F;
                }
            }
        }
        1 => {
            for x in 0..w {
                if indices[x] != 0 {
                    row[x / 8] |= 0x80 >> (x % 8);
                }
            }
        }
        _ => unreachable!(),
    }
    row
}

/// Quantise an RGBA8 buffer to palette indices for the given depth.
fn quantize(rgba: &[u8], w: usize, h: usize, depth: Depth) -> (Vec<u8>, Vec<(u8, u8, u8)>) {
    let px = |x: usize, y: usize| {
        let o = (y * w + x) * 4;
        (rgba[o], rgba[o + 1], rgba[o + 2])
    };
    match depth {
        Depth::One => {
            let mut idx = vec![0u8; w * h];
            for y in 0..h {
                for x in 0..w {
                    let (r, g, b) = px(x, y);
                    // thresholds 8..248 so pure white stays white, pure black black
                    let t = BAYER4[y % 4][x % 4] as u32 * 16 + 8;
                    idx[y * w + x] = if luma(r, g, b) > t { 0 } else { 1 };
                }
            }
            (idx, PAL_1.to_vec())
        }
        // Adaptive median-cut palette built from the image's own colours.
        Depth::Four => map_to_palette(rgba, w, h, &median_cut(sample_rgb(rgba, w * h), 16)),
        Depth::Eight => map_to_palette(rgba, w, h, &median_cut(sample_rgb(rgba, w * h), 256)),
        Depth::Sixteen | Depth::TwentyFour => unreachable!("16/24-bit are direct, not indexed"),
    }
}

fn map_to_palette(
    rgba: &[u8],
    w: usize,
    h: usize,
    palette: &[(u8, u8, u8)],
) -> (Vec<u8>, Vec<(u8, u8, u8)>) {
    let mut idx = vec![0u8; w * h];
    for i in 0..w * h {
        let o = i * 4;
        idx[i] = nearest(palette, rgba[o], rgba[o + 1], rgba[o + 2]);
    }
    (idx, palette.to_vec())
}

/// Sample up to ~16k RGB pixels from an RGBA buffer (for palette generation).
fn sample_rgb(rgba: &[u8], n_px: usize) -> Vec<[u8; 3]> {
    let step = (n_px / 16384).max(1);
    let mut v = Vec::new();
    let mut i = 0;
    while i < n_px {
        let o = i * 4;
        v.push([rgba[o], rgba[o + 1], rgba[o + 2]]);
        i += step;
    }
    v
}

/// Median-cut colour quantisation: recursively split the colour box with the
/// widest channel at its median until we have `n` boxes, then average each box.
/// Returns ≤ `n` palette colours (fewer if the image has fewer distinct colours).
fn median_cut(mut px: Vec<[u8; 3]>, n: usize) -> Vec<(u8, u8, u8)> {
    if px.is_empty() {
        return vec![(0, 0, 0)];
    }
    struct Bx {
        s: usize,
        e: usize,
    }
    let mut boxes = vec![Bx { s: 0, e: px.len() }];
    while boxes.len() < n {
        // Pick the splittable box with the widest channel range.
        let mut best: Option<usize> = None;
        let mut best_range = 0i32;
        let mut best_chan = 0usize;
        for (bi, b) in boxes.iter().enumerate() {
            if b.e - b.s < 2 {
                continue;
            }
            let mut lo = [255i32; 3];
            let mut hi = [0i32; 3];
            for p in &px[b.s..b.e] {
                for c in 0..3 {
                    let v = p[c] as i32;
                    lo[c] = lo[c].min(v);
                    hi[c] = hi[c].max(v);
                }
            }
            for c in 0..3 {
                let r = hi[c] - lo[c];
                if r > best_range {
                    best_range = r;
                    best = Some(bi);
                    best_chan = c;
                }
            }
        }
        let Some(bi) = best else { break }; // nothing left to split
        let (s, e) = (boxes[bi].s, boxes[bi].e);
        px[s..e].sort_by_key(|p| p[best_chan]);
        let mid = s + (e - s) / 2;
        boxes[bi] = Bx { s, e: mid };
        boxes.push(Bx { s: mid, e });
    }
    boxes
        .iter()
        .map(|b| {
            let cnt = (b.e - b.s) as u64;
            let (mut r, mut g, mut bl) = (0u64, 0u64, 0u64);
            for p in &px[b.s..b.e] {
                r += p[0] as u64;
                g += p[1] as u64;
                bl += p[2] as u64;
            }
            if cnt == 0 {
                (0, 0, 0)
            } else {
                ((r / cnt) as u8, (g / cnt) as u8, (bl / cnt) as u8)
            }
        })
        .collect()
}

// ---- picture assembly ------------------------------------------------------
fn write_pixmap_fields(out: &mut Vec<u8>, rowbytes: u16, w: u16, h: u16, depth: Depth, pack_type: u16) {
    let (cmp_count, cmp_size) = depth.components();
    out.extend(be16(rowbytes | 0x8000)); // high bit => PixMap (not old BitMap)
    rect(out, 0, 0, h, w); // bounds
    out.extend(be16(0)); // pmVersion
    out.extend(be16(pack_type)); // 0=default(PackBits), 1=unpacked
    out.extend(be32(0)); // packSize
    out.extend(be32(0x0048_0000)); // hRes 72.0
    out.extend(be32(0x0048_0000)); // vRes 72.0
    out.extend(be16(if depth.is_indexed() { 0 } else { 16 })); // pixelType: 0=indexed,16=RGBDirect
    out.extend(be16(depth.pixel_size())); // pixelSize (32 for 24-bit "Millions")
    out.extend(be16(cmp_count));
    out.extend(be16(cmp_size));
    out.extend(be32(0)); // planeBytes
    out.extend(be32(0)); // pmTable
    out.extend(be32(0)); // pmReserved
}

fn write_color_table(out: &mut Vec<u8>, palette: &[(u8, u8, u8)]) {
    out.extend(be32(0)); // ctSeed
    out.extend(be16(0)); // ctFlags
    out.extend(be16((palette.len() - 1) as u16)); // ctSize = n-1
    for (i, &(r, g, b)) in palette.iter().enumerate() {
        out.extend(be16(i as u16)); // value = index
        out.extend(be16((r as u16) << 8 | r as u16)); // 8-bit -> 16-bit
        out.extend(be16((g as u16) << 8 | g as u16));
        out.extend(be16((b as u16) << 8 | b as u16));
    }
}

fn encode_indexed(w: u16, h: u16, rgba: &[u8], depth: Depth, pack: bool) -> (Vec<u8>, usize) {
    let (wi, hi) = (w as usize, h as usize);
    let (idx, palette) = quantize(rgba, wi, hi, depth);
    let bits = depth.bits();
    // Classic QuickDraw requires EVEN rowBytes for a (Pix)Map; an odd value
    // (e.g. a 180px-wide 1-bit image -> 23) makes DrawPicture hang. Pad to even;
    // the extra byte is unused padding (bounds width is unchanged).
    let rowbytes = ((wi * bits as usize + 7) / 8 + 1) & !1usize;
    // PackBitsRect rows are packed only when rowBytes >= 8 AND packing is on;
    // packType 0 = (default) PackBits, 1 = unpacked raw rows.
    let do_pack = pack && rowbytes >= 8;

    let mut out = Vec::new();
    out.extend(be16(0x0098)); // PackBitsRect
    write_pixmap_fields(&mut out, rowbytes as u16, w, h, depth, if do_pack { 0 } else { 1 });
    write_color_table(&mut out, &palette);
    rect(&mut out, 0, 0, h, w); // srcRect
    rect(&mut out, 0, 0, h, w); // dstRect
    out.extend(be16(0)); // mode = srcCopy

    for y in 0..hi {
        let raw = pack_row(&idx[y * wi..(y + 1) * wi], wi, bits, rowbytes);
        if !do_pack {
            out.extend(raw); // unpacked: raw rows, no count
        } else {
            let packed = packbits(&raw);
            if rowbytes > 250 {
                out.extend(be16(packed.len() as u16));
            } else {
                out.push(packed.len() as u8);
            }
            out.extend(packed);
        }
    }
    (out, palette.len())
}

/// Direct (true-colour) PixMap: 16-bit 1-5-5-5 ("Thousands") or 24-bit 8-8-8 in
/// 32-bit pixels ("Millions"). packType=1 (unpacked) raw rows — the launcher's
/// off-screen GWorld down-converts to whatever the screen depth is.
fn encode_direct(w: u16, h: u16, rgba: &[u8], depth: Depth) -> (Vec<u8>, usize) {
    let (wi, hi) = (w as usize, h as usize);
    let bytes_pp = if depth == Depth::TwentyFour { 4 } else { 2 };
    let rowbytes = wi * bytes_pp;

    let mut out = Vec::new();
    out.extend(be16(0x009A)); // DirectBitsRect
    out.extend(be32(0x0000_00FF)); // pseudo baseAddr for DirectBits
    write_pixmap_fields(&mut out, rowbytes as u16, w, h, depth, 1);
    rect(&mut out, 0, 0, h, w); // srcRect
    rect(&mut out, 0, 0, h, w); // dstRect
    out.extend(be16(0)); // mode = srcCopy

    // packType=1 (unpacked): raw rows, no count.
    for y in 0..hi {
        for x in 0..wi {
            let o = (y * wi + x) * 4;
            if depth == Depth::TwentyFour {
                // 32-bit pixel: unused high byte, then R, G, B (8-8-8).
                out.push(0);
                out.push(rgba[o]);
                out.push(rgba[o + 1]);
                out.push(rgba[o + 2]);
            } else {
                // 16-bit 1-5-5-5 big-endian word.
                let r5 = (rgba[o] >> 3) as u16;
                let g5 = (rgba[o + 1] >> 3) as u16;
                let b5 = (rgba[o + 2] >> 3) as u16;
                out.extend(be16((r5 << 10) | (g5 << 5) | b5));
            }
        }
    }
    (out, 0)
}

/// Build the PICT v2 picture data (no 512-byte file header).
fn build_pict(w: u16, h: u16, rgba: &[u8], depth: Depth, pack: bool) -> (Vec<u8>, usize) {
    let (pixdata, colors) = if depth.is_indexed() {
        encode_indexed(w, h, rgba, depth, pack)
    } else {
        encode_direct(w, h, rgba, depth)
    };

    let mut body = Vec::new();
    rect(&mut body, 0, 0, h, w); // picFrame
    body.extend(be16(0x0011)); // VersionOp
    body.extend(be16(0x02FF)); // version 2
    body.extend(be16(0x0C00)); // HeaderOp
    body.extend(be16(0xFFFE)); // -2 = extended v2 header
    body.extend(be16(0)); // reserved
    body.extend(be32(0x0048_0000)); // hRes 72.0
    body.extend(be32(0x0048_0000)); // vRes 72.0
    rect(&mut body, 0, 0, h, w); // optimal source rect
    body.extend(be32(0)); // reserved
    body.extend(be16(0x0001)); // Clip
    body.extend(be16(10)); // rgnSize
    rect(&mut body, 0, 0, h, w); // rgnBBox
    body.extend(&pixdata);
    // PICT v2 opcodes must be word-aligned: "a byte of 0 is added after odd-size
    // data" (Imaging With QuickDraw, App. A). The PackBitsRect pixel data can be
    // odd; without this pad OpEndPic lands on an odd offset and DrawPicture
    // mis-parses it (blank/crash at 1/4/8-bit). Pad so OpEndPic is word-aligned.
    if body.len() % 2 == 1 {
        body.push(0);
    }
    body.extend(be16(0x00FF)); // OpEndPic

    let total = 2 + body.len();
    let mut data = Vec::with_capacity(total);
    data.extend(be16((total & 0xFFFF) as u16)); // picSize (low word)
    data.extend(body);
    (data, colors)
}

/// Decode an image file to RGBA8, optionally downscaling so the longest side is
/// at most `max` px (aspect preserved; never upscales). Returns (w, h, rgba).
fn load_rgba(input: &Path, max: Option<u32>) -> Result<(u16, u16, Vec<u8>)> {
    let mut dynimg = image::ImageReader::open(input)
        .with_context(|| format!("opening {}", input.display()))?
        .with_guessed_format()
        .with_context(|| format!("reading {}", input.display()))?
        .decode()
        .with_context(|| format!("decoding {}", input.display()))?;
    if let Some(m) = max {
        if dynimg.width() > m || dynimg.height() > m {
            dynimg = dynimg.resize(m, m, image::imageops::FilterType::Lanczos3);
        }
    }
    let img = dynimg.to_rgba8();
    let (w, h) = img.dimensions();
    anyhow::ensure!(w <= 0x7FFF && h <= 0x7FFF, "image too large ({w}x{h})");
    Ok((w as u16, h as u16, img.into_raw()))
}

/// Convert an image file to a PICT file (512-byte header + picture data).
/// `max` (if set) downscales so the longest side is at most `max` px (aspect
/// preserved; never upscales) — docs/06 "sized to the target resolution".
pub fn run(input: &Path, output: &Path, depth: Depth, pack: bool, max: Option<u32>) -> Result<Stats> {
    let (w, h, rgba) = load_rgba(input, max)?;
    let (data, colors) = build_pict(w, h, &rgba, depth, pack);

    let mut bytes = vec![0u8; 512]; // PICT file header
    bytes.extend(&data);
    std::fs::write(output, &bytes).with_context(|| format!("writing {}", output.display()))?;

    Ok(Stats {
        width: w,
        height: h,
        depth: depth.bits(),
        colors,
        bytes: bytes.len(),
    })
}

/// Build the raw 1-bit bitmap sidecar body (12-byte header + MSB-first rows).
/// Layout (all big-endian): magic 'A','B'; u16 version=1; u16 width; u16 height;
/// u16 rowBytes (even); u16 depth=1; then `rowBytes*height` bytes of pixels
/// (MSB-first, a set bit = black, matching the PICT 1-bit index where 1=black).
/// The launcher CopyBits this straight into its off-screen GWorld, bypassing the
/// PICT/DrawPicture opcode interpreter that faults Snow on some valid 1-bit art.
pub const RAW1_HEADER_LEN: usize = 12;

/// Wrap already-packed 1-bit pixel rows (MSB-first, `rowbytes` per row, even)
/// in the `.raw` sidecar header. `pixels` must be exactly `rowbytes*h` bytes.
/// Shared by box-art conversion (`build_raw1`) and app-icon harvest (`icons`).
pub fn raw1_wrap(w: u16, h: u16, rowbytes: u16, pixels: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(RAW1_HEADER_LEN + pixels.len());
    out.extend(b"AB"); // magic
    out.extend(be16(1)); // version
    out.extend(be16(w));
    out.extend(be16(h));
    out.extend(be16(rowbytes)); // even, high bit clear
    out.extend(be16(1)); // depth
    out.extend_from_slice(pixels);
    out
}

fn build_raw1(w: u16, h: u16, rgba: &[u8]) -> (Vec<u8>, usize) {
    let (wi, hi) = (w as usize, h as usize);
    let (idx, _palette) = quantize(rgba, wi, hi, Depth::One);
    // Even rowBytes (QuickDraw requirement; see encode_indexed).
    let rowbytes = ((wi + 7) / 8 + 1) & !1usize;

    let mut pixels = Vec::with_capacity(rowbytes * hi);
    for y in 0..hi {
        pixels.extend(pack_row(&idx[y * wi..(y + 1) * wi], wi, 1, rowbytes));
    }
    (raw1_wrap(w, h, rowbytes as u16, &pixels), rowbytes)
}

/// Convert an image file to a raw 1-bit bitmap sidecar (see `build_raw1`).
pub fn run_raw1(input: &Path, output: &Path, max: Option<u32>) -> Result<Stats> {
    let (w, h, rgba) = load_rgba(input, max)?;
    let (bytes, _rowbytes) = build_raw1(w, h, &rgba);
    std::fs::write(output, &bytes).with_context(|| format!("writing {}", output.display()))?;
    Ok(Stats {
        width: w,
        height: h,
        depth: 1,
        colors: 2,
        bytes: bytes.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packbits_roundtrips_repeat_and_literal() {
        // unpack helper (the classic PackBits decode)
        fn unpack(src: &[u8], out_len: usize) -> Vec<u8> {
            let mut out = Vec::new();
            let mut i = 0;
            while i < src.len() && out.len() < out_len {
                let n = src[i] as i8;
                i += 1;
                if n >= 0 {
                    let cnt = n as usize + 1;
                    out.extend_from_slice(&src[i..i + cnt]);
                    i += cnt;
                } else if n != -128 {
                    let cnt = (1 - n as i32) as usize;
                    out.extend(std::iter::repeat(src[i]).take(cnt));
                    i += 1;
                }
            }
            out
        }
        let data = b"AAAAAAA\x01\x02\x03BBBBCCDDEEFF".to_vec();
        let packed = packbits(&data);
        assert!(packed.len() < data.len()); // the AAAAAAA run compresses
        assert_eq!(unpack(&packed, data.len()), data);
    }

    #[test]
    fn pack_row_bit_layouts() {
        // 1-bit: indices 1,0,1,0,... MSB-first
        let idx = [1u8, 0, 1, 0, 0, 0, 0, 0];
        let row = pack_row(&idx, 8, 1, 1);
        assert_eq!(row, vec![0b1010_0000]);
        // 4-bit: two pixels per byte, hi nibble first
        let idx = [0x0A, 0x0B];
        let row = pack_row(&idx, 2, 4, 1);
        assert_eq!(row, vec![0xAB]);
        // 8-bit: straight copy
        let idx = [3u8, 7, 9];
        let row = pack_row(&idx, 3, 8, 3);
        assert_eq!(row, vec![3, 7, 9]);
    }

    #[test]
    fn pict_opcodes_are_word_aligned() {
        // Every depth must produce word-aligned opcodes: the picture data length
        // must be even so the trailing OpEndPic sits on a word boundary (else
        // DrawPicture mis-parses — the 1/4/8-bit blank/crash bug).
        for (w, h) in [(7u16, 5u16), (13, 9), (31, 17), (130, 97)] {
            let rgba = vec![0x80u8; w as usize * h as usize * 4];
            for depth in [Depth::One, Depth::Four, Depth::Eight, Depth::Sixteen, Depth::TwentyFour] {
                // build_pict returns the picture data (no 512-byte file header).
                let (data, _) = build_pict(w, h, &rgba, depth, true);
                assert_eq!(
                    data.len() % 2,
                    0,
                    "{w}x{h} {:?}: picture data must be even-length (word-aligned OpEndPic)",
                    depth
                );
                assert_eq!(&data[data.len() - 2..], &[0x00, 0xFF], "must end with OpEndPic");
            }
        }
    }

    #[test]
    fn pict_structure_starts_and_ends_correctly() {
        // 2x2 RGBA, 8-bit. Check the v2 header opcodes + EndPic.
        let rgba = vec![255u8; 2 * 2 * 4];
        let (data, _) = build_pict(2, 2, &rgba, Depth::Eight, true);
        // picSize(2) + picFrame(8) + VersionOp...
        assert_eq!(&data[2..10], &[0, 0, 0, 0, 0, 2, 0, 2]); // picFrame 0,0,2,2
        assert_eq!(&data[10..12], &[0x00, 0x11]); // VersionOp
        assert_eq!(&data[12..14], &[0x02, 0xFF]); // version 2
        assert_eq!(&data[14..16], &[0x0C, 0x00]); // HeaderOp
        assert_eq!(&data[data.len() - 2..], &[0x00, 0xFF]); // OpEndPic
    }

    #[test]
    fn sixteen_bit_uses_directbits() {
        let rgba = vec![255u8; 2 * 2 * 4];
        let (data, colors) = build_pict(2, 2, &rgba, Depth::Sixteen, true);
        assert_eq!(colors, 0); // direct: no colour table
        // find the DirectBitsRect opcode (0x009A) after the header/clip
        assert!(data.windows(2).any(|w| w == [0x00, 0x9A]));
    }

    #[test]
    fn twentyfour_bit_is_direct_32bit_pixels() {
        // 2x2 solid red. 24-bit -> DirectBitsRect, pixelSize 32, cmpCount/size 3/8,
        // and 4 bytes/pixel of 0,R,G,B unpacked data.
        let mut rgba = vec![0u8; 2 * 2 * 4];
        for p in rgba.chunks_mut(4) {
            p[0] = 0xFF; // R
            p[3] = 0xFF; // A
        }
        let (data, colors) = build_pict(2, 2, &rgba, Depth::TwentyFour, true);
        assert_eq!(colors, 0); // direct: no colour table
        let op = data.windows(2).position(|w| w == [0x00, 0x9A]).expect("DirectBitsRect");
        // rowBytes(2, with 0x8000) + bounds(8) -> then pmVersion... pixelType at +24,
        // pixelSize at +26 from the rowBytes word (op + 2 baseAddr(4) + rowBytes).
        // Simpler: assert the unpacked pixel bytes (0,FF,0,0) appear for a red pixel.
        assert!(data.windows(4).any(|w| w == [0x00, 0xFF, 0x00, 0x00]), "32-bit 0,R,G,B pixel");
        let _ = op;
    }

    #[test]
    fn median_cut_separates_distinct_colors() {
        // four very different colours -> four palette entries, each near an input
        let px = vec![
            [255, 0, 0],
            [0, 255, 0],
            [0, 0, 255],
            [10, 10, 10],
        ];
        let pal = median_cut(px.clone(), 4);
        assert_eq!(pal.len(), 4);
        for c in &px {
            let i = nearest(&pal, c[0], c[1], c[2]) as usize;
            let p = pal[i];
            let d = (p.0 as i32 - c[0] as i32).abs()
                + (p.1 as i32 - c[1] as i32).abs()
                + (p.2 as i32 - c[2] as i32).abs();
            assert!(d < 40, "colour {c:?} mapped to far palette entry {p:?}");
        }
    }

    #[test]
    fn median_cut_caps_at_n_and_handles_few_colors() {
        // one colour, ask for 16 -> 1 entry (can't split)
        let pal = median_cut(vec![[100, 100, 100]; 50], 16);
        assert_eq!(pal.len(), 1);
        assert_eq!(pal[0], (100, 100, 100));
    }

    #[test]
    fn rowbytes_padded_even_for_odd_widths() {
        // 24px 1-bit -> ceil(24/8)=3 (odd); QuickDraw needs even -> must pad to 4.
        let rgba = vec![0u8; 24 * 2 * 4];
        let (data, _) = build_pict(24, 2, &rgba, Depth::One, true);
        // search past the header/clip so we don't match picSize/coords by accident
        let p = 50 + data[50..].windows(2).position(|w| w == [0x00, 0x98]).unwrap();
        let rb = u16::from_be_bytes([data[p + 2], data[p + 3]]) & 0x7FFF;
        assert_eq!(rb % 2, 0, "rowBytes must be even, got {rb}");
        assert_eq!(rb, 4);
    }

    #[test]
    fn raw1_header_and_rows_are_well_formed() {
        // 9px wide -> ceil(9/8)=2 bytes, already even -> rowBytes 2.
        // Pure black image -> every bit set (index 1 = black).
        let rgba = vec![0u8; 9 * 3 * 4];
        let (bytes, rowbytes) = build_raw1(9, 3, &rgba);
        assert_eq!(rowbytes, 2);
        assert_eq!(&bytes[0..2], b"AB"); // magic
        assert_eq!(&bytes[2..4], &[0, 1]); // version 1
        assert_eq!(&bytes[4..6], &[0, 9]); // width
        assert_eq!(&bytes[6..8], &[0, 3]); // height
        assert_eq!(&bytes[8..10], &[0, 2]); // rowBytes (even)
        assert_eq!(&bytes[10..12], &[0, 1]); // depth
        assert_eq!(bytes.len(), RAW1_HEADER_LEN + 2 * 3);
        // first row: 9 black pixels MSB-first -> 0xFF, then 0x80 (bit 8), pad bit clear
        assert_eq!(bytes[12], 0xFF);
        assert_eq!(bytes[13], 0x80);
    }

    #[test]
    fn raw1_pads_odd_rowbytes_to_even() {
        // 24px -> ceil(24/8)=3 (odd) -> pad to 4.
        let rgba = vec![255u8; 24 * 2 * 4];
        let (bytes, rowbytes) = build_raw1(24, 2, &rgba);
        assert_eq!(rowbytes, 4);
        assert_eq!(&bytes[8..10], &[0, 4]);
        // pure white -> all index 0 -> all bits clear
        assert!(bytes[RAW1_HEADER_LEN..].iter().all(|&b| b == 0));
    }

    #[test]
    fn one_bit_white_image_is_all_zero_indices() {
        // pure white -> luma 255 > any Bayer threshold -> index 0 (white)
        let rgba = vec![255u8; 4 * 4 * 4];
        let (idx, pal) = quantize(&rgba, 4, 4, Depth::One);
        assert!(idx.iter().all(|&i| i == 0));
        assert_eq!(pal[0], (255, 255, 255));
    }
}
