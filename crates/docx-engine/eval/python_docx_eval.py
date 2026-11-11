"""评估 python-docx 1.2：读入再写出的保真度，以及用它实现同一份排版规范要绕多少路。

用法：python eval/python_docx_eval.py <输入.docx> <输出目录>
输出：<输出目录>/python-docx-roundtrip-<名>、python-docx-format-<名>，再用 docx-engine verify 比较。
"""

import json
import sys
from pathlib import Path

from docx import Document
from docx.enum.text import WD_ALIGN_PARAGRAPH
from docx.oxml.ns import qn
from docx.shared import Cm, Pt, RGBColor

SPEC = json.loads((Path(__file__).parent.parent / "spec" / "thesis-sample.json").read_text(encoding="utf-8"))
ALIGN = {"both": WD_ALIGN_PARAGRAPH.JUSTIFY, "center": WD_ALIGN_PARAGRAPH.CENTER, "left": WD_ALIGN_PARAGRAPH.LEFT, "right": WD_ALIGN_PARAGRAPH.RIGHT}
WORKAROUNDS = []  # 记录 API 不支持、只能直接改 XML 的地方


def workaround(what):
    if what not in WORKAROUNDS:
        WORKAROUNDS.append(what)


def apply_style(style, ts):
    f = style.font
    if ts.get("font_latin"):
        f.name = ts["font_latin"]
    rfonts = style.element.get_or_add_rPr().get_or_add_rFonts()
    if ts.get("font_east_asia"):
        rfonts.set(qn("w:eastAsia"), ts["font_east_asia"])
        workaround("中文字体（w:eastAsia）没有 API")
    for a in ("w:asciiTheme", "w:hAnsiTheme", "w:eastAsiaTheme", "w:cstheme"):
        if rfonts.get(qn(a)) is not None:
            del rfonts.attrib[qn(a)]
            workaround("去掉主题字体属性没有 API")
    if ts.get("size_pt"):
        f.size = Pt(ts["size_pt"])
    if ts.get("bold") is not None:
        f.bold = ts["bold"]
    if ts.get("color"):
        f.color.rgb = RGBColor.from_string(ts["color"])
    pf = style.paragraph_format
    if ts.get("align"):
        pf.alignment = ALIGN[ts["align"]]
    if ts.get("line_spacing") is not None:
        pf.line_spacing = ts["line_spacing"]
    if ts.get("space_before_pt") is not None:
        pf.space_before = Pt(ts["space_before_pt"])
    if ts.get("space_after_pt") is not None:
        pf.space_after = Pt(ts["space_after_pt"])
    if ts.get("first_line_indent_chars") is not None:
        c = ts["first_line_indent_chars"]
        pf.first_line_indent = Pt(c * (ts.get("size_pt") or SPEC["body"]["size_pt"]))
        ind = style.element.get_or_add_pPr().find(qn("w:ind"))
        if ind is not None:
            ind.set(qn("w:firstLineChars"), str(int(c * 100)))
        workaround("按字符的首行缩进（firstLineChars）没有 API")


def style_by_name(doc, name):
    for s in doc.styles:
        if s.name.lower() == name.lower():
            return s
    return None


def strip_runs(paragraphs):
    n = 0
    for p in paragraphs:
        sid = (p.style.name or "").lower() if p.style is not None else ""
        if "code" in sid or "代码" in sid:
            continue
        for r in p.runs:
            if r.font.name or r.font.size:
                r.font.name = None
                r.font.size = None
                rpr = r._element.rPr
                if rpr is not None and rpr.rFonts is not None:
                    rpr.remove(rpr.rFonts)
                n += 1
    return n


def format_doc(doc):
    for s in doc.sections:
        portrait = s.page_width < s.page_height
        s.page_width, s.page_height = (Cm(21), Cm(29.7)) if portrait else (Cm(29.7), Cm(21))
        m = SPEC["page"]["margins_cm"]
        s.top_margin, s.bottom_margin, s.left_margin, s.right_margin = Cm(m["top"]), Cm(m["bottom"]), Cm(m["left"]), Cm(m["right"])
        s.header_distance, s.footer_distance = Cm(SPEC["page"]["header_cm"]), Cm(SPEC["page"]["footer_cm"])
    apply_style(style_by_name(doc, "Normal"), SPEC["body"])
    for h in SPEC["headings"]:
        st = style_by_name(doc, f"Heading {h['level']}")
        if st is not None:
            apply_style(st, h)
    n = strip_runs(doc.paragraphs)
    for t in doc.tables:
        for row in t.rows:
            for cell in row.cells:
                n += strip_runs(cell.paragraphs)
    workaround("脚注、尾注没有 API（python-docx 1.2 不读写 footnotes.xml），其中的直接格式无法清理")
    workaround("内容控件（sdt）里的段落不在 doc.paragraphs 中，需要直接遍历 XML")
    return n


def main():
    src, out = Path(sys.argv[1]), Path(sys.argv[2])
    out.mkdir(parents=True, exist_ok=True)
    Document(src).save(out / f"python-docx-roundtrip-{src.name}")
    doc = Document(src)
    n = format_doc(doc)
    doc.save(out / f"python-docx-format-{src.name}")
    print(json.dumps({"input": src.name, "runs_stripped": n, "workarounds": WORKAROUNDS}, ensure_ascii=False, indent=1))


if __name__ == "__main__":
    main()
