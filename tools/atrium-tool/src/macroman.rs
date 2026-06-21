//! UTF-8 → Mac OS Roman transcoding.
//!
//! Classic-Mac text files are MacRoman, not UTF-8. The on-device launcher draws
//! bytes straight through Chicago, so the catalog we emit must be MacRoman. This
//! module maps the high half (0x80–0xFF) of Mac OS Roman back from Unicode;
//! ASCII passes through unchanged, and anything outside the repertoire becomes
//! '?' (counted by the caller so the dataset can be fixed).

/// Mac OS Roman code points 0x80..=0xFF, in order. Index 0 == byte 0x80.
const HIGH: [char; 128] = [
    'Ä', 'Å', 'Ç', 'É', 'Ñ', 'Ö', 'Ü', 'á', 'à', 'â', 'ä', 'ã', 'å', 'ç', 'é', 'è', // 0x80
    'ê', 'ë', 'í', 'ì', 'î', 'ï', 'ñ', 'ó', 'ò', 'ô', 'ö', 'õ', 'ú', 'ù', 'û', 'ü', // 0x90
    '†', '°', '¢', '£', '§', '•', '¶', 'ß', '®', '©', '™', '´', '¨', '≠', 'Æ', 'Ø', // 0xA0
    '∞', '±', '≤', '≥', '¥', 'µ', '∂', '∑', '∏', 'π', '∫', 'ª', 'º', 'Ω', 'æ', 'ø', // 0xB0
    '¿', '¡', '¬', '√', 'ƒ', '≈', '∆', '«', '»', '…', '\u{00A0}', 'À', 'Ã', 'Õ', 'Œ', 'œ', // 0xC0
    '–', '—', '“', '”', '‘', '’', '÷', '◊', 'ÿ', 'Ÿ', '⁄', '€', '‹', '›', 'ﬁ', 'ﬂ', // 0xD0
    '‡', '·', '‚', '„', '‰', 'Â', 'Ê', 'Á', 'Ë', 'È', 'Í', 'Î', 'Ï', 'Ì', 'Ó', 'Ô', // 0xE0
    '\u{F8FF}', 'Ò', 'Ú', 'Û', 'Ù', 'ı', 'ˆ', '˜', '¯', '˘', '˙', '˚', '¸', '˝', '˛', 'ˇ', // 0xF0
];

/// Encode one `char` to its MacRoman byte, or `None` if not representable.
fn encode_char(c: char) -> Option<u8> {
    if (c as u32) < 0x80 {
        return Some(c as u8);
    }
    HIGH.iter().position(|&h| h == c).map(|i| 0x80 + i as u8)
}

/// Transcode a UTF-8 string to MacRoman bytes. Returns the bytes plus the count
/// of characters that had no MacRoman equivalent (emitted as '?').
pub fn encode(s: &str) -> (Vec<u8>, usize) {
    let mut out = Vec::with_capacity(s.len());
    let mut lossy = 0usize;
    for c in s.chars() {
        match encode_char(c) {
            Some(b) => out.push(b),
            None => {
                out.push(b'?');
                lossy += 1;
            }
        }
    }
    (out, lossy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_passes_through() {
        let (b, lossy) = encode("Dark Castle");
        assert_eq!(b, b"Dark Castle");
        assert_eq!(lossy, 0);
    }

    #[test]
    fn accents_and_symbols() {
        assert_eq!(encode("é").0, vec![0x8E]);
        assert_eq!(encode("™").0, vec![0xAA]);
        assert_eq!(encode("Déjà Vu").0, vec![b'D', 0x8E, b'j', 0x88, b' ', b'V', b'u']);
        // ƒ (the classic "folder" glyph) round-trips to 0xC4.
        assert_eq!(encode("ƒ").0, vec![0xC4]);
    }

    #[test]
    fn unrepresentable_becomes_question_mark() {
        let (b, lossy) = encode("漢"); // CJK — not in MacRoman
        assert_eq!(b, b"?");
        assert_eq!(lossy, 1);
    }
}
