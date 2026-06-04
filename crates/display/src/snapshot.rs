use crate::columns::{display_column_for_source_column, source_column_for_display_column};
use crate::{DisplayPoint, DisplayRow};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct DisplaySnapshot {
    pub(crate) rows: Arc<[DisplayRow]>,
    pub(crate) source_len: usize,
}

impl DisplaySnapshot {
    pub(crate) fn new(rows: Vec<DisplayRow>, source_len: usize) -> Self {
        Self {
            rows: Arc::from(rows),
            source_len,
        }
    }

    pub fn rows(&self) -> &[DisplayRow] {
        &self.rows
    }

    pub fn shares_rows_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.rows, &other.rows)
    }

    pub fn source_len(&self) -> usize {
        self.source_len
    }

    pub fn text(&self) -> String {
        self.rows
            .iter()
            .map(|row| row.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn source_offset_for_display_point(&self, point: DisplayPoint) -> usize {
        let Some(row) = self.rows.get(point.row) else {
            return self.source_len;
        };
        row.source_range.start + source_column_for_display_column(&row.text, point.column)
    }

    pub fn display_point_for_source_offset(&self, source_offset: usize) -> DisplayPoint {
        let source_offset = source_offset.min(self.source_len);
        for row in self.rows.iter() {
            if source_offset >= row.source_range.start && source_offset <= row.source_range.end {
                return DisplayPoint {
                    row: row.row,
                    column: display_column_for_source_column(
                        &row.text,
                        source_offset - row.source_range.start,
                    ),
                };
            }
        }

        self.rows
            .last()
            .map(|row| DisplayPoint {
                row: row.row,
                column: row.text.chars().count(),
            })
            .unwrap_or(DisplayPoint { row: 0, column: 0 })
    }

    pub fn source_offset_for_vertical_movement(
        &self,
        source_offset: usize,
        row_delta: isize,
        desired_column: usize,
    ) -> usize {
        let point = self.display_point_for_source_offset(source_offset);
        let target_row = point
            .row
            .saturating_add_signed(row_delta)
            .min(self.rows.len().saturating_sub(1));
        self.source_offset_for_display_point(DisplayPoint {
            row: target_row,
            column: desired_column,
        })
    }
}
