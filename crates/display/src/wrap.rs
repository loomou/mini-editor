pub(crate) fn next_wrap_boundary(line: &str, start: usize, wrap_column: usize) -> usize {
    let mut count = 0;
    for (offset, _) in line[start..].char_indices() {
        if count == wrap_column {
            return start + offset;
        }
        count += 1;
    }
    line.len()
}
