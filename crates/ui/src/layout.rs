use crate::{
    Bounds, CARET_WIDTH, CONTENT_GAP, DEFAULT_SOFT_WRAP_COLUMN, DISPLAY_COLUMN_WIDTH,
    EDITOR_PADDING, HEADER_HEIGHT, LINE_HEIGHT, LINE_NUMBER_WIDTH, Pixels, Point, SCROLLBAR_GAP,
    ScrollDelta, size,
};
use std::ops::Range;

pub(crate) fn byte_offset_for_display_column(text: &str, display_column: usize) -> Option<usize> {
    if display_column == text.chars().count() {
        return Some(text.len());
    }

    text.char_indices()
        .nth(display_column)
        .map(|(offset, _)| offset)
}

pub(crate) fn byte_offset_for_display_column_or_end(text: &str, display_column: usize) -> usize {
    byte_offset_for_display_column(text, display_column).unwrap_or(text.len())
}

pub(crate) fn display_column_for_byte_offset(text: &str, byte_offset: usize) -> usize {
    text.char_indices()
        .take_while(|(offset, _)| *offset < byte_offset)
        .count()
}

pub(crate) fn display_range_for_source_range(
    row_text: &str,
    row_source_range: &Range<usize>,
    range: Range<usize>,
) -> Option<Range<usize>> {
    if range.end <= row_source_range.start || range.start >= row_source_range.end {
        return None;
    }
    let start = range.start.max(row_source_range.start);
    let end = range.end.min(row_source_range.end);
    (start < end).then_some(
        display_column_for_byte_offset(row_text, start - row_source_range.start)
            ..display_column_for_byte_offset(row_text, end - row_source_range.start),
    )
}

pub(crate) fn floor_char_boundary(text: &str, mut offset: usize) -> usize {
    offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

pub(crate) fn marker_priority(marker: char) -> usize {
    match marker {
        '|' => 0,
        ']' => 1,
        '}' => 2,
        '[' => 3,
        '{' => 4,
        _ => 5,
    }
}

pub(crate) fn availability_label(available: bool) -> &'static str {
    if available { "on" } else { "off" }
}

pub(crate) fn visible_display_point_for_mouse_position(position: Point<Pixels>) -> (usize, usize) {
    let row_origin = scrollbar_track_top();
    let column_origin = EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP;
    let row = if position.y <= row_origin {
        0
    } else {
        ((position.y - row_origin) / LINE_HEIGHT).floor() as usize
    };
    let column = if position.x <= column_origin {
        0
    } else {
        ((position.x - column_origin) / DISPLAY_COLUMN_WIDTH).round() as usize
    };
    (row, column)
}

pub(crate) fn editor_text_width() -> Pixels {
    DISPLAY_COLUMN_WIDTH * DEFAULT_SOFT_WRAP_COLUMN
}

pub(crate) fn scrollbar_track_left() -> Pixels {
    EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + editor_text_width() + SCROLLBAR_GAP
}

pub(crate) fn scrollbar_track_top() -> Pixels {
    EDITOR_PADDING + HEADER_HEIGHT
}

pub(crate) fn bounds_for_visible_display_range(
    element_bounds: Bounds<Pixels>,
    visible_row: usize,
    columns: Range<usize>,
) -> Bounds<Pixels> {
    let column_count = columns.end.saturating_sub(columns.start);
    let width = if column_count == 0 {
        CARET_WIDTH
    } else {
        DISPLAY_COLUMN_WIDTH * column_count
    };
    Bounds {
        origin: Point {
            x: element_bounds.origin.x
                + EDITOR_PADDING
                + LINE_NUMBER_WIDTH
                + CONTENT_GAP
                + DISPLAY_COLUMN_WIDTH * columns.start,
            y: element_bounds.origin.y + EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT * visible_row,
        },
        size: size(width, LINE_HEIGHT),
    }
}

pub(crate) fn scroll_rows_for_delta(delta: ScrollDelta) -> isize {
    let rows = match delta {
        ScrollDelta::Lines(delta) => delta.y,
        ScrollDelta::Pixels(delta) => delta.y / LINE_HEIGHT,
    };

    if rows == 0.0 {
        return 0;
    }

    let row_count = rows.abs().ceil() as isize;
    if rows.is_sign_positive() {
        -row_count
    } else {
        row_count
    }
}
