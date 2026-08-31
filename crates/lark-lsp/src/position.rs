//! Positions, in the two forms that the two sides use.
//!
//! Lark works in byte offsets. The protocol works in a line and a character,
//! and it counts a character in UTF-16 code units.

/// Turns a line and a character into a byte offset.
pub fn to_offset(text: &str, line: u32, character: u32) -> u32 {
    let mut offset = 0usize;

    for (current_line, raw) in text.split_inclusive('\n').enumerate() {
        if u32::try_from(current_line).unwrap_or(u32::MAX) == line {
            return offset_in_line(raw, character, offset);
        }
        offset += raw.len();
    }
    u32::try_from(text.len()).unwrap_or(0)
}

/// Turns a byte offset into a line and a character.
pub fn to_position(text: &str, offset: u32) -> (u32, u32) {
    let end = (offset as usize).min(text.len());
    let head = &text[..end];
    let line = u32::try_from(head.matches('\n').count()).unwrap_or(0);
    let start = head.rfind('\n').map_or(0, |index| index + 1);
    let character =
        u32::try_from(text[start..end].chars().map(char::len_utf16).sum::<usize>()).unwrap_or(0);
    (line, character)
}

/// Returns the byte offset of a character inside one line.
fn offset_in_line(line: &str, character: u32, base: usize) -> u32 {
    let mut units = 0u32;
    for (index, item) in line.char_indices() {
        if units >= character {
            return u32::try_from(base + index).unwrap_or(0);
        }
        units += u32::try_from(item.len_utf16()).unwrap_or(1);
    }
    u32::try_from(base + line.trim_end_matches(['\n', '\r']).len()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{to_offset, to_position};

    #[test]
    fn a_position_and_an_offset_agree() {
        let text = "one\ntwo\nthree\n";
        assert_eq!(to_offset(text, 0, 0), 0);
        assert_eq!(to_offset(text, 1, 0), 4);
        assert_eq!(to_offset(text, 1, 2), 6);
        assert_eq!(to_offset(text, 2, 5), 13);

        assert_eq!(to_position(text, 0), (0, 0));
        assert_eq!(to_position(text, 4), (1, 0));
        assert_eq!(to_position(text, 6), (1, 2));
    }

    #[test]
    fn a_character_counts_utf16_units() {
        // The euro sign is one UTF-16 unit and three bytes.
        let text = "a\u{20ac}b\n";
        assert_eq!(to_offset(text, 0, 2), 4);
        assert_eq!(to_position(text, 4), (0, 2));

        // A character outside the basic plane takes two units.
        let wide = "a\u{1F600}b\n";
        assert_eq!(to_position(wide, 5), (0, 3));
        assert_eq!(to_offset(wide, 0, 3), 5);
    }

    #[test]
    fn a_position_past_the_end_lands_on_the_end() {
        let text = "one\n";
        assert_eq!(to_offset(text, 9, 0), 4);
        assert_eq!(to_position(text, 99), (1, 0));
    }
}
