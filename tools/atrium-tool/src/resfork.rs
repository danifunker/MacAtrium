//! Classic Mac **resource-fork writer** — build a resource fork from an arbitrary
//! set of `(type, id, data)` resources across multiple types.
//!
//! This generalizes the single-`snd ` builder that used to live in `snd.rs` so it
//! can also pack a title's depth-variant artwork (an `ABMP` 1-bit bitmap + one
//! `PICT` per colour depth) into one per-item `images/<id>.rsrc` file (docs/36).
//! The byte layout mirrors the proven `snd ` fork exactly: data starts at the
//! conventional 256-byte offset, resources carry no names and zero attributes, so
//! a single-resource call is **byte-identical** to the old `snd.rs::build_resfork`
//! (asserted in the tests + `snd.rs`).

/// A four-char resource type (OSType), e.g. `*b"PICT"`.
pub type OsType = [u8; 4];

/// One resource to place in the fork. `data` is the raw resource body (for a
/// `PICT` resource that's the picture data with **no** 512-byte file header).
pub struct Res<'a> {
    pub tag: OsType,
    pub id: i16,
    pub data: &'a [u8],
}

impl<'a> Res<'a> {
    pub fn new(tag: OsType, id: i16, data: &'a [u8]) -> Self {
        Res { tag, id, data }
    }
}

/// Conventional offset of the data section from the start of the fork.
const DATA_OFF: usize = 256;
/// Map layout: 24 reserved bytes (header copy + next-map handle + attrs), then the
/// typeList/nameList offset words, then the type list — so the type list begins 28
/// bytes into the map.
const TYPE_LIST_OFF: u16 = 28;
/// Bytes per reference-list entry: id(2) + nameOffset(2) + attrs(1) + dataOffset(3)
/// + reserved handle(4).
const REF_ENTRY_LEN: usize = 12;

/// Build a complete classic resource fork holding `resources`. Types are emitted
/// in first-seen order; within a type, resources keep their given order. Returns
/// the fork bytes (empty-input yields a valid fork with an empty type list).
pub fn build(resources: &[Res]) -> Vec<u8> {
    // Distinct types in first-seen order.
    let mut types: Vec<OsType> = Vec::new();
    for r in resources {
        if !types.contains(&r.tag) {
            types.push(r.tag);
        }
    }

    // Global resource order = grouped by type (the order both the data section and
    // the reference lists are written in, so their dataOffsets line up).
    let order: Vec<usize> = types
        .iter()
        .flat_map(|t| {
            resources
                .iter()
                .enumerate()
                .filter(move |(_, r)| r.tag == *t)
                .map(|(i, _)| i)
        })
        .collect();

    // Data section: [u32 length][body] per resource; remember each resource's
    // offset (of its length word) from the start of the data section.
    let mut data = Vec::new();
    let mut data_off_of = vec![0usize; resources.len()];
    for &i in &order {
        data_off_of[i] = data.len();
        data.extend_from_slice(&(resources[i].data.len() as u32).to_be_bytes());
        data.extend_from_slice(resources[i].data);
    }

    let n_types = types.len();
    let counts: Vec<usize> = types
        .iter()
        .map(|t| resources.iter().filter(|r| r.tag == *t).count())
        .collect();

    // Type list: [u16 nTypes-1] then one 8-byte entry per type. Reference lists
    // follow, so the first ref list sits `2 + nTypes*8` bytes into the type list.
    let mut tl = Vec::new();
    tl.extend_from_slice(&(n_types.wrapping_sub(1) as u16).to_be_bytes());
    let mut ref_off = 2 + n_types * 8;
    for (ti, t) in types.iter().enumerate() {
        tl.extend_from_slice(t);
        tl.extend_from_slice(&(counts[ti].wrapping_sub(1) as u16).to_be_bytes());
        tl.extend_from_slice(&(ref_off as u16).to_be_bytes());
        ref_off += counts[ti] * REF_ENTRY_LEN;
    }
    // Reference lists, grouped by type in the same order as `order`.
    for t in &types {
        for (i, r) in resources.iter().enumerate() {
            if r.tag != *t {
                continue;
            }
            tl.extend_from_slice(&r.id.to_be_bytes());
            tl.extend_from_slice(&0xFFFFu16.to_be_bytes()); // name offset: none
            tl.push(0); // attributes
            let d = data_off_of[i];
            tl.push((d >> 16) as u8); // 3-byte data offset
            tl.push((d >> 8) as u8);
            tl.push(d as u8);
            tl.extend_from_slice(&0u32.to_be_bytes()); // reserved handle
        }
    }

    let name_list_off = TYPE_LIST_OFF as usize + tl.len();
    let mut map = vec![0u8; 24];
    map.extend_from_slice(&TYPE_LIST_OFF.to_be_bytes());
    map.extend_from_slice(&(name_list_off as u16).to_be_bytes());
    map.extend_from_slice(&tl);

    let data_len = data.len();
    let map_off = DATA_OFF + data_len;
    let mut out = vec![0u8; DATA_OFF];
    out[0..4].copy_from_slice(&(DATA_OFF as u32).to_be_bytes());
    out[4..8].copy_from_slice(&(map_off as u32).to_be_bytes());
    out[8..12].copy_from_slice(&(data_len as u32).to_be_bytes());
    out[12..16].copy_from_slice(&(map.len() as u32).to_be_bytes());
    out.extend_from_slice(&data);
    out.extend_from_slice(&map);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn be16(b: &[u8], o: usize) -> u16 {
        u16::from_be_bytes([b[o], b[o + 1]])
    }
    fn be32(b: &[u8], o: usize) -> u32 {
        u32::from_be_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
    }

    /// Minimal resource-fork reader: find a resource's body by (type, id). Mirrors
    /// what the 68k Resource Manager does, so a round-trip proves the fork is valid.
    fn get_resource(fork: &[u8], want_tag: &[u8; 4], want_id: i16) -> Option<Vec<u8>> {
        let data_off = be32(fork, 0) as usize;
        let map_off = be32(fork, 4) as usize;
        let type_list_off = be16(fork, map_off + 24) as usize;
        let tl = map_off + type_list_off;
        let n_types = be16(fork, tl) as i32 + 1;
        for ti in 0..n_types as usize {
            let e = tl + 2 + ti * 8;
            let tag = &fork[e..e + 4];
            let count = be16(fork, e + 4) as i32 + 1;
            let ref_off = be16(fork, e + 6) as usize;
            for ri in 0..count as usize {
                let r = tl + ref_off + ri * 12;
                let id = i16::from_be_bytes([fork[r], fork[r + 1]]);
                if tag == want_tag && id == want_id {
                    let d = ((fork[r + 5] as usize) << 16)
                        | ((fork[r + 6] as usize) << 8)
                        | (fork[r + 7] as usize);
                    let at = data_off + d;
                    let len = be32(fork, at) as usize;
                    return Some(fork[at + 4..at + 4 + len].to_vec());
                }
            }
        }
        None
    }

    #[test]
    fn single_resource_header_and_roundtrip() {
        let body = [0x11u8, 0x22, 0x33, 0x44, 0x55];
        let fork = build(&[Res::new(*b"snd ", 128, &body)]);
        // header: data offset 256, map right after the data section.
        assert_eq!(be32(&fork, 0), 256);
        let data_len = be32(&fork, 8) as usize;
        assert_eq!(be32(&fork, 4) as usize, 256 + data_len); // map offset
        assert_eq!(data_len, 4 + body.len()); // [u32 len][body]
        // type list begins 28 into the map; type four bytes past the count word.
        let map_off = be32(&fork, 4) as usize;
        assert_eq!(be16(&fork, map_off + 24), 28);
        assert_eq!(&fork[map_off + 28 + 2..map_off + 28 + 6], b"snd ");
        assert_eq!(get_resource(&fork, b"snd ", 128).unwrap(), body);
    }

    #[test]
    fn multi_type_multi_id_roundtrip() {
        // An item's fork: a 1-bit ABMP + three colour PICTs, distinct ids.
        let abmp = vec![0xABu8; 7];
        let p8 = vec![0x08u8; 300]; // >255 so the 3-byte offsets matter
        let p16 = vec![0x16u8; 40];
        let p24 = vec![0x24u8; 5];
        let fork = build(&[
            Res::new(*b"ABMP", 129, &abmp),
            Res::new(*b"PICT", 136, &p8),
            Res::new(*b"PICT", 144, &p16),
            Res::new(*b"PICT", 152, &p24),
        ]);
        assert_eq!(get_resource(&fork, b"ABMP", 129).unwrap(), abmp);
        assert_eq!(get_resource(&fork, b"PICT", 136).unwrap(), p8);
        assert_eq!(get_resource(&fork, b"PICT", 144).unwrap(), p16);
        assert_eq!(get_resource(&fork, b"PICT", 152).unwrap(), p24);
        assert!(get_resource(&fork, b"PICT", 999).is_none());
        // two types (ABMP, PICT), PICT holds three resources.
        let map_off = be32(&fork, 4) as usize;
        assert_eq!(be16(&fork, map_off + 28), 1); // nTypes-1 = 1
    }
}
