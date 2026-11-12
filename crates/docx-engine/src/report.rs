//! 中文 Markdown 报告：改了什么、没处理什么、校验结果。

use crate::format::Report;
use crate::verify::{Check, Problem};

pub fn markdown(rep: Option<&Report>, problems: &[Problem], checks: &[Check]) -> String {
    let mut s = String::new();
    let fidelity_ok = problems.is_empty();
    let spec_ok = checks.iter().all(|c| c.ok);
    s.push_str(&format!(
        "# 排版报告\n\n**结论：{}**\n\n- 正文保真：{}\n- 规范生效：{}\n\n",
        if fidelity_ok && spec_ok { "通过" } else { "不通过" },
        if fidelity_ok { if rep.is_some_and(|r| r.structure.is_some()) { "通过（文字、域、公式、其他部件均未变；结构只有下面列出的改动）".to_string() } else { "通过（文字、域、公式、结构、其他部件均未变）".to_string() } } else { format!("发现 {} 处问题", problems.len()) },
        if checks.is_empty() { "未检查".to_string() } else if spec_ok { format!("{} 项全部符合", checks.len()) } else { format!("{} 项不符合", checks.iter().filter(|c| !c.ok).count()) }
    ));
    if let Some(r) = rep {
        s.push_str(&format!("## 改动\n\n规范：{}\n\n", r.spec));
        s.push_str(&format!("- 改动的部件：{}\n", r.parts_changed.join("、")));
        s.push_str(&format!("- 修改的样式：{}\n", r.styles_changed.join("、")));
        if !r.styles_created.is_empty() {
            s.push_str(&format!("- 新建的样式：{}\n", r.styles_created.join("、")));
        }
        s.push_str(&format!(
            "- 清理直接格式：run {} 处、段落 {} 处\n- 表格段落套用表格文字样式：{}；图片 / 公式段落套用居中样式：{}\n- 代码段落保留原格式：{}；列表 / 参考文献段落保留缩进：{}\n- 分节页面设置：{} 处\n\n",
            r.runs_cleaned, r.paragraphs_cleaned, r.table_paragraphs, r.figure_paragraphs, r.code_paragraphs_kept, r.list_paragraphs_kept_indent, r.sections
        ));
        if r.table_bib_paragraphs > 0 {
            s.push_str(&format!("- 表格里原来用参考文献样式的段落：{} 个，按表格文字排（参考文献的缩进放进窄单元格会把字挤断）\n", r.table_bib_paragraphs));
        }
        if r.page_breaks_not_doubled > 0 {
            s.push_str(&format!("- 不再加段前分页：{} 处（前面已有分节符或分页符，再加会多出空白页；或是一个标题分两段写，再加会拆到两页）\n", r.page_breaks_not_doubled));
        }
        if !r.own_indent_styles.is_empty() || r.own_indent_paragraphs > 0 {
            let mut what = Vec::new();
            if !r.own_indent_styles.is_empty() {
                what.push(format!("样式 {}", r.own_indent_styles.join("、")));
            }
            if r.own_indent_paragraphs > 0 {
                what.push(format!("段落 {} 个", r.own_indent_paragraphs));
            }
            s.push_str(&format!("- 自带缩进写明 0 字符缩进：{}（正文改成按字符缩进后，Word 会用继承来的字符缩进盖掉它们自己的悬挂缩进）\n", what.join("；")));
        }
        if let Some(a) = &r.bibliography_align_kept {
            s.push_str(&format!("- 参考文献保持原来的对齐（{}）：规范没写参考文献的对齐，不跟着正文两端对齐\n", align_label(a)));
        }
        if r.table_bib_paragraphs > 0 || r.page_breaks_not_doubled > 0 || !r.own_indent_styles.is_empty() || r.own_indent_paragraphs > 0 || r.bibliography_align_kept.is_some() {
            s.push('\n');
        }
        if !r.remapped.is_empty() {
            s.push_str("## 段落角色映射（排版计划）\n\n");
            for (k, v) in &r.remapped {
                s.push_str(&format!("- {k}：{v} 段\n"));
            }
            s.push('\n');
        }
        if r.protected_paragraphs > 0 {
            s.push_str(&format!("## 受保护区域\n\n- 段落 {} 个保持原样；为抵消样式改动写回原外观 {} 处\n\n", r.protected_paragraphs, r.frozen_properties));
        }
        if let Some(st) = &r.structure {
            s.push_str("## 结构\n\n");
            if !st.zones.is_empty() {
                s.push_str(&format!("- 分区：{}（{}）\n", st.zones.iter().map(|(z, n)| format!("{z} {n} 节")).collect::<Vec<_>>().join("、"), if st.duplex { "双面：页眉页脚分奇偶页" } else { "单面" }));
            }
            if !st.breaks_inserted.is_empty() {
                s.push_str(&format!("- 新加的分节符：{}\n", st.breaks_inserted.join("、")));
            }
            if st.header_sections > 0 {
                s.push_str(&format!("- 页眉：重设了 {} 节（原来的页眉部件留在文档里，没有删）\n", st.header_sections));
            }
            if st.page_number_sections > 0 {
                s.push_str(&format!("- 页码：重设了 {} 节的页脚与页码形式\n", st.page_number_sections));
            }
            if st.body_on_odd_page {
                s.push_str("- 正文从奇数页开始\n");
            }
            if !st.three_line.is_empty() {
                s.push_str(&format!("- 排成三线表：第 {} 个表格（顶线、底线 1.5 磅，栏目线 0.75 磅）\n", st.three_line.iter().map(|n| n.to_string()).collect::<Vec<_>>().join("、")));
            }
            for (n, why) in &st.tables_skipped {
                s.push_str(&format!("- 第 {n} 个表格没有动：{why}\n"));
            }
            if st.footnote_numbering {
                s.push_str("- 脚注编号：改了文档的脚注编号设置\n");
            }
            if st.tight_headings > 0 {
                s.push_str(&format!("- 紧跟上一级标题的标题：{} 个段前改成 0\n", st.tight_headings));
            }
            for n in &st.notes {
                s.push_str(&format!("- ⚠ {n}\n"));
            }
            s.push('\n');
        }
        if !r.untouched.is_empty() {
            s.push_str("## 未处理、原样保留\n\n");
            for (k, v) in &r.untouched {
                s.push_str(&format!("- {k}：{v} 处\n"));
            }
            s.push('\n');
        }
        if !r.notes.is_empty() {
            s.push_str("## 提示\n\n");
            for n in &r.notes {
                s.push_str(&format!("- {n}\n"));
            }
            s.push('\n');
        }
    }
    if !problems.is_empty() {
        s.push_str("## 保真问题\n\n| 部件 | 类型 | 说明 |\n| --- | --- | --- |\n");
        for p in problems {
            s.push_str(&format!("| {} | {} | {} |\n", p.part, p.kind, p.detail.replace('|', "\\|")));
        }
        s.push('\n');
    }
    if !checks.is_empty() {
        s.push_str("## 规范核对\n\n| 项 | 结果 | 说明 |\n| --- | --- | --- |\n");
        for c in checks {
            s.push_str(&format!("| {} | {} | {} |\n", c.item, if c.ok { "✓" } else { "✗" }, c.detail));
        }
    }
    s
}

fn align_label(v: &str) -> &str {
    match v {
        "left" | "start" => "左对齐",
        "both" => "两端对齐",
        "center" => "居中",
        "right" | "end" => "右对齐",
        other => other,
    }
}
