//! 审计：打开结果时，从存下的文本把每张卡片的每一句重新核对一遍（同 v0.3 的 `audit`）。
//! 存下的结果被改坏（引文改了字、锚点换了段、标成已核实其实没核上、关系连到不存在的卡片、存下的文本被换了）都查得出来。

use serde::{Deserialize, Serialize};

use crate::model::{Run, Status};
use crate::prompts::Books;
use crate::verify::{self, SourceLookup};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    /// 卡片编号（K3）或关系编号（R2）
    pub at: String,
    /// 第几句（从 1 起）
    pub sentence: Option<usize>,
    pub message: String,
}

pub fn audit(run: &Run, books: &Books, sources: SourceLookup) -> Vec<Finding> {
    let mut out = Vec::new();
    for card in &run.cards {
        for (i, s) in card.sentences.iter().enumerate() {
            let mut again = s.clone();
            match verify::check(&mut again, books, sources) {
                Err(why) => out.push(Finding { at: card.id.clone(), sentence: Some(i + 1), message: format!("{}句核不上了：{why}", s.category.label()) }),
                Ok(()) => {
                    if again.check.status != s.check.status {
                        let claimed = match s.check.status {
                            Status::Verified => "已核实",
                            Status::Unanchored => "没挂上原文",
                            Status::Speculative => "推演",
                            Status::Pending => "没核过",
                        };
                        out.push(Finding { at: card.id.clone(), sentence: Some(i + 1), message: format!("存下的是「{claimed}」，重新核对的结果不一样") });
                    } else if again.check.span != s.check.span || again.check.pages != s.check.pages || again.check.zh_status != s.check.zh_status || again.paragraph != s.paragraph {
                        out.push(Finding { at: card.id.clone(), sentence: Some(i + 1), message: "存下的位置、页码或中译和重新核对的不一样".into() });
                    }
                }
            }
        }
    }
    for r in &run.relations {
        if run.card(&r.from).is_none() || run.card(&r.to).is_none() {
            out.push(Finding { at: r.id.clone(), sentence: None, message: format!("关系连着不存在的卡片（{} → {}）", r.from, r.to) });
            continue;
        }
        let mut again = r.note.clone();
        if let Err(why) = verify::check(&mut again, books, sources) {
            out.push(Finding { at: r.id.clone(), sentence: None, message: format!("关系的说明核不上了：{why}") });
        }
    }
    out
}
