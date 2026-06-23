//! App-icon harvest (docs/14 follow-up): decode a BinHex 4.0 (`.hqx`) app — as
//! produced by `rb-cli get-binhex`, both forks — parse its resource fork, and
//! extract the Finder icon (`ICN#`, the classic 32×32 1-bit icon) as a raw
//! 1-bit bitmap (`.raw`). The launcher CopyBits this as **fallback art** for
//! titles that have no box art, reusing the same crash-free blit path as box
//! art (no PICT/DrawPicture involved).
//!
//! The icon is resolved the proper Finder way — `BNDL` → the `FREF` whose file
//! type is `APPL` → that local id's `ICN#` — falling back to the lowest-id
//! `ICN#` when the bundle is missing or inconsistent.

use crate::pict;
use anyhow::{anyhow, Result};
use std::path::Path;

// ---- big-endian readers (all bounds-checked, None on overrun) --------------
fn be_u16(b: &[u8], o: usize) -> Option<u16> {
    b.get(o..o + 2).map(|s| u16::from_be_bytes([s[0], s[1]]))
}
fn be_i16(b: &[u8], o: usize) -> Option<i16> {
    be_u16(b, o).map(|v| v as i16)
}
fn be_u24(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 3).map(|s| (s[0] as u32) << 16 | (s[1] as u32) << 8 | s[2] as u32)
}
fn be_u32(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}

// ---- BinHex 4.0 ------------------------------------------------------------
// The 64-character BinHex 4.0 alphabet (note the gaps: no 7, W, g, n, o).
const BINHEX_TABLE: &[u8] =
    b"!\"#$%&'()*+,-012345689@ABCDEFGHIJKLMNPQRSTUVXYZ[`abcdefhijklmpqr";

/// Decode a `.hqx` and return just the **resource fork** bytes.
pub fn binhex_resource_fork(hqx: &[u8]) -> Result<Vec<u8>> {
    let text = std::str::from_utf8(hqx).map_err(|_| anyhow!("hqx not ASCII"))?;
    // Payload is everything between the first ':' and the next ':'.
    let start = text.find(':').ok_or_else(|| anyhow!("no BinHex start ':'"))?;
    let rest = &text[start + 1..];
    let end = rest.find(':').ok_or_else(|| anyhow!("no BinHex end ':'"))?;
    let payload = &rest[..end];

    let mut rev = [0xFFu8; 256];
    for (i, &c) in BINHEX_TABLE.iter().enumerate() {
        rev[c as usize] = i as u8;
    }

    // 6-bit symbols -> bytes.
    let mut bytes = Vec::with_capacity(payload.len() * 3 / 4);
    let (mut acc, mut nbits) = (0u32, 0u32);
    for &b in payload.as_bytes() {
        if b.is_ascii_whitespace() {
            continue;
        }
        let v = rev[b as usize];
        if v == 0xFF {
            continue; // stray char; the alphabet excludes whitespace anyway
        }
        acc = (acc << 6) | v as u32;
        nbits += 6;
        if nbits >= 8 {
            nbits -= 8;
            bytes.push((acc >> nbits) as u8);
        }
    }

    // RLE: 0x90 is the run marker. "X 0x90 n" => X repeated n times (n==0 =>
    // a literal 0x90).
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        i += 1;
        if b != 0x90 {
            out.push(b);
        } else {
            let cnt = *bytes.get(i).unwrap_or(&0);
            i += 1;
            if cnt == 0 {
                out.push(0x90);
            } else if let Some(&last) = out.last() {
                for _ in 0..cnt - 1 {
                    out.push(last);
                }
            }
        }
    }

    // Header: u8 nameLen, name, u8 version, type[4], creator[4], flags[2],
    // u32 dataLen, u32 rsrcLen, u16 crc, <data fork>, u16 crc, <rsrc fork>, ...
    let name_len = *out.first().ok_or_else(|| anyhow!("empty BinHex stream"))? as usize;
    let p = 1 + name_len + 1;
    let data_len = be_u32(&out, p + 10).ok_or_else(|| anyhow!("truncated BinHex header"))? as usize;
    let rsrc_len = be_u32(&out, p + 14).ok_or_else(|| anyhow!("truncated BinHex header"))? as usize;
    let rsrc_start = p + 20 + data_len + 2; // skip data fork + its 2-byte CRC
    out.get(rsrc_start..rsrc_start + rsrc_len)
        .map(<[u8]>::to_vec)
        .ok_or_else(|| anyhow!("BinHex resource fork truncated"))
}

// ---- resource fork ---------------------------------------------------------
/// All resources of `want` type as (id, data) — bounds-checked; a malformed map
/// just yields fewer entries.
fn resources_of_type<'a>(rsrc: &'a [u8], want: &[u8; 4]) -> Vec<(i16, &'a [u8])> {
    let mut out = Vec::new();
    let (Some(data_off), Some(map_off)) = (be_u32(rsrc, 0), be_u32(rsrc, 4)) else {
        return out;
    };
    let (data_off, map_off) = (data_off as usize, map_off as usize);
    // Type list starts at map_off + offset stored at map_off+24.
    let Some(type_list_rel) = be_u16(rsrc, map_off + 24) else { return out };
    let type_list = map_off + type_list_rel as usize;
    let Some(ntypes_m1) = be_u16(rsrc, type_list) else { return out };
    for t in 0..(ntypes_m1 as usize + 1) {
        let e = type_list + 2 + t * 8;
        if rsrc.get(e..e + 4) != Some(&want[..]) {
            continue;
        }
        let (Some(cnt_m1), Some(ref_rel)) = (be_u16(rsrc, e + 4), be_u16(rsrc, e + 6)) else {
            continue;
        };
        let ref_list = type_list + ref_rel as usize;
        for r in 0..(cnt_m1 as usize + 1) {
            let re = ref_list + r * 12;
            let (Some(id), Some(d_rel)) = (be_i16(rsrc, re), be_u24(rsrc, re + 5)) else {
                continue;
            };
            let abs = data_off + d_rel as usize;
            let Some(len) = be_u32(rsrc, abs) else { continue };
            if let Some(data) = rsrc.get(abs + 4..abs + 4 + len as usize) {
                out.push((id, data));
            }
        }
    }
    out
}

/// The `ICN#` resource id that the app bundle designates as the application
/// icon: `BNDL` → `FREF` whose file type is `APPL` → that icon's local id.
fn app_icn_id_via_bndl(rsrc: &[u8]) -> Option<i16> {
    let bndls = resources_of_type(rsrc, b"BNDL");
    let (_, bndl) = bndls.first()?;
    // BNDL: u32 ownerType, u16 ownerID, u16 nTypes-1, then per type:
    // type[4], u16 count-1, count×(u16 localID, u16 resID).
    let n_types = be_u16(bndl, 6)? as usize + 1;
    let mut p = 8;
    let mut fref_map: Vec<(u16, i16)> = Vec::new();
    let mut icn_map: Vec<(u16, i16)> = Vec::new();
    for _ in 0..n_types {
        let ty = bndl.get(p..p + 4)?.to_vec();
        p += 4;
        let cnt = be_u16(bndl, p)? as usize + 1;
        p += 2;
        let mut map = Vec::with_capacity(cnt);
        for _ in 0..cnt {
            map.push((be_u16(bndl, p)?, be_i16(bndl, p + 2)?));
            p += 4;
        }
        if ty == b"FREF" {
            fref_map = map;
        } else if ty == b"ICN#" {
            icn_map = map;
        }
    }
    let frefs = resources_of_type(rsrc, b"FREF");
    for (local, res) in &fref_map {
        let Some((_, fdata)) = frefs.iter().find(|(i, _)| i == res) else { continue };
        if fdata.get(0..4) != Some(b"APPL") {
            continue;
        }
        // FREF: fileType[4], u16 iconLocalID, name. Prefer that id, else the
        // BNDL pairing's local id.
        let icon_local = be_u16(fdata, 4).unwrap_or(*local);
        if let Some((_, r)) = icn_map.iter().find(|(l, _)| *l == icon_local) {
            return Some(*r);
        }
        if let Some((_, r)) = icn_map.iter().find(|(l, _)| l == local) {
            return Some(*r);
        }
    }
    None
}

/// Extract the app icon (32×32 1-bit plane of its `ICN#`) from a resource fork.
fn app_icn_plane(rsrc: &[u8]) -> Option<[u8; 128]> {
    let icns = resources_of_type(rsrc, b"ICN#");
    if icns.is_empty() {
        return None;
    }
    // Preferred: the bundle's designated app icon.
    let chosen = app_icn_id_via_bndl(rsrc)
        .and_then(|id| icns.iter().find(|(i, _)| *i == id).map(|(_, d)| *d))
        // Fallback: the lowest-id ICN#.
        .or_else(|| icns.iter().min_by_key(|(i, _)| *i).map(|(_, d)| *d))?;
    // ICN# = 128 bytes icon plane + 128 bytes mask; we want the icon plane.
    let plane = chosen.get(0..128)?;
    let mut out = [0u8; 128];
    out.copy_from_slice(plane);
    Some(out)
}

/// Decode a `.hqx` app and return its icon as `.raw` 1-bit bitmap bytes, or
/// `None` if the app has no usable icon. A 32×32 `ICN#` is already MSB-first
/// 1-bit with rowBytes 4, so it wraps straight into the sidecar header.
pub fn app_icon_raw1(hqx: &[u8]) -> Result<Option<Vec<u8>>> {
    let rsrc = binhex_resource_fork(hqx)?;
    Ok(app_icn_plane(&rsrc).map(|icn| pict::raw1_wrap(32, 32, 4, &icn)))
}

// ---- colour icon (icl8) ----------------------------------------------------
/// The standard Macintosh 8-bit system palette (the default `clut` 8): a
/// 6×6×6 colour cube (component levels FF/CC/99/66/33/00, index = r*36+g*6+b)
/// followed by red, green, blue and gray ramps over the in-between repeated-
/// digit values. `icl8` pixels index into this. Returns 256 [r,g,b] entries.
fn mac_palette() -> [[u8; 3]; 256] {
    const CUBE: [u8; 6] = [0xFF, 0xCC, 0x99, 0x66, 0x33, 0x00];
    const RAMP: [u8; 10] = [0xEE, 0xDD, 0xBB, 0xAA, 0x88, 0x77, 0x55, 0x44, 0x22, 0x11];
    let mut pal = [[0u8; 3]; 256];
    let mut i = 0;
    for &r in &CUBE {
        for &g in &CUBE {
            for &b in &CUBE {
                pal[i] = [r, g, b];
                i += 1;
            }
        }
    }
    // i == 216: pure-colour and gray ramps fill 216..256.
    for &v in &RAMP { pal[i] = [v, 0, 0]; i += 1; }
    for &v in &RAMP { pal[i] = [0, v, 0]; i += 1; }
    for &v in &RAMP { pal[i] = [0, 0, v]; i += 1; }
    for &v in &RAMP { pal[i] = [v, v, v]; i += 1; }
    pal
}

/// The app's 32×32 8-bit colour icon (`icl8`, 1024 index bytes): the bundle's
/// designated icon (shares its `ICN#` id), else the lowest-id `icl8`.
fn app_icl8_data(rsrc: &[u8]) -> Option<Vec<u8>> {
    let icl8s = resources_of_type(rsrc, b"icl8");
    if icl8s.is_empty() {
        return None;
    }
    let chosen = app_icn_id_via_bndl(rsrc)
        .and_then(|id| icl8s.iter().find(|(i, _)| *i == id).map(|(_, d)| *d))
        .or_else(|| icl8s.iter().min_by_key(|(i, _)| *i).map(|(_, d)| *d))?;
    (chosen.len() >= 1024).then(|| chosen[..1024].to_vec())
}

/// Decode a `.hqx` app's colour icon (`icl8`) and write it as a 32×32 PNG at
/// `out`, resolving palette indices through the standard Mac 256-colour table.
/// Returns Ok(true) when written, Ok(false) when the app has no usable `icl8`.
pub fn app_icl8_png(hqx: &[u8], out: &Path) -> Result<bool> {
    let rsrc = binhex_resource_fork(hqx)?;
    let icl8 = match app_icl8_data(&rsrc) {
        Some(d) => d,
        None => return Ok(false),
    };
    let pal = mac_palette();
    let mut rgba = Vec::with_capacity(32 * 32 * 4);
    for &idx in &icl8 {
        let [r, g, b] = pal[idx as usize];
        rgba.extend_from_slice(&[r, g, b, 0xFF]);
    }
    let img: image::RgbaImage = image::ImageBuffer::from_raw(32, 32, rgba)
        .ok_or_else(|| anyhow!("icl8 RGBA size mismatch"))?;
    img.save(out).map_err(|e| anyhow!("writing {}: {e}", out.display()))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal resource fork with the given resources for testing.
    fn make_rsrc(resources: &[(&[u8; 4], i16, Vec<u8>)]) -> Vec<u8> {
        // data section: each resource = u32 len + data; remember offsets.
        let mut data = Vec::new();
        let mut offsets = Vec::new();
        for (_, _, d) in resources {
            offsets.push(data.len() as u32);
            data.extend((d.len() as u32).to_be_bytes());
            data.extend(d.iter());
        }
        // group by type, preserving order of first appearance.
        let mut types: Vec<[u8; 4]> = Vec::new();
        for (t, _, _) in resources {
            if !types.contains(*t) {
                types.push(**t);
            }
        }
        // type list: u16 nTypes-1, then per type (type[4], u16 cnt-1, u16 refRel)
        // ref lists follow the type list.
        let type_entry_bytes = 2 + types.len() * 8;
        let mut type_list = Vec::new();
        type_list.extend(((types.len() - 1) as u16).to_be_bytes());
        let mut ref_lists = Vec::new();
        for t in &types {
            let members: Vec<usize> = resources
                .iter()
                .enumerate()
                .filter(|(_, (rt, _, _))| *rt == t)
                .map(|(i, _)| i)
                .collect();
            let ref_rel = (type_entry_bytes + ref_lists.len()) as u16;
            type_list.extend(t);
            type_list.extend(((members.len() - 1) as u16).to_be_bytes());
            type_list.extend(ref_rel.to_be_bytes());
            for i in members {
                let (_, id, _) = &resources[i];
                ref_lists.extend(id.to_be_bytes());
                ref_lists.extend((0xFFFFu16).to_be_bytes()); // no name
                ref_lists.push(0); // attrs
                ref_lists.extend(&offsets[i].to_be_bytes()[1..4]); // u24 data offset
                ref_lists.extend((0u32).to_be_bytes()); // reserved
            }
        }
        let map_body = {
            let mut m = Vec::new();
            m.extend(&type_list);
            m.extend(&ref_lists);
            m
        };
        // map: 16 reserved + 4 + 2 + 2 + u16 typeListOff + u16 nameListOff + body
        let map_header = 16 + 4 + 2 + 2 + 2 + 2;
        let mut map = vec![0u8; map_header];
        let type_list_off = map_header as u16;
        map[24..26].copy_from_slice(&type_list_off.to_be_bytes());
        map[26..28].copy_from_slice(&((map_header + map_body.len()) as u16).to_be_bytes());
        map.extend(&map_body);

        let data_off = 16u32;
        let map_off = data_off + data.len() as u32;
        let mut out = Vec::new();
        out.extend(data_off.to_be_bytes());
        out.extend(map_off.to_be_bytes());
        out.extend((data.len() as u32).to_be_bytes());
        out.extend((map.len() as u32).to_be_bytes());
        out.extend(&data);
        out.extend(&map);
        out
    }

    #[test]
    fn resfork_reads_resources_by_type() {
        let icn = vec![0xABu8; 256]; // 128 icon + 128 mask
        let rsrc = make_rsrc(&[(b"ICN#", 128, icn.clone())]);
        let got = resources_of_type(&rsrc, b"ICN#");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0, 128);
        assert_eq!(got[0].1, &icn[..]);
        assert!(resources_of_type(&rsrc, b"BNDL").is_empty());
    }

    #[test]
    fn bndl_resolves_appl_icon() {
        // FREF 128 -> APPL, iconLocalID 0; BNDL maps FREF local0->128, ICN# local0->256.
        let mut fref = b"APPL".to_vec();
        fref.extend((0u16).to_be_bytes()); // iconLocalID
        fref.push(0); // empty pascal name
        let mut bndl = Vec::new();
        bndl.extend(b"TEST"); // owner type
        bndl.extend((0u16).to_be_bytes()); // owner id
        bndl.extend((1u16).to_be_bytes()); // nTypes-1 = 1 (two types)
        bndl.extend(b"ICN#");
        bndl.extend((0u16).to_be_bytes()); // count-1
        bndl.extend((0u16).to_be_bytes()); // local 0
        bndl.extend((256i16).to_be_bytes()); // -> ICN# 256
        bndl.extend(b"FREF");
        bndl.extend((0u16).to_be_bytes());
        bndl.extend((0u16).to_be_bytes()); // local 0
        bndl.extend((128i16).to_be_bytes()); // -> FREF 128

        let app_icn = vec![0x11u8; 256];
        let other_icn = vec![0x22u8; 256];
        let rsrc = make_rsrc(&[
            (b"ICN#", 200, other_icn), // lower id, but NOT the app icon
            (b"ICN#", 256, app_icn.clone()),
            (b"FREF", 128, fref),
            (b"BNDL", 128, bndl),
        ]);
        let plane = app_icn_plane(&rsrc).expect("icon");
        assert_eq!(&plane[..], &app_icn[0..128]); // chose 256 via BNDL, not 200
    }

    #[test]
    fn mac_palette_endpoints_and_ramps() {
        let pal = mac_palette();
        assert_eq!(pal[0], [0xFF, 0xFF, 0xFF]); // white at the cube's first entry
        assert_eq!(pal[215], [0x00, 0x00, 0x00]); // black at the cube's last entry
        assert_eq!(pal[216], [0xEE, 0x00, 0x00]); // first red-ramp entry
        assert_eq!(pal[246], [0xEE, 0xEE, 0xEE]); // first gray-ramp entry
        assert_eq!(pal[255], [0x11, 0x11, 0x11]); // last entry
        // cube ordering: index = r*36 + g*6 + b over levels FF/CC/99/66/33/00.
        assert_eq!(pal[1], [0xFF, 0xFF, 0xCC]); // blue steps fastest
        assert_eq!(pal[36], [0xCC, 0xFF, 0xFF]); // red step
    }

    #[test]
    fn falls_back_to_lowest_icn_without_bundle() {
        let lo = vec![0x33u8; 256];
        let hi = vec![0x44u8; 256];
        let rsrc = make_rsrc(&[(b"ICN#", 130, hi), (b"ICN#", 129, lo.clone())]);
        let plane = app_icn_plane(&rsrc).expect("icon");
        assert_eq!(&plane[..], &lo[0..128]);
    }
}
