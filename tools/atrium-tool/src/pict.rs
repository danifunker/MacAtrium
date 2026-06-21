//! `atrium pict` — convert PNG/JPEG artwork to classic-Mac **PICT** (docs/06).
//!
//! QuickDraw can `DrawPicture` a PICT directly; there's no PNG/JPEG decoder on
//! 68k. We emit **PICT v2** (a 512-byte file header + the picture data):
//!   • 1/4/8-bit → indexed `PackBitsRect` (0x0098) with an embedded colour table
//!     (PackBits-compressed rows), and
//!   • 16-bit → `DirectBitsRect` (0x009A), 1-5-5-5 "thousands" pixels.
//! Indexed depths use fixed palettes (B/W; the classic Mac 16-colour CLUT; a
//! 6×6×6 cube + greys for 8-bit); 1-bit uses an ordered (Bayer) dither.
//! Adaptive (median-cut) palettes are a future quality pass.

use anyhow::{Context, Result};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Depth {
    One,
    Four,
    Eight,
    Sixteen,
}

impl Depth {
    pub fn parse(s: &str) -> Result<Depth> {
        Ok(match s {
            "1" => Depth::One,
            "4" => Depth::Four,
            "8" => Depth::Eight,
            "16" => Depth::Sixteen,
            _ => anyhow::bail!("depth must be 1, 4, 8, or 16"),
        })
    }
    pub fn bits(self) -> u16 {
        match self {
            Depth::One => 1,
            Depth::Four => 4,
            Depth::Eight => 8,
            Depth::Sixteen => 16,
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
        Depth::Sixteen => unreachable!("16-bit is direct, not indexed"),
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
fn write_pixmap_fields(out: &mut Vec<u8>, rowbytes: u16, w: u16, h: u16, depth: Depth, pack_type: u16, indexed: bool) {
    out.extend(be16(rowbytes | 0x8000)); // high bit => PixMap (not old BitMap)
    rect(out, 0, 0, h, w); // bounds
    out.extend(be16(0)); // pmVersion
    out.extend(be16(pack_type)); // 0=default(PackBits), 1=unpacked
    out.extend(be32(0)); // packSize
    out.extend(be32(0x0048_0000)); // hRes 72.0
    out.extend(be32(0x0048_0000)); // vRes 72.0
    out.extend(be16(if indexed { 0 } else { 16 })); // pixelType: 0=indexed,16=RGBDirect
    out.extend(be16(depth.bits())); // pixelSize
    let (cmp_count, cmp_size) = if indexed { (1, depth.bits()) } else { (3, 5) };
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
    let rowbytes = (wi * bits as usize + 7) / 8;
    // PackBitsRect rows are packed only when rowBytes >= 8 AND packing is on;
    // packType 0 = (default) PackBits, 1 = unpacked raw rows.
    let do_pack = pack && rowbytes >= 8;

    let mut out = Vec::new();
    out.extend(be16(0x0098)); // PackBitsRect
    write_pixmap_fields(&mut out, rowbytes as u16, w, h, depth, if do_pack { 0 } else { 1 }, true);
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

fn encode_direct16(w: u16, h: u16, rgba: &[u8]) -> (Vec<u8>, usize) {
    let (wi, hi) = (w as usize, h as usize);
    let rowbytes = wi * 2;

    let mut out = Vec::new();
    out.extend(be16(0x009A)); // DirectBitsRect
    out.extend(be32(0x0000_00FF)); // pseudo baseAddr for DirectBits
    write_pixmap_fields(&mut out, rowbytes as u16, w, h, Depth::Sixteen, 1, false);
    rect(&mut out, 0, 0, h, w); // srcRect
    rect(&mut out, 0, 0, h, w); // dstRect
    out.extend(be16(0)); // mode = srcCopy

    // packType=1 (unpacked): raw rows, no count. 1-5-5-5 big-endian words.
    for y in 0..hi {
        for x in 0..wi {
            let o = (y * wi + x) * 4;
            let r5 = (rgba[o] >> 3) as u16;
            let g5 = (rgba[o + 1] >> 3) as u16;
            let b5 = (rgba[o + 2] >> 3) as u16;
            out.extend(be16((r5 << 10) | (g5 << 5) | b5));
        }
    }
    (out, 0)
}

/// Build the PICT v2 picture data (no 512-byte file header).
fn build_pict(w: u16, h: u16, rgba: &[u8], depth: Depth, pack: bool) -> (Vec<u8>, usize) {
    let (pixdata, colors) = if depth == Depth::Sixteen {
        encode_direct16(w, h, rgba)
    } else {
        encode_indexed(w, h, rgba, depth, pack)
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
    body.extend(be16(0x00FF)); // OpEndPic

    let total = 2 + body.len();
    let mut data = Vec::with_capacity(total);
    data.extend(be16((total & 0xFFFF) as u16)); // picSize (low word)
    data.extend(body);
    (data, colors)
}

/// Convert an image file to a PICT file (512-byte header + picture data).
/// `max` (if set) downscales so the longest side is at most `max` px (aspect
/// preserved; never upscales) — docs/06 "sized to the target resolution".
pub fn run(input: &Path, output: &Path, depth: Depth, pack: bool, max: Option<u32>) -> Result<Stats> {
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
    anyhow::ensure!(w <= 0x7FFF && h <= 0x7FFF, "image too large for PICT ({w}x{h})");

    let (data, colors) = build_pict(w as u16, h as u16, img.as_raw(), depth, pack);

    let mut bytes = vec![0u8; 512]; // PICT file header
    bytes.extend(&data);
    std::fs::write(output, &bytes).with_context(|| format!("writing {}", output.display()))?;

    Ok(Stats {
        width: w as u16,
        height: h as u16,
        depth: depth.bits(),
        colors,
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
    fn one_bit_white_image_is_all_zero_indices() {
        // pure white -> luma 255 > any Bayer threshold -> index 0 (white)
        let rgba = vec![255u8; 4 * 4 * 4];
        let (idx, pal) = quantize(&rgba, 4, 4, Depth::One);
        assert!(idx.iter().all(|&i| i == 0));
        assert_eq!(pal[0], (255, 255, 255));
    }
}
