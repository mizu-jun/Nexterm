//! スクロールバック — 履歴バッファと全文検索

use regex::Regex;

use nexterm_proto::Cell;

/// スクロールバック履歴バッファ（リングバッファ）
pub struct Scrollback {
    /// 最大保持行数
    capacity: usize,
    /// 行データのリング
    lines: Vec<Vec<Cell>>,
    /// 書き込み先インデックス（次に上書きする位置）
    head: usize,
    /// 実際に格納されている行数
    len: usize,
}

impl Scrollback {
    /// 指定容量でスクロールバックを生成する
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            lines: Vec::with_capacity(capacity.min(1024)),
            head: 0,
            len: 0,
        }
    }

    /// 行を追加する（容量を超えたら最古行を上書き）
    pub fn push_line(&mut self, line: Vec<Cell>) {
        if self.len < self.capacity {
            if self.lines.len() <= self.head {
                self.lines.push(line);
            } else {
                self.lines[self.head] = line;
            }
            self.len += 1;
        } else {
            // 上書き
            if self.lines.len() <= self.head {
                self.lines.push(line);
            } else {
                self.lines[self.head] = line;
            }
        }
        self.head = (self.head + 1) % self.capacity;
    }

    /// 格納行数を返す
    pub fn len(&self) -> usize {
        self.len
    }

    /// 空かどうかを返す
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// インデックスで行を取得する（0 = 最古）
    pub fn get(&self, idx: usize) -> Option<&Vec<Cell>> {
        if idx >= self.len {
            return None;
        }
        // リング内の実際の位置を計算する
        let actual = if self.len < self.capacity {
            idx
        } else {
            (self.head + idx) % self.capacity
        };
        self.lines.get(actual)
    }

    /// 行をテキストに変換する（検索用）
    fn line_to_string(line: &[Cell]) -> String {
        line.iter()
            .map(|c| c.ch)
            .collect::<String>()
            .trim_end()
            .to_string()
    }

    /// 正規表現パターンで全行を検索する
    ///
    /// 戻り値: `(行インデックス, マッチ開始列, マッチ終了列)` のリスト
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

    /// インクリメンタル検索 — パターンにマッチする次の行インデックスを返す
    ///
    /// `from_row` から順方向に検索する。折り返しあり。
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexterm_proto::Cell;

    fn make_line(text: &str) -> Vec<Cell> {
        text.chars()
            .map(|ch| Cell { ch, ..Default::default() })
            .collect()
    }

    #[test]
    fn スクロールバックに行を追加できる() {
        let mut sb = Scrollback::new(100);
        sb.push_line(make_line("hello world"));
        sb.push_line(make_line("foo bar"));
        assert_eq!(sb.len(), 2);
    }

    #[test]
    fn 容量を超えると古い行が上書きされる() {
        let mut sb = Scrollback::new(3);
        sb.push_line(make_line("line1"));
        sb.push_line(make_line("line2"));
        sb.push_line(make_line("line3"));
        sb.push_line(make_line("line4")); // line1 が上書きされる

        // 最古行は line2 になっているはず
        let oldest = sb.get(0).unwrap();
        let text: String = oldest.iter().map(|c| c.ch).collect();
        assert_eq!(text.trim(), "line2");
        assert_eq!(sb.len(), 3);
    }

    #[test]
    fn 正規表現検索が動作する() {
        let mut sb = Scrollback::new(100);
        sb.push_line(make_line("hello world"));
        sb.push_line(make_line("foo bar baz"));
        sb.push_line(make_line("hello rust"));

        let results = sb.search("hello");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, 0); // 行0
        assert_eq!(results[1].0, 2); // 行2
    }

    #[test]
    fn インクリメンタル検索が次のマッチを返す() {
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
    fn 無効な正規表現は空を返す() {
        let mut sb = Scrollback::new(10);
        sb.push_line(make_line("test"));
        let results = sb.search("[invalid");
        assert!(results.is_empty());
    }
}
