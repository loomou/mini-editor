use std::ops::Range;

const DEFAULT_CHUNK_SIZE: usize = 32;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rope {
    chunks: Vec<String>,
    len: usize,
}

impl Rope {
    pub fn new(text: impl Into<String>) -> Self {
        Self::from_text_with_chunk_size(text.into(), DEFAULT_CHUNK_SIZE)
    }

    pub fn from_text_with_chunk_size(text: String, chunk_size: usize) -> Self {
        assert!(chunk_size > 0, "chunk size must be non-zero");
        let mut chunks = Vec::new();
        let mut chunk = String::new();

        for char in text.chars() {
            if chunk.len() + char.len_utf8() > chunk_size && !chunk.is_empty() {
                chunks.push(chunk);
                chunk = String::new();
            }
            chunk.push(char);
        }

        if !chunk.is_empty() || chunks.is_empty() {
            chunks.push(chunk)
        }

        let len = chunks.iter().map(|chunk| chunk.len()).sum();
        Self { chunks, len }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn chunks(&self) -> &[String] {
        &self.chunks
    }

    pub fn text(&self) -> String {
        self.chunks.concat()
    }

    pub fn slice(&self, range: Range<usize>) -> String {
        assert!(range.start <= range.end, "slice range is reversed");
        assert!(range.end <= self.len, "slice range is out of bounds");

        let mut output = String::new();
        let mut chunk_start = 0;
        for chunk in &self.chunks {
            let chunk_end = chunk_start + chunk.len();
            let start = range.start.max(chunk_start);
            let end = range.end.min(chunk_end);
            if start < end {
                output.push_str(&chunk[start - chunk_start..end - chunk_start]);
            }
            chunk_start = chunk_end;
            if chunk_start >= range.end {
                break;
            }
        }
        output
    }

    pub fn replace(&mut self, range: Range<usize>, replacement: impl Into<String>) {
        assert!(range.start <= range.end, "replace range is reversed");
        assert!(range.end <= self.len, "replace range is out of bounds");

        let mut text = self.text();
        text.replace_range(range, &replacement.into());
        *self = Self::from_text_with_chunk_size(text, DEFAULT_CHUNK_SIZE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_text_into_chunks() {
        let rope = Rope::from_text_with_chunk_size("abcdef".to_string(), 2);

        assert_eq!(rope.chunks(), &["ab", "cd", "ef"]);
        assert_eq!(rope.text(), "abcdef");
    }

    #[test]
    fn slices_across_chunk_boundaries() {
        let rope = Rope::from_text_with_chunk_size("abcdef".to_string(), 2);

        assert_eq!(rope.slice(1..5), "bcde");
    }

    #[test]
    fn replaces_text_and_rebalances_chunks() {
        let mut rope = Rope::from_text_with_chunk_size(
            "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ".to_string(),
            5,
        );

        rope.replace(10..20, "zed");

        assert_eq!(rope.text(), "abcdefghijzeduvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ");
        assert!(rope.chunks().len() > 1);
    }
}
