//! 文字的「折叠」：核对摘录时只容忍空白和全半角的差异（`decisions/0020`「核对」）。
//!
//! 折叠 = 去掉所有空白 + 全角形式（Ｕ+FF01–FF5E、全角空格）换成半角。别的一概不动：
//! 「。」和「.」、弯引号和直引号、简体和繁体都算不同的字。

/// 一个字符折叠后是什么；空白返回 `None`（被去掉）
pub fn fold_char(c: char) -> Option<char> {
    if c.is_whitespace() || matches!(c, '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{FEFF}' | '\u{00AD}') {
        return None;
    }
    Some(half_width(c))
}

/// 全角形式换成半角（不去空白）
pub fn half_width(c: char) -> char {
    match c {
        '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
        '\u{3000}' => ' ',
        _ => c,
    }
}

pub fn fold(text: &str) -> String {
    text.chars().filter_map(fold_char).collect()
}

/// 折叠后的文字，记得每个字符在原文里的位置（按字符数）
pub struct Folded {
    text: String,
    /// 折叠后第 i 个字符：(在 `text` 里的字节位置, 在原文里是第几个字符)
    index: Vec<(usize, usize)>,
    original_chars: usize,
}

/// 在原文里的位置，按字符数，`start..end`
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Folded {
    pub fn new(original: &str) -> Folded {
        let mut text = String::with_capacity(original.len());
        let mut index = Vec::new();
        let mut n = 0;
        for (i, c) in original.chars().enumerate() {
            n = i + 1;
            if let Some(f) = fold_char(c) {
                index.push((text.len(), i));
                text.push(f);
            }
        }
        Folded { text, index, original_chars: n }
    }

    pub fn len_chars(&self) -> usize {
        self.index.len()
    }

    /// 找折叠后的 `needle`（调用方先 `fold`）：返回第一处在原文里的位置，和一共有几处
    pub fn find(&self, needle: &str) -> Option<(Span, usize)> {
        if needle.is_empty() {
            return None;
        }
        let first = self.text.find(needle)?;
        let count = self.text.matches(needle).count();
        let at = self.index.binary_search_by_key(&first, |(byte, _)| *byte).ok()?;
        let last = at + needle.chars().count() - 1;
        let start = self.index[at].1;
        let end = self.index.get(last).map(|(_, orig)| orig + 1).unwrap_or(self.original_chars);
        Some((Span { start, end }, count))
    }
}

/// 按字符位置切原文
pub fn slice_chars(text: &str, start: usize, end: usize) -> String {
    text.chars().skip(start).take(end.saturating_sub(start)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_only_whitespace_and_width() {
        assert_eq!(fold("示例市 ２０２４ 年\n青年失业率为　１２．３％"), "示例市2024年青年失业率为12.3%");
        assert_eq!(fold("A，B；C（D）"), "A,B;C(D)");
        // 不是全半角关系的不动
        assert_eq!(fold("句号。和.不同"), "句号。和.不同");
        assert_eq!(fold("“弯”和\"直\""), "“弯”和\"直\"");
        assert_eq!(fold("零宽\u{200B}空格"), "零宽空格");
    }

    #[test]
    fn finds_and_maps_back() {
        let original = "第一段。\n\n示例市 2024 年青年失业率为 12.3％，\n比上年下降。";
        let f = Folded::new(original);
        let (span, count) = f.find(&fold("2024年青年失业率为12.3%")).unwrap();
        assert_eq!(count, 1);
        assert_eq!(slice_chars(original, span.start, span.end), "2024 年青年失业率为 12.3％");
        assert!(f.find(&fold("2023年青年失业率")).is_none());
        assert!(f.find("").is_none());
        // 出现两次：给第一处，报两处
        let twice = Folded::new("甲乙丙，甲乙丙");
        let (span, count) = twice.find("甲乙丙").unwrap();
        assert_eq!((span, count), (Span { start: 0, end: 3 }, 2));
        // 结尾处
        let (span, _) = Folded::new("甲 乙 丙 ").find("乙丙").unwrap();
        assert_eq!(span, Span { start: 2, end: 5 });
    }
}
