use crate::selection::Selection;
use std::ops::Range;

pub(crate) fn floor_char_boundary(text: &str, offset: usize) -> usize {
    let mut clipped = offset.min(text.len());
    while clipped > 0 && !text.is_char_boundary(clipped) {
        clipped -= 1;
    }
    clipped
}

pub(crate) fn previous_char_boundary(text: &str, offset: usize) -> usize {
    let clipped = floor_char_boundary(text, offset);
    if clipped == 0 {
        return 0;
    }
    text[..clipped]
        .char_indices()
        .last()
        .map(|(offset, _)| offset)
        .unwrap_or(0)
}

pub(crate) fn next_char_boundary(text: &str, offset: usize) -> usize {
    let mut clipped = offset.min(text.len());
    while clipped < text.len() && !text.is_char_boundary(clipped) {
        clipped += 1;
    }
    if clipped == text.len() {
        return text.len();
    }
    let mut chars = text[clipped..].char_indices();
    let _current = chars.next();
    chars
        .next()
        .map(|(relative_offset, _)| clipped + relative_offset)
        .unwrap_or(text.len())
}

pub(crate) fn previous_word_boundary(text: &str, offset: usize) -> usize {
    let clipped = floor_char_boundary(text, offset);
    let chars = text[..clipped].char_indices().collect::<Vec<_>>();
    let Some((mut index, _)) = chars
        .len()
        .checked_sub(1)
        .map(|index| (index, chars[index].1))
    else {
        return 0;
    };

    if is_word_char(chars[index].1) {
        while index > 0 && is_word_char(chars[index - 1].1) {
            index -= 1;
        }
        return chars[index].0;
    }

    while index > 0 && !is_word_char(chars[index].1) {
        index -= 1;
    }
    if !is_word_char(chars[index].1) {
        return 0;
    }
    while index > 0 && is_word_char(chars[index - 1].1) {
        index -= 1;
    }
    chars[index].0
}

pub(crate) fn next_word_boundary(text: &str, offset: usize) -> usize {
    let mut clipped = floor_char_boundary(text, offset);
    if clipped >= text.len() {
        return text.len();
    }

    while clipped < text.len() {
        let Some(character) = text[clipped..].chars().next() else {
            return text.len();
        };
        if !is_word_char(character) {
            break;
        }
        clipped += character.len_utf8();
    }

    while clipped < text.len() {
        let Some(character) = text[clipped..].chars().next() else {
            return text.len();
        };
        if is_word_char(character) {
            break;
        }
        clipped += character.len_utf8();
    }

    clipped
}

pub(crate) fn is_word_char(character: char) -> bool {
    character == '_' || character.is_alphanumeric()
}

pub(crate) fn word_range_at_offset(text: &str, offset: usize) -> Range<usize> {
    let mut clipped = floor_char_boundary(text, offset);
    if clipped == text.len() && clipped > 0 {
        clipped = previous_char_boundary(text, clipped);
    }

    let Some(character) = text.get(clipped..).and_then(|text| text.chars().next()) else {
        return clipped..clipped;
    };
    if !is_word_char(character) {
        return clipped..next_char_boundary(text, clipped);
    }

    let mut start = clipped;
    while start > 0 {
        let previous = previous_char_boundary(text, start);
        let Some(character) = text
            .get(previous..start)
            .and_then(|text| text.chars().next())
        else {
            break;
        };
        if !is_word_char(character) {
            break;
        }
        start = previous;
    }

    let mut end = clipped;
    while end < text.len() {
        let Some(character) = text.get(end..).and_then(|text| text.chars().next()) else {
            break;
        };
        if !is_word_char(character) {
            break;
        }
        end += character.len_utf8();
    }

    start..end
}

pub(crate) fn line_range_at_offset(text: &str, offset: usize) -> Range<usize> {
    let clipped = floor_char_boundary(text, offset);
    let start = text[..clipped]
        .rfind('\n')
        .map(|offset| offset + 1)
        .unwrap_or(0);
    let end = text[clipped..]
        .find('\n')
        .map(|offset| clipped + offset)
        .unwrap_or(text.len());
    start..end
}

pub(crate) fn find_next_non_overlapping_match(
    text: &str,
    query: &str,
    search_start: usize,
    selections: &[Selection],
) -> Option<Range<usize>> {
    find_non_overlapping_match(text, query, search_start, text.len(), selections)
        .or_else(|| find_non_overlapping_match(text, query, 0, search_start, selections))
}

pub(crate) fn find_non_overlapping_match(
    text: &str,
    query: &str,
    start: usize,
    end: usize,
    selections: &[Selection],
) -> Option<Range<usize>> {
    if start >= end {
        return None;
    }

    let mut cursor = floor_char_boundary(text, start);
    let end = floor_char_boundary(text, end);
    while cursor < end {
        let haystack = &text[cursor..end];
        let Some(relative_offset) = haystack.find(query) else {
            return None;
        };
        let match_start = cursor + relative_offset;
        let match_end = match_start + query.len();
        let range = match_start..match_end;
        if !selections
            .iter()
            .any(|selection| selection.start == range.start && selection.end == range.end)
            && !selections
                .iter()
                .any(|selection| selection.start < range.end && range.start < selection.end)
        {
            return Some(range);
        }
        cursor = next_char_boundary(text, match_start);
    }

    None
}

pub(crate) fn find_all_non_overlapping_matches(text: &str, query: &str) -> Vec<Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }

    let mut ranges = Vec::new();
    let mut cursor = 0;
    while cursor < text.len() {
        let haystack = &text[cursor..];
        let Some(relative_offset) = haystack.find(query) else {
            break;
        };
        let match_start = cursor + relative_offset;
        let match_end = match_start + query.len();
        ranges.push(match_start..match_end);
        cursor = match_end;
    }
    ranges
}
