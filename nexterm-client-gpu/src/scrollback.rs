//! Scrollback — history buffer plus full-text search.

use regex::Regex;

use nexterm_proto::Cell;

/// Scrollback history buffer (ring buffer).
pub struct Scrollback {
    /// Maximum number of retained rows.
    capacity: usize,
    /// Ring of row data.
    lines: Vec<Vec<Cell>>,
    /// Write index (the slot the next push will overwrite).
    head: usize,
    /// Number of rows currently stored.
    len: usize,
}

impl Scrollback {
    /// Build a scrollback with the given capacity.
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            lines: Vec::with_capacity(capacity.min(1024)),
            head: 0,
            len: 0,
        }
    }

    /// Append a row (overwriting the oldest one once capacity is exceeded).
    pub fn push_line(&mut self, line: Vec<Cell>) {
        if self.len < self.capacity {
            if self.lines.len() <= self.head {
                self.lines.push(line);
            } else {
                self.lines[self.head] = line;
            }
            self.len += 1;
        } else {
            // Overwrite.
            if self.lines.len() <= self.head {
                self.lines.push(line);
            } else {
                self.lines[self.head] = line;
            }
        }
        self.head = (self.head + 1) % self.capacity;
    }

    /// Return the number of stored rows.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Return whether the buffer is empty.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Retrieve a row by index (0 = oldest).
    pub fn get(&self, idx: usize) -> Option<&Vec<Cell>> {
        if idx >= self.len {
            return None;
        }
        // Translate the logical index to the ring position.
        let actual = if self.len < self.capacity {
            idx
        } else {
            (self.head + idx) % self.capacity
        };
        self.lines.get(actual)
    }

    /// Convert a row into a string (used by search).
    fn line_to_string(line: &[Cell]) -> String {
        line.iter()
            .map(|c| c.ch)
            .collect::<String>()
            .trim_end()
            .to_string()
    }

    /// Search every row with a regex pattern.
    ///
    /// Returns a list of `(row index, match start column, match end column)` tuples.
    #[allow(dead_code)]
    pub fn search(&self, pattern: &str) -> Vec<(usize, usize, usize)> {
        let Ok(re) = Regex::new(pattern) else {
            return vec![];
        };

        let mut results = Vec::new();
        for row_idx in 0..self.len {
            if let Some(line) = self.get(row_idx) {
                let text = Self::line_to_string(line);
                for m in re.find_iter(&text) {
                    results.push((row_idx, m.start(), m.end()));
                }
            }
        }
        results
    }

    /// Incremental search — return the next row index matching the pattern.
    ///
    /// Searches forward starting at `from_row` and wraps around.
    pub fn search_next(&self, pattern: &str, from_row: usize) -> Option<usize> {
        if pattern.is_empty() {
            return None;
        }
        let Ok(re) = Regex::new(pattern) else {
            return None;
        };

        for offset in 0..self.len {
            let row_idx = (from_row + offset) % self.len;
            if let Some(line) = self.get(row_idx) {
                let text = Self::line_to_string(line);
                if re.is_match(&text) {
                    return Some(row_idx);
                }
            }
        }
        None
    }

    /// Incremental search — return the previous row index matching the pattern.
    ///
    /// Searches backward from rows preceding `before_row` and wraps around.
    pub fn search_prev(&self, pattern: &str, before_row: usize) -> Option<usize> {
        if pattern.is_empty() || self.len == 0 {
            return None;
        }
        let Ok(re) = Regex::new(pattern) else {
            return None;
        };

        for offset in 1..=self.len {
            let row_idx = (before_row + self.len - offset) % self.len;
            if let Some(line) = self.get(row_idx) {
                let text = Self::line_to_string(line);
                if re.is_match(&text) {
                    return Some(row_idx);
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexterm_proto::Cell;

    fn make_line(text: &str) -> Vec<Cell> {
        text.chars()
            .map(|ch| Cell {
                ch,
                ..Default::default()
            })
            .collect()
    }

    #[test]
    fn scrollback_accepts_pushed_rows() {
        let mut sb = Scrollback::new(100);
        sb.push_line(make_line("hello world"));
        sb.push_line(make_line("foo bar"));
        assert_eq!(sb.len(), 2);
    }

    #[test]
    fn exceeding_capacity_overwrites_oldest_rows() {
        let mut sb = Scrollback::new(3);
        sb.push_line(make_line("line1"));
        sb.push_line(make_line("line2"));
        sb.push_line(make_line("line3"));
        sb.push_line(make_line("line4")); // line1 is overwritten

        // The oldest row should now be line2.
        let oldest = sb.get(0).unwrap();
        let text: String = oldest.iter().map(|c| c.ch).collect();
        assert_eq!(text.trim(), "line2");
        assert_eq!(sb.len(), 3);
    }

    #[test]
    fn regex_search_works() {
        let mut sb = Scrollback::new(100);
        sb.push_line(make_line("hello world"));
        sb.push_line(make_line("foo bar baz"));
        sb.push_line(make_line("hello rust"));

        let results = sb.search("hello");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, 0); // row 0
        assert_eq!(results[1].0, 2); // row 2
    }

    #[test]
    fn incremental_search_returns_next_match() {
        let mut sb = Scrollback::new(100);
        sb.push_line(make_line("first match here"));
        sb.push_line(make_line("no hit here"));
        sb.push_line(make_line("second match here"));

        let result = sb.search_next("match", 0);
        assert_eq!(result, Some(0));

        let result2 = sb.search_next("match", 1);
        assert_eq!(result2, Some(2));
    }

    #[test]
    fn invalid_regex_returns_empty() {
        let mut sb = Scrollback::new(10);
        sb.push_line(make_line("test"));
        let results = sb.search("[invalid");
        assert!(results.is_empty());
    }

    // -------------------------------------------------------------------
    // Property-based invariants (QA persona: データ整合性監査役)
    //
    // The scrollback is a ring buffer; the invariants below must hold for
    // any sequence of pushes regardless of capacity.
    // -------------------------------------------------------------------
    use proptest::prelude::*;

    proptest! {
        /// `len()` never exceeds `capacity`, and equals min(pushed, capacity).
        #[test]
        fn len_never_exceeds_capacity(
            capacity in 1usize..64,
            pushes in 0usize..256,
        ) {
            let mut sb = Scrollback::new(capacity);
            for i in 0..pushes {
                sb.push_line(make_line(&format!("row{}", i)));
            }
            prop_assert!(sb.len() <= capacity);
            prop_assert_eq!(sb.len(), pushes.min(capacity));
        }

        /// `get(i)` is `Some` for every `i < len()` and `None` afterwards.
        #[test]
        fn get_is_well_defined_within_len(
            capacity in 1usize..32,
            pushes in 0usize..128,
        ) {
            let mut sb = Scrollback::new(capacity);
            for i in 0..pushes {
                sb.push_line(make_line(&format!("row{}", i)));
            }
            for i in 0..sb.len() {
                prop_assert!(sb.get(i).is_some());
            }
            prop_assert!(sb.get(sb.len()).is_none());
            prop_assert!(sb.get(sb.len() + 1).is_none());
        }

        /// After more than `capacity` pushes the oldest visible row is the
        /// `(pushes - capacity)`-th row that was inserted.
        #[test]
        fn oldest_row_matches_ring_semantics(
            capacity in 2usize..16,
            extra in 0usize..32,
        ) {
            let pushes = capacity + extra;
            let mut sb = Scrollback::new(capacity);
            for i in 0..pushes {
                sb.push_line(make_line(&format!("row{}", i)));
            }
            let oldest_expected = pushes - capacity;
            let oldest_actual: String = sb
                .get(0)
                .unwrap()
                .iter()
                .map(|c| c.ch)
                .collect();
            prop_assert_eq!(oldest_actual.trim(), format!("row{}", oldest_expected));
        }
    }
}
