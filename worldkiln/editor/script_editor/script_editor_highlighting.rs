use egui_code_editor::{ColorTheme, Syntax};
use std::collections::BTreeSet;

// Worldkiln 0.8.7 | WO 0.8.7-28 | 2026-10-06 12:40 -05:00
// The editor surface owns syntax layout and numbered-line presentation. These helpers
// keep AeoScript configuration and source-position conversion independent of widget geometry.
pub(crate) fn aeoscript_syntax() -> Syntax {
    Syntax::simple("//")
        .with_case_sensitive(true)
        .with_keywords(BTreeSet::from([
            "const", "else", "entity", "fn", "for", "if", "import", "in", "on", "return", "while",
        ]))
        .with_types(BTreeSet::from(["basket", "bool", "number", "string"]))
        .with_special(BTreeSet::from([
            "false", "nil", "true", "find", "get", "print", "spawn", "wait",
        ]))
}

pub(crate) fn aeoscript_editor_theme(dark_mode: bool) -> ColorTheme {
    if dark_mode {
        ColorTheme {
            name: "Worldkiln Dark",
            dark: true,
            bg: "#0B0B0B",
            cursor: "#E89130",
            selection: "#49311F",
            comments: "#6A9955",
            functions: "#61AFEF",
            keywords: "#C678DD",
            literals: "#DCDCDC",
            numerics: "#D19A66",
            punctuation: "#B4B4B4",
            strs: "#98C379",
            types: "#56B6C2",
            special: "#DCDCAA",
        }
    } else {
        ColorTheme {
            name: "Worldkiln Light",
            dark: false,
            bg: "#FFFFFF",
            cursor: "#873A16",
            selection: "#E8D5C8",
            comments: "#3F703D",
            functions: "#1856A4",
            keywords: "#7E3094",
            literals: "#202020",
            numerics: "#BE4A18",
            punctuation: "#464646",
            strs: "#2A7033",
            types: "#006976",
            special: "#7A5E00",
        }
    }
}

pub(crate) fn byte_offset_to_char_index(source: &str, byte_offset: usize) -> usize {
    source
        .get(..byte_offset.min(source.len()))
        .map(|prefix| prefix.chars().count())
        .unwrap_or_else(|| source.chars().count())
}

pub(crate) fn char_index_to_line(source: &str, char_index: usize) -> usize {
    let bounded = char_index.min(source.chars().count());

    source
        .chars()
        .take(bounded)
        .filter(|character| *character == '\n')
        .count()
        + 1
}

pub(crate) fn line_start_char_index(source: &str, one_based_line: usize) -> Option<usize> {
    if one_based_line == 0 {
        return None;
    }

    if one_based_line == 1 {
        return Some(0);
    }

    let mut line = 1usize;

    for (char_index, character) in source.chars().enumerate() {
        if character == '\n' {
            line += 1;

            if line == one_based_line {
                return Some(char_index + 1);
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aeoscript_syntax_recognizes_core_contract() {
        let syntax = aeoscript_syntax();

        assert!(syntax.is_keyword("entity"));
        assert!(syntax.is_keyword("fn"));
        assert!(syntax.is_keyword("const"));
        assert!(syntax.is_type("number"));
        assert!(syntax.is_type("basket"));
        assert!(syntax.is_special("true"));
        assert!(syntax.is_special("spawn"));
        assert_eq!(syntax.comment(), "//");
    }

    #[test]
    fn first_source_line_is_always_line_one() {
        let source = "// DUNGEON ENEMY BRAIN\nconst value = 1";

        assert_eq!(line_start_char_index(source, 1), Some(0));
        assert_eq!(char_index_to_line(source, 0), 1);
        assert_eq!(char_index_to_line(source, 5), 1);
    }

    #[test]
    fn trailing_newline_creates_an_addressable_empty_line() {
        let source = "first\n";

        assert_eq!(line_start_char_index(source, 1), Some(0));
        assert_eq!(
            line_start_char_index(source, 2),
            Some(source.chars().count())
        );
        assert_eq!(char_index_to_line(source, source.chars().count()), 2);
    }

    #[test]
    fn line_mapping_is_unicode_safe() {
        let source = "éPlayer\n第二行\nthird";

        assert_eq!(byte_offset_to_char_index(source, 2), 1);
        assert_eq!(line_start_char_index(source, 2), Some(8));
        assert_eq!(char_index_to_line(source, 8), 2);
        assert_eq!(line_start_char_index(source, 3), Some(12));
        assert_eq!(char_index_to_line(source, 12), 3);
    }

    #[test]
    fn long_source_line_identity_does_not_depend_on_scroll_offset() {
        let source = (1..=80)
            .map(|line| format!("line_{line}"))
            .collect::<Vec<_>>()
            .join("\n");

        let start = line_start_char_index(&source, 55).expect("line 55 should exist");

        assert_eq!(char_index_to_line(&source, start), 55);
        assert_eq!(
            &source.chars().skip(start).take(7).collect::<String>(),
            "line_55"
        );
    }
}
