//! Aligned columns for the terminal.

/// Rows of cells, printed with each column as wide as its widest cell.
#[derive(Default)]
pub struct Table {
    rows: Vec<Row>,
}

enum Row {
    Cells(Vec<String>),
    /// A line under the row before, indented to its second column.
    Detail(String),
}

impl Table {
    pub fn row<S: Into<String>>(&mut self, cells: impl IntoIterator<Item = S>) {
        self.rows.push(Row::Cells(cells.into_iter().map(Into::into).collect()));
    }

    pub fn detail(&mut self, text: impl Into<String>) {
        self.rows.push(Row::Detail(text.into()));
    }

    pub fn render(&self, indent: usize) -> String {
        let mut widths: Vec<usize> = Vec::new();
        for row in &self.rows {
            if let Row::Cells(cells) = row {
                for (i, c) in cells.iter().enumerate() {
                    let w = c.chars().count();
                    match widths.get_mut(i) {
                        Some(x) => *x = (*x).max(w),
                        None => widths.push(w),
                    }
                }
            }
        }
        let detail_at = indent + widths.first().map_or(0, |w| w + 2);
        let mut out = String::new();
        for row in &self.rows {
            match row {
                Row::Cells(cells) => {
                    let mut line = " ".repeat(indent);
                    for (i, c) in cells.iter().enumerate() {
                        line.push_str(c);
                        if i + 1 < cells.len() {
                            line.push_str(&" ".repeat(widths[i] - c.chars().count() + 2));
                        }
                    }
                    out.push_str(line.trim_end());
                    out.push('\n');
                }
                Row::Detail(text) => {
                    out.push_str(&" ".repeat(detail_at));
                    out.push_str(text);
                    out.push('\n');
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_line_up() {
        let mut t = Table::default();
        t.row(["a", "bbb", "c"]);
        t.detail("under");
        t.row(["dddd", "e", ""]);
        assert_eq!(t.render(2), "  a     bbb  c\n        under\n  dddd  e\n");
    }
}
