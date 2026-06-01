pub(crate) fn source_column_for_display_column(text: &str, display_column: usize) -> usize {
    text.char_indices()
        .nth(display_column)
        .map(|(offset, _)| offset)
        .unwrap_or(text.len())
}

pub(crate) fn display_column_for_source_column(text: &str, source_column: usize) -> usize {
    text.char_indices()
        .take_while(|(offset, _)| *offset < source_column)
        .count()
}
