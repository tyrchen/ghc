//! Table formatting for CLI output.
//!
//! Maps from Go's `internal/tableprinter` package.

use comfy_table::{Cell, ContentArrangement, Table as ComfyTable};

use crate::iostreams::IOStreams;

/// Table printer that adapts output based on TTY/non-TTY mode.
#[derive(Debug)]
pub struct TablePrinter {
    is_tty: bool,
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
}

impl TablePrinter {
    /// Create a new table printer.
    pub fn new(ios: &IOStreams) -> Self {
        Self {
            is_tty: ios.is_stdout_tty(),
            headers: Vec::new(),
            rows: Vec::new(),
        }
    }

    /// Set table headers. Pass empty to disable headers.
    #[must_use]
    pub fn with_headers(mut self, headers: &[&str]) -> Self {
        self.headers = headers.iter().map(|h| h.to_uppercase()).collect();
        self
    }

    /// Add a row of values.
    pub fn add_row(&mut self, fields: Vec<String>) {
        self.rows.push(fields);
    }

    /// Render the table to a string.
    pub fn render(&self) -> String {
        if self.is_tty {
            self.render_tty()
        } else {
            self.render_plain()
        }
    }

    fn render_tty(&self) -> String {
        let mut table = ComfyTable::new();
        table.set_content_arrangement(ContentArrangement::Dynamic);
        table.load_preset(comfy_table::presets::NOTHING);

        if !self.headers.is_empty() {
            let header_cells: Vec<Cell> = self.headers.iter().map(Cell::new).collect();
            table.set_header(header_cells);
        }

        for row in &self.rows {
            let cells: Vec<Cell> = row.iter().map(Cell::new).collect();
            table.add_row(cells);
        }

        table.to_string()
    }

    fn render_plain(&self) -> String {
        self.rows
            .iter()
            .map(|row| row.join("\t"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Check if the table is empty.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Number of rows.
    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_render_plain_tab_separated() {
        let ios = IOStreams::test();
        let mut tp = TablePrinter::new(&ios);
        tp.add_row(vec!["1".into(), "hello".into(), "open".into()]);
        tp.add_row(vec!["2".into(), "world".into(), "closed".into()]);

        let output = tp.render();
        assert!(output.contains("1\thello\topen"));
        assert!(output.contains("2\tworld\tclosed"));
    }

    #[test]
    fn test_should_separate_rows_with_newlines() {
        let ios = IOStreams::test();
        let mut tp = TablePrinter::new(&ios);
        tp.add_row(vec!["a".into()]);
        tp.add_row(vec!["b".into()]);

        let output = tp.render();
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "a");
        assert_eq!(lines[1], "b");
    }

    #[test]
    fn test_should_track_row_count() {
        let ios = IOStreams::test();
        let mut tp = TablePrinter::new(&ios);
        assert!(tp.is_empty());
        assert_eq!(tp.len(), 0);

        tp.add_row(vec!["test".into()]);
        assert!(!tp.is_empty());
        assert_eq!(tp.len(), 1);

        tp.add_row(vec!["another".into()]);
        assert_eq!(tp.len(), 2);
    }

    #[test]
    fn test_should_render_empty_table() {
        let ios = IOStreams::test();
        let tp = TablePrinter::new(&ios);
        let output = tp.render();
        assert!(output.is_empty());
    }

    #[test]
    fn test_should_support_with_headers() {
        let ios = IOStreams::test();
        let tp = TablePrinter::new(&ios).with_headers(&["id", "title", "state"]);
        // Headers are uppercased
        assert!(tp.is_empty()); // No rows added
    }

    #[test]
    fn test_should_handle_single_column() {
        let ios = IOStreams::test();
        let mut tp = TablePrinter::new(&ios);
        tp.add_row(vec!["only-column".into()]);

        let output = tp.render();
        assert_eq!(output, "only-column");
    }
}
