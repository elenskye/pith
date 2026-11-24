//! 极简可编辑 XML 树：基于 quick-xml 事件，保留原始写法。
//! 没被改过的开始标签、文本、注释按原字节写回；只有被改动的元素才重新生成标签。
//! 名字按「前缀:本地名」存放，命名空间前缀从根元素的声明解析（OOXML 惯例全部声明在根上）。

use std::collections::HashMap;

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::{Error, Result};

#[derive(Clone, Debug)]
pub enum Node {
    El(El),
    /// 原样保留的文本、实体引用、注释、CDATA、处理指令
    Raw(String),
}

#[derive(Clone, Debug)]
pub struct El {
    pub name: String,
    /// 属性：(限定名, 已转义的原始值)
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
    /// 原始开始标签（含 `<` `>`）；属性被改后清空
    raw_start: Option<String>,
    /// 原来是 `<x/>` 形式
    was_empty: bool,
}

#[derive(Clone, Debug)]
pub struct XmlDoc {
    pub prolog: String,
    pub root: El,
    pub epilog: String,
    /// 前缀 → 命名空间 URI（来自根元素）
    pub ns: HashMap<String, String>,
}

impl El {
    pub fn new(name: impl Into<String>) -> Self {
        El { name: name.into(), attrs: Vec::new(), children: Vec::new(), raw_start: None, was_empty: true }
    }

    pub fn with_attr(mut self, k: &str, v: &str) -> Self {
        self.set_attr(k, v);
        self
    }

    pub fn prefix(&self) -> &str {
        self.name.split_once(':').map(|(p, _)| p).unwrap_or("")
    }

    pub fn local(&self) -> &str {
        self.name.split_once(':').map(|(_, l)| l).unwrap_or(&self.name)
    }

    pub fn attr(&self, k: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }

    /// 设置属性（v 为未转义的值）。
    pub fn set_attr(&mut self, k: &str, v: &str) {
        let v = escape_attr(v);
        if let Some(a) = self.attrs.iter_mut().find(|(n, _)| n == k) {
            if a.1 == v {
                return;
            }
            a.1 = v;
        } else {
            self.attrs.push((k.to_string(), v));
        }
        self.raw_start = None;
    }

    pub fn remove_attr(&mut self, k: &str) -> bool {
        let before = self.attrs.len();
        self.attrs.retain(|(n, _)| n != k);
        let removed = self.attrs.len() != before;
        if removed {
            self.raw_start = None;
        }
        removed
    }

    pub fn remove_attrs_where(&mut self, pred: impl Fn(&str) -> bool) -> usize {
        let before = self.attrs.len();
        self.attrs.retain(|(n, _)| !pred(n));
        let n = before - self.attrs.len();
        if n > 0 {
            self.raw_start = None;
        }
        n
    }

    pub fn els(&self) -> impl Iterator<Item = &El> {
        self.children.iter().filter_map(|n| if let Node::El(e) = n { Some(e) } else { None })
    }

    pub fn els_mut(&mut self) -> impl Iterator<Item = &mut El> {
        self.children.iter_mut().filter_map(|n| if let Node::El(e) = n { Some(e) } else { None })
    }

    pub fn child(&self, name: &str) -> Option<&El> {
        self.els().find(|e| e.name == name)
    }

    pub fn child_mut(&mut self, name: &str) -> Option<&mut El> {
        self.els_mut().find(|e| e.name == name)
    }

    /// 删除所有名为 name 的子元素，返回删除个数。
    pub fn remove_children(&mut self, name: &str) -> usize {
        let before = self.children.len();
        self.children.retain(|n| !matches!(n, Node::El(e) if e.name == name));
        before - self.children.len()
    }

    /// 按 schema 顺序插入子元素（同名旧元素先删除）。order 为本地名顺序表，prefix 为该命名空间前缀。
    pub fn put_child(&mut self, new: El, prefix: &str, order: &[&str]) {
        self.remove_children(&new.name);
        let rank = |e: &El| -> usize {
            if e.prefix() == prefix {
                order.iter().position(|o| *o == e.local()).unwrap_or(order.len())
            } else {
                order.len()
            }
        };
        let new_rank = rank(&new);
        let pos = self
            .children
            .iter()
            .position(|n| matches!(n, Node::El(e) if rank(e) > new_rank))
            .unwrap_or(self.children.len());
        self.children.insert(pos, Node::El(new));
    }

    /// 取已有子元素；没有就按 schema 顺序插入一个空的。
    pub fn ensure_child(&mut self, name: &str, prefix: &str, order: &[&str]) -> &mut El {
        if self.child(name).is_none() {
            self.put_child(El::new(name), prefix, order);
        }
        self.child_mut(name).expect("刚插入的子元素")
    }

    pub fn has_descendant(&self, pred: &dyn Fn(&El) -> bool) -> bool {
        self.els().any(|e| pred(e) || e.has_descendant(pred))
    }

    fn write(&self, out: &mut String) {
        let empty_now = self.children.is_empty();
        match (&self.raw_start, self.was_empty, empty_now) {
            (Some(raw), true, true) => out.push_str(raw),
            (Some(raw), false, _) => {
                out.push_str(raw);
                for c in &self.children {
                    write_node(c, out);
                }
                out.push_str("</");
                out.push_str(&self.name);
                out.push('>');
            }
            _ => {
                out.push('<');
                out.push_str(&self.name);
                for (k, v) in &self.attrs {
                    out.push(' ');
                    out.push_str(k);
                    out.push_str("=\"");
                    out.push_str(v);
                    out.push('"');
                }
                if empty_now {
                    out.push_str("/>");
                } else {
                    out.push('>');
                    for c in &self.children {
                        write_node(c, out);
                    }
                    out.push_str("</");
                    out.push_str(&self.name);
                    out.push('>');
                }
            }
        }
    }
}

fn write_node(n: &Node, out: &mut String) {
    match n {
        Node::El(e) => e.write(out),
        Node::Raw(s) => out.push_str(s),
    }
}

pub fn escape_attr(v: &str) -> String {
    v.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// 反转义 XML 文字或属性值（预定义实体与字符引用；认不出的实体原样保留）。
pub fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let Some(end) = tail.find(';') else {
            out.push_str(tail);
            return out;
        };
        let ent = &tail[1..end];
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            e if e.starts_with("#x") => u32::from_str_radix(&e[2..], 16).ok().and_then(char::from_u32),
            e if e.starts_with('#') => e[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match ch {
            Some(c) => out.push(c),
            None => out.push_str(&tail[..=end]),
        }
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    out
}

pub fn escape_text(v: &str) -> String {
    v.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

pub fn parse(src: &str) -> Result<XmlDoc> {
    let mut reader = Reader::from_str(src);
    reader.config_mut().trim_text(false);
    let mut stack: Vec<El> = Vec::new();
    let mut prolog = String::new();
    let mut root: Option<El> = None;
    let mut epilog = String::new();
    loop {
        let start = reader.buffer_position() as usize;
        let ev = reader.read_event().map_err(|e| Error::Xml(format!("第 {start} 字节附近：{e}")))?;
        let end = reader.buffer_position() as usize;
        let raw = &src[start..end];
        match ev {
            Event::Start(e) | Event::Empty(e) => {
                let is_empty = raw.ends_with("/>");
                let mut el = El {
                    name: e.name().as_ref().to_string(),
                    attrs: Vec::new(),
                    children: Vec::new(),
                    raw_start: Some(raw.to_string()),
                    was_empty: is_empty,
                };
                for a in e.attributes().with_checks(false) {
                    let a = a.map_err(|e| Error::Xml(e.to_string()))?;
                    el.attrs.push((a.key.as_ref().to_string(), a.value.into_owned()));
                }
                if is_empty {
                    match stack.last_mut() {
                        Some(p) => p.children.push(Node::El(el)),
                        None => root = Some(el),
                    }
                } else {
                    stack.push(el);
                }
            }
            Event::End(_) => {
                let el = stack.pop().ok_or_else(|| Error::Xml("多余的结束标签".into()))?;
                match stack.last_mut() {
                    Some(p) => p.children.push(Node::El(el)),
                    None => root = Some(el),
                }
            }
            Event::Eof => break,
            _ => match stack.last_mut() {
                Some(p) => p.children.push(Node::Raw(raw.to_string())),
                None if root.is_none() => prolog.push_str(raw),
                None => epilog.push_str(raw),
            },
        }
    }
    if !stack.is_empty() {
        return Err(Error::Xml("元素未闭合".into()));
    }
    let root = root.ok_or_else(|| Error::Xml("没有根元素".into()))?;
    let mut ns = HashMap::new();
    for (k, v) in &root.attrs {
        if let Some(p) = k.strip_prefix("xmlns:") {
            ns.insert(p.to_string(), v.clone());
        } else if k == "xmlns" {
            ns.insert(String::new(), v.clone());
        }
    }
    Ok(XmlDoc { prolog, root, epilog, ns })
}

impl XmlDoc {
    pub fn to_xml(&self) -> String {
        let mut out = String::with_capacity(4096);
        out.push_str(&self.prolog);
        self.root.write(&mut out);
        out.push_str(&self.epilog);
        out
    }

    /// 命名空间 URI 对应的前缀（找不到时返回 None）。
    pub fn prefix_of(&self, uri: &str) -> Option<String> {
        self.ns.iter().find(|(_, u)| u.as_str() == uri).map(|(p, _)| p.clone())
    }
}
