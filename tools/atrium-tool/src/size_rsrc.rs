//! Patch the `'SIZE'` (-1) resource in the launcher MacBinary so each build can
//! give the shell a memory partition sized to its target machine.
//!
//! The launcher ships ONE `MacAtrium.bin`, whose `'SIZE'` (-1) resource (set in
//! `src/macatrium.r`) requests a fixed 2 MB preferred / 1 MB minimum partition.
//! That's fine on a colour Mac II, but starves a 4 MB Mac Plus/SE (System 6 +
//! the launched game need the rest). Rather than build a launcher per target, we
//! patch the 8 partition-size bytes of that one resource at image-build time —
//! so a compact B&W appliance can ask for a few hundred KB and a colour build
//! keeps its headroom.
//!
//! A `'SIZE'` resource body is `flags (u16) | preferredSize (u32) | minimumSize
//! (u32)` (Inside Macintosh: Processes). We overwrite only the two u32s, leaving
//! the flags (suspend/resume, 32-bit-clean, high-level-event aware, …) intact.
//!
//! The bytes are a MacBinary file: a 128-byte header, the (here empty) data fork
//! padded to 128 bytes, then the resource fork. Within the resource fork we walk
//! the standard map (Inside Macintosh: More Macintosh Toolbox, "Resource File
//! Format") to find type `SIZE`, id -1, and its data offset.

use anyhow::{bail, Context, Result};

fn be_u32(b: &[u8], off: usize) -> u32 {
    u32::from_be_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}
fn be_u16(b: &[u8], off: usize) -> u16 {
    u16::from_be_bytes([b[off], b[off + 1]])
}

/// Byte offset of the start of the resource fork inside a MacBinary buffer, plus
/// the resource fork length. Errors if `bytes` isn't a plausible MacBinary with a
/// resource fork.
fn macbinary_rsrc_fork(bytes: &[u8]) -> Result<(usize, usize)> {
    if bytes.len() < 128 {
        bail!("not MacBinary: shorter than a 128-byte header");
    }
    let data_len = be_u32(bytes, 83) as usize;
    let rsrc_len = be_u32(bytes, 87) as usize;
    if rsrc_len == 0 {
        bail!("MacBinary has no resource fork");
    }
    // Each fork is padded to a 128-byte boundary; the resource fork follows the
    // (padded) data fork.
    let pad = |n: usize| n.div_ceil(128) * 128;
    let start = 128 + pad(data_len);
    if start + rsrc_len > bytes.len() {
        bail!(
            "resource fork [{}..{}] runs past the {}-byte file",
            start,
            start + rsrc_len,
            bytes.len()
        );
    }
    Ok((start, rsrc_len))
}

/// Locate the `'SIZE'` id -1 resource body inside a resource fork (a slice
/// starting at the resource header). Returns the byte range of the resource's
/// 10-byte body (after its 4-byte length prefix), relative to the fork slice.
fn find_size_minus1_body(rf: &[u8]) -> Result<(usize, usize)> {
    if rf.len() < 16 {
        bail!("resource fork too small for its 16-byte header");
    }
    let data_off = be_u32(rf, 0) as usize; // resource data section
    let map_off = be_u32(rf, 4) as usize; // resource map
    if map_off + 28 > rf.len() {
        bail!("resource map offset {map_off} out of range");
    }
    let map = &rf[map_off..];
    let type_list_off = be_u16(map, 24) as usize; // from map start
    if type_list_off + 2 > map.len() {
        bail!("type list offset out of range");
    }
    let ntypes = be_u16(map, type_list_off) as i32 + 1; // count is stored minus 1
    let mut p = type_list_off + 2;
    for _ in 0..ntypes {
        if p + 8 > map.len() {
            bail!("truncated type list");
        }
        let typ = &map[p..p + 4];
        let count = be_u16(map, p + 4) as i32 + 1; // resources of this type, minus 1
        let ref_off = be_u16(map, p + 6) as usize; // ref list, from type-list start
        p += 8;
        if typ != b"SIZE" {
            continue;
        }
        let mut rp = type_list_off + ref_off;
        for _ in 0..count {
            if rp + 12 > map.len() {
                bail!("truncated SIZE reference list");
            }
            let id = i16::from_be_bytes([map[rp], map[rp + 1]]);
            // 1-byte attrs + 3-byte data offset (from the data section start).
            let res_data_off =
                u32::from_be_bytes([0, map[rp + 5], map[rp + 6], map[rp + 7]]) as usize;
            rp += 12;
            if id != -1 {
                continue;
            }
            let abs = data_off + res_data_off; // -> 4-byte length, then the body
            if abs + 4 > rf.len() {
                bail!("SIZE (-1) data offset out of range");
            }
            let body_len = be_u32(rf, abs) as usize;
            let body = abs + 4;
            if body_len < 10 || body + 10 > rf.len() {
                bail!("SIZE (-1) body is {body_len} bytes; expected >= 10");
            }
            return Ok((body, body + 10));
        }
        bail!("SIZE type present but no id -1 resource");
    }
    bail!("no SIZE resource type in the launcher");
}

/// Overwrite the preferred/minimum partition sizes (in **bytes**) of the
/// `'SIZE'` (-1) resource in a launcher MacBinary, in place. Returns the previous
/// `(preferred, minimum)` so the build can log the change. The flags word is left
/// untouched, so suspend/resume + 32-bit + high-level-event behaviour is kept.
pub fn patch_app_mem(bytes: &mut [u8], preferred: u32, minimum: u32) -> Result<(u32, u32)> {
    let (fork_start, fork_len) =
        macbinary_rsrc_fork(bytes).context("finding resource fork in launcher")?;
    let (body_lo, _body_hi) = {
        let rf = &bytes[fork_start..fork_start + fork_len];
        find_size_minus1_body(rf).context("finding SIZE (-1) resource")?
    };
    // The body is `flags(2) | preferred(4) | minimum(4)`; patch the two u32s.
    let pref_off = fork_start + body_lo + 2;
    let min_off = fork_start + body_lo + 6;
    let old_pref = be_u32(bytes, pref_off);
    let old_min = be_u32(bytes, min_off);
    bytes[pref_off..pref_off + 4].copy_from_slice(&preferred.to_be_bytes());
    bytes[min_off..min_off + 4].copy_from_slice(&minimum.to_be_bytes());
    Ok((old_pref, old_min))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal MacBinary (empty data fork) whose resource fork holds a
    /// single SIZE (-1) resource, so the patcher can be exercised without the
    /// real launcher. Layout mirrors a Retro68 fork closely enough to parse.
    fn synthetic_macbinary(pref: u32, min: u32) -> Vec<u8> {
        // ---- resource fork ----
        // body: flags(2) + pref(4) + min(4)
        let mut body = vec![0x48u8, 0xc0];
        body.extend_from_slice(&pref.to_be_bytes());
        body.extend_from_slice(&min.to_be_bytes());
        // data section: 4-byte length + body
        let mut data = (body.len() as u32).to_be_bytes().to_vec();
        data.extend_from_slice(&body);

        // map: 16 reserved + 4 + 2 + 2(attrs) + 2(type-list off) + 2(name-list off)
        //      then type list, then ref list.
        let type_list_off: u16 = 28; // right after the 28-byte map header fields
        // type list: count(2) + one 8-byte type entry
        // ref list begins immediately after the type list (offset from type-list start)
        let ref_off: u16 = 2 + 8;
        let mut map = vec![0u8; 24];
        map.extend_from_slice(&type_list_off.to_be_bytes()); // [24..26]
        let name_list_off: u16 = type_list_off + ref_off + 12; // past the ref list
        map.extend_from_slice(&name_list_off.to_be_bytes()); // [26..28]
        // type list
        map.extend_from_slice(&0i16.to_be_bytes()); // ntypes-1 = 0
        map.extend_from_slice(b"SIZE");
        map.extend_from_slice(&0i16.to_be_bytes()); // count-1 = 0
        map.extend_from_slice(&ref_off.to_be_bytes());
        // ref list: id(2) nameOff(2) attrs(1)+dataOff(3) handle(4)
        map.extend_from_slice(&(-1i16).to_be_bytes());
        map.extend_from_slice(&0xffffu16.to_be_bytes()); // no name
        let data_off_in_section: u32 = 0;
        map.push(0); // attrs
        map.extend_from_slice(&data_off_in_section.to_be_bytes()[1..4]); // 3-byte offset
        map.extend_from_slice(&[0u8; 4]); // handle placeholder

        let data_off: u32 = 16; // data section right after the 16-byte fork header
        let map_off: u32 = data_off + data.len() as u32;
        let mut rf = Vec::new();
        rf.extend_from_slice(&data_off.to_be_bytes());
        rf.extend_from_slice(&map_off.to_be_bytes());
        rf.extend_from_slice(&(data.len() as u32).to_be_bytes());
        rf.extend_from_slice(&(map.len() as u32).to_be_bytes());
        rf.extend_from_slice(&data);
        rf.extend_from_slice(&map);

        // ---- MacBinary ----
        let mut mb = vec![0u8; 128];
        mb[1] = 9;
        mb[2..2 + 9].copy_from_slice(b"MacAtrium");
        mb[83..87].copy_from_slice(&0u32.to_be_bytes()); // data fork len = 0
        mb[87..91].copy_from_slice(&(rf.len() as u32).to_be_bytes());
        mb.extend_from_slice(&rf);
        mb
    }

    #[test]
    fn patches_pref_and_min_keeping_flags() {
        let mut mb = synthetic_macbinary(2048 * 1024, 1024 * 1024);
        let (old_p, old_m) = patch_app_mem(&mut mb, 512 * 1024, 384 * 1024).unwrap();
        assert_eq!(old_p, 2048 * 1024);
        assert_eq!(old_m, 1024 * 1024);
        // re-read: the new values stuck and the flags word survived
        let (start, len) = macbinary_rsrc_fork(&mb).unwrap();
        let (lo, _) = find_size_minus1_body(&mb[start..start + len]).unwrap();
        assert_eq!(be_u16(&mb[start..start + len], lo), 0x48c0, "flags preserved");
        assert_eq!(be_u32(&mb, start + lo + 2), 512 * 1024);
        assert_eq!(be_u32(&mb, start + lo + 6), 384 * 1024);
    }

    #[test]
    fn rejects_non_macbinary() {
        let mut junk = vec![0u8; 64];
        assert!(patch_app_mem(&mut junk, 1, 1).is_err());
    }
}
