//! 概念研究自己的文字折叠（`decisions/0024`「内容的分类与核对」）：在 v0.3 折叠（去空白、全角换半角，
//! `research_engine::fold`）之外，再容忍
//!
//! - 弯直引号：‘ ’ ‚ ‛ ′ ` ´ ʼ → '，“ ” „ ‟ ″ → "（撇号的样式也在里面）；
//! - 拉丁字母大小写：EPUB 段首的小型大写（`THE greatest`）和模型写的 `The greatest` 算同一句。
//!
//! 别的照旧不容忍：标点、连字符、繁简、拼写（judgment / judgement）都算不同。v0.3 的 `fold` 一点没改。

use research_engine::fold::{self as base, Span};

pub fn fold_char(c: char) -> Option<char> {
    let c = base::fold_char(c)?;
    Some(match c {
        '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' | '\u{2032}' | '`' | '\u{00B4}' | '\u{02BC}' => '\'',
        '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' | '\u{2033}' => '"',
        c if c.is_alphabetic() && is_latin(c) => {
            let mut l = c.to_lowercase();
            match (l.next(), l.next()) {
                (Some(x), None) => x,
                _ => c,
            }
        }
        c => c,
    })
}

fn is_latin(c: char) -> bool {
    matches!(c as u32, 0x41..=0x5A | 0x61..=0x7A | 0xC0..=0x24F | 0x1E00..=0x1EFF)
}

pub fn fold(text: &str) -> String {
    text.chars().filter_map(fold_char).collect()
}

/// 折叠后的文字，记得每个字在原文里的位置（同 `research_engine::fold::Folded`，用这里的规则）
pub struct Folded {
    text: String,
    index: Vec<(usize, usize)>,
    original_chars: usize,
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

    /// 找一句引文（原样传进来，这里折叠）：第一处在原文里的位置，和一共几处
    pub fn find(&self, quote: &str) -> Option<(Span, usize)> {
        let needle = fold(quote);
        if needle.is_empty() {
            return None;
        }
        let first = self.text.find(&needle)?;
        let count = self.text.matches(&needle).count();
        let at = self.index.binary_search_by_key(&first, |(byte, _)| *byte).ok()?;
        let last = at + needle.chars().count() - 1;
        let start = self.index[at].1;
        let end = self.index.get(last).map(|(_, orig)| orig + 1).unwrap_or(self.original_chars);
        Some((Span { start, end }, count))
    }
}

pub use base::slice_chars;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tolerates_quotes_apostrophes_and_case_only() {
        assert_eq!(fold("THE greatest"), fold("The greatest"));
        assert_eq!(fold("workman’s"), fold("workman's"));
        assert_eq!(fold("“trifling”"), fold("\"trifling\""));
        assert_eq!(fold("Ｔｈｅ　ｍａｎ"), "theman");
        assert_ne!(fold("judgment"), fold("judgement"));
        assert_ne!(fold("pin-maker"), fold("pin maker"));
        assert_ne!(fold("劳动。"), fold("劳动."));
        // v0.3 的折叠没变：引号和大小写照样算不同
        assert_ne!(base::fold("“a”"), base::fold("\"a\""));
        assert_ne!(base::fold("A"), base::fold("a"));
    }

    #[test]
    fn finds_back_in_the_original() {
        let original = "THE greatest improvement in the productive powers of labour";
        let f = Folded::new(original);
        let (span, n) = f.find("The greatest improvement").unwrap();
        assert_eq!(n, 1);
        assert_eq!(slice_chars(original, span.start, span.end), "THE greatest improvement");
        assert!(f.find("The greatest improvements").is_none());
        assert!(f.find("  ").is_none());
    }
}
