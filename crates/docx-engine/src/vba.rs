//! 从 VBA 源码生成 vbaProject.bin（[MS-OVBA]）。Pith 的 Word 全局模板（.dotm）用它带宏，不需要在 Word 里手工导入。
//!
//! - 只写源码，不写性能缓存：规范要求写入时 `_VBA_PROJECT` 的版本为 0xFFFF、不带缓存，Word 打开时按源码重新编译
//! - 不写任何引用（stdole、Office 库）：宏里 Office 库的类型一律用 `Object`（后期绑定）
//! - 生成结果确定（同样的输入得到同样的字节），设置页据此判断已安装的模板是不是最新的
//! - 源码只能是 ASCII（Mac 版 VBA 编辑器按旧代码页读，中文会乱码；Word 里的文字一律英文，`decisions/0006`）

use std::io::{Cursor, Read, Write};

use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModuleKind {
    /// 标准模块（.bas）
    Standard,
    /// 文档模块（Word 的 ThisDocument）
    Document,
}

#[derive(Clone, Debug)]
pub struct Module {
    pub name: String,
    pub kind: ModuleKind,
    /// 不含 `Attribute VB_Name` 等属性行的代码
    pub code: String,
}

#[derive(Clone, Debug)]
pub struct Project {
    pub name: String,
    pub modules: Vec<Module>,
}

/// Mac Roman（Word for Mac 自己存的工程用它；源码只用 ASCII，与 1252 无差别）
const CODE_PAGE: u16 = 10000;
const LCID: u32 = 0x0409;
/// 64 位 Windows。Word for Mac 照样按源码编译（见 `decisions/0016`）
const SYS_KIND: u32 = 3;

impl Project {
    /// Word 模板的工程：ThisDocument + 给定的标准模块
    pub fn word(name: &str, modules: Vec<Module>) -> Self {
        let mut all = vec![Module { name: "ThisDocument".into(), kind: ModuleKind::Document, code: String::new() }];
        all.extend(modules);
        Project { name: name.to_string(), modules: all }
    }

    fn guid(&self) -> String {
        let mut h = Sha256::new();
        h.update(self.name.as_bytes());
        for m in &self.modules {
            h.update(m.name.as_bytes());
            h.update(m.code.as_bytes());
        }
        let x: String = h.finalize().iter().take(16).map(|b| format!("{b:02X}")).collect();
        format!("{{{}-{}-{}-{}-{}}}", &x[0..8], &x[8..12], &x[12..16], &x[16..20], &x[20..32])
    }
}

fn crlf(code: &str) -> String {
    let mut s = code.replace("\r\n", "\n").replace('\r', "\n").replace('\n', "\r\n");
    if !s.is_empty() && !s.ends_with("\r\n") {
        s.push_str("\r\n");
    }
    s
}

/// 存进模块流的源码：属性行 + 代码，CRLF 换行
pub fn module_source(m: &Module) -> String {
    let head = match m.kind {
        ModuleKind::Standard => format!("Attribute VB_Name = \"{}\"\r\n", m.name),
        // 基类是 Word 的 Document 类（CLSID），与 Word 自己存的一样；写成 "1Normal.ThisDocument" 会找不到工程
        ModuleKind::Document => format!(
            "Attribute VB_Name = \"{}\"\r\nAttribute VB_Base = \"0{{00020906-0000-0000-C000-000000000046}}\"\r\nAttribute VB_GlobalNameSpace = False\r\nAttribute VB_Creatable = False\r\nAttribute VB_PredeclaredId = True\r\nAttribute VB_Exposed = True\r\nAttribute VB_TemplateDerived = False\r\nAttribute VB_Customizable = True\r\n",
            m.name
        ),
    };
    head + &crlf(&m.code)
}

// ---------- 压缩（MS-OVBA 2.4.1） ----------

/// CopyToken 的位数：块内已解压的字节数越多，偏移占的位越多
fn copy_token_bits(difference: usize) -> u32 {
    let mut bits = 0u32;
    while (1usize << bits) < difference {
        bits += 1;
    }
    bits.max(4)
}

fn compress_chunk(chunk: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(chunk.len());
    let mut i = 0;
    while i < chunk.len() {
        let flag_at = out.len();
        out.push(0u8);
        let mut flags = 0u8;
        for bit in 0..8 {
            if i >= chunk.len() {
                break;
            }
            let bits = copy_token_bits(i);
            let max_len = (0xFFFFusize >> bits) + 3;
            let (mut best_len, mut best_off) = (0usize, 0usize);
            for cand in (0..i).rev() {
                let mut len = 0;
                while i + len < chunk.len() && len < max_len && chunk[cand + len] == chunk[i + len] {
                    len += 1;
                }
                if len > best_len {
                    best_len = len;
                    best_off = i - cand;
                    if len == max_len {
                        break;
                    }
                }
            }
            if best_len >= 3 {
                let token = (((best_off - 1) as u16) << (16 - bits)) | ((best_len - 3) as u16);
                out.extend_from_slice(&token.to_le_bytes());
                flags |= 1 << bit;
                i += best_len;
            } else {
                out.push(chunk[i]);
                i += 1;
            }
        }
        out[flag_at] = flags;
    }
    out
}

pub fn compress(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x01u8];
    for chunk in data.chunks(4096) {
        let c = compress_chunk(chunk);
        if c.len() <= 4096 {
            let header = 0xB000u16 | (c.len() as u16 + 2 - 3);
            out.extend_from_slice(&header.to_le_bytes());
            out.extend_from_slice(&c);
        } else {
            // 压不下去的整块原样写（只有满 4096 字节的块会走到这里：不足 3641 字节的块全写字面量也放得下）
            debug_assert_eq!(chunk.len(), 4096);
            out.extend_from_slice(&0x3FFFu16.to_le_bytes());
            out.extend_from_slice(chunk);
        }
    }
    out
}

pub fn decompress(data: &[u8]) -> Result<Vec<u8>, String> {
    if data.first() != Some(&0x01) {
        return Err("压缩容器的签名字节不是 0x01".into());
    }
    let mut out = Vec::new();
    let mut pos = 1;
    while pos < data.len() {
        let header = u16::from_le_bytes([data[pos], *data.get(pos + 1).ok_or("块头不完整")?]);
        if (header >> 12) & 0b111 != 0b011 {
            return Err(format!("第 {pos} 字节：块签名不对"));
        }
        let size = (header & 0x0FFF) as usize + 3;
        let end = (pos + size).min(data.len());
        let body = &data[pos + 2..end];
        let start = out.len();
        if header & 0x8000 == 0 {
            out.extend_from_slice(body);
        } else {
            let mut p = 0;
            while p < body.len() {
                let flags = body[p];
                p += 1;
                for bit in 0..8 {
                    if p >= body.len() {
                        break;
                    }
                    if flags & (1 << bit) == 0 {
                        out.push(body[p]);
                        p += 1;
                    } else {
                        let token = u16::from_le_bytes([body[p], *body.get(p + 1).ok_or("CopyToken 不完整")?]);
                        p += 2;
                        let bits = copy_token_bits(out.len() - start);
                        let len = (token & (0xFFFF >> bits)) as usize + 3;
                        let off = (token >> (16 - bits)) as usize + 1;
                        if off > out.len() - start {
                            return Err("CopyToken 的偏移越过了块开头".into());
                        }
                        for _ in 0..len {
                            out.push(out[out.len() - off]);
                        }
                    }
                }
            }
        }
        pos = end;
    }
    Ok(out)
}

// ---------- 加密（MS-OVBA 2.4.3，用于 PROJECT 流的 CMG / DPB / GC） ----------

fn encrypt(project_id: &str, seed: u8, data: &[u8]) -> String {
    const VERSION: u8 = 2;
    let key = project_id.bytes().fold(0u8, |a, b| a.wrapping_add(b));
    let mut out = vec![seed, seed ^ VERSION, seed ^ key];
    let (mut unenc1, mut enc1, mut enc2) = (key, seed ^ key, seed ^ VERSION);
    let mut push = |byte: u8, out: &mut Vec<u8>| {
        let e = byte ^ enc2.wrapping_add(unenc1);
        out.push(e);
        enc2 = enc1;
        enc1 = e;
        unenc1 = byte;
    };
    for _ in 0..(seed & 6) / 2 {
        push(0, &mut out);
    }
    for b in (data.len() as u32).to_le_bytes().iter().chain(data) {
        push(*b, &mut out);
    }
    out.iter().map(|b| format!("{b:02X}")).collect()
}

/// 解密（测试用）：返回 Data
pub fn decrypt(hex: &str) -> Result<Vec<u8>, String> {
    let bytes: Vec<u8> = (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|e| e.to_string())).collect::<Result<_, _>>()?;
    let [seed, version_enc, key_enc, ..] = bytes[..] else { return Err("太短".into()) };
    if seed ^ version_enc != 2 {
        return Err("版本不是 2".into());
    }
    let key = seed ^ key_enc;
    let (mut unenc1, mut enc1, mut enc2) = (key, key_enc, version_enc);
    let mut plain = Vec::new();
    for &e in &bytes[3..] {
        let b = e ^ enc2.wrapping_add(unenc1);
        plain.push(b);
        enc2 = enc1;
        enc1 = e;
        unenc1 = b;
    }
    let skip = ((seed & 6) / 2) as usize;
    let len = u32::from_le_bytes(plain.get(skip..skip + 4).ok_or("缺长度")?.try_into().map_err(|_| "长度")?) as usize;
    plain.get(skip + 4..skip + 4 + len).map(<[u8]>::to_vec).ok_or("数据不够长".into())
}

// ---------- 各个流 ----------

fn rec(out: &mut Vec<u8>, id: u16, data: &[u8]) {
    out.extend_from_slice(&id.to_le_bytes());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(data);
}

fn utf16(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

/// dir 流（压缩前）：工程信息、引用（无）、模块列表
fn dir_stream(p: &Project) -> Vec<u8> {
    let mut d = Vec::new();
    rec(&mut d, 0x0001, &SYS_KIND.to_le_bytes());
    rec(&mut d, 0x0002, &LCID.to_le_bytes());
    rec(&mut d, 0x0014, &LCID.to_le_bytes());
    rec(&mut d, 0x0003, &CODE_PAGE.to_le_bytes());
    rec(&mut d, 0x0004, p.name.as_bytes());
    rec(&mut d, 0x0005, b"");
    rec(&mut d, 0x0040, b"");
    rec(&mut d, 0x0006, b"");
    rec(&mut d, 0x003D, b"");
    rec(&mut d, 0x0007, &0u32.to_le_bytes());
    rec(&mut d, 0x0008, &0u32.to_le_bytes());
    // PROJECTVERSION：Reserved 固定为 4，后面是 4 字节主版本 + 2 字节次版本
    d.extend_from_slice(&0x0009u16.to_le_bytes());
    d.extend_from_slice(&4u32.to_le_bytes());
    d.extend_from_slice(&1u32.to_le_bytes());
    d.extend_from_slice(&0u16.to_le_bytes());
    rec(&mut d, 0x000C, b"");
    rec(&mut d, 0x003C, b"");
    // 模块
    rec(&mut d, 0x000F, &(p.modules.len() as u16).to_le_bytes());
    rec(&mut d, 0x0013, &0xFFFFu16.to_le_bytes());
    for m in &p.modules {
        rec(&mut d, 0x0019, m.name.as_bytes());
        rec(&mut d, 0x0047, &utf16(&m.name));
        rec(&mut d, 0x001A, m.name.as_bytes());
        rec(&mut d, 0x0032, &utf16(&m.name));
        rec(&mut d, 0x001C, b"");
        rec(&mut d, 0x0048, b"");
        rec(&mut d, 0x0031, &0u32.to_le_bytes());
        rec(&mut d, 0x001E, &0u32.to_le_bytes());
        rec(&mut d, 0x002C, &0xFFFFu16.to_le_bytes());
        rec(&mut d, if m.kind == ModuleKind::Document { 0x0022 } else { 0x0021 }, b"");
        rec(&mut d, 0x002B, b"");
    }
    rec(&mut d, 0x0010, b"");
    d
}

fn project_stream(p: &Project) -> String {
    let id = p.guid();
    let mut s = format!("ID=\"{id}\"\r\n");
    for m in &p.modules {
        match m.kind {
            ModuleKind::Document => s.push_str(&format!("Document={}/&H00000000\r\n", m.name)),
            ModuleKind::Standard => s.push_str(&format!("Module={}\r\n", m.name)),
        }
    }
    s.push_str(&format!("Name=\"{}\"\r\nHelpContextID=\"0\"\r\nVersionCompatible32=\"393222000\"\r\n", p.name));
    // 不保护、无密码、可见
    s.push_str(&format!("CMG=\"{}\"\r\n", encrypt(&id, 0x31, &[0, 0, 0, 0])));
    s.push_str(&format!("DPB=\"{}\"\r\n", encrypt(&id, 0x59, &[0])));
    s.push_str(&format!("GC=\"{}\"\r\n", encrypt(&id, 0xA8, &[0xFF])));
    s.push_str("\r\n[Host Extender Info]\r\n&H00000001={3832D640-CF90-11CF-8E43-00A0C911005A};VBE;&H00000000\r\n");
    s
}

fn project_wm(p: &Project) -> Vec<u8> {
    let mut out = Vec::new();
    for m in &p.modules {
        out.extend_from_slice(m.name.as_bytes());
        out.push(0);
        out.extend_from_slice(&utf16(&m.name));
        out.extend_from_slice(&[0, 0]);
    }
    out.extend_from_slice(&[0, 0]);
    out
}

pub fn build(p: &Project) -> Result<Vec<u8>, String> {
    let ascii = |s: &str| s.is_ascii() && !s.is_empty();
    if !ascii(&p.name) || p.modules.iter().any(|m| !ascii(&m.name) || !m.code.is_ascii()) {
        return Err("VBA 工程名、模块名与源码只能是 ASCII".into());
    }
    let io = |e: std::io::Error| format!("写 vbaProject.bin 失败：{e}");
    let mut cf = cfb::CompoundFile::create_with_version(cfb::Version::V3, Cursor::new(Vec::new())).map_err(io)?;
    let put = |cf: &mut cfb::CompoundFile<Cursor<Vec<u8>>>, path: &str, data: &[u8]| -> Result<(), String> {
        let mut s = cf.create_stream(path).map_err(io)?;
        s.write_all(data).map_err(io)
    };
    cf.create_storage("/VBA").map_err(io)?;
    put(&mut cf, "/PROJECT", project_stream(p).as_bytes())?;
    put(&mut cf, "/PROJECTwm", &project_wm(p))?;
    put(&mut cf, "/VBA/_VBA_PROJECT", &[0xCC, 0x61, 0xFF, 0xFF, 0x00, 0x00, 0x00])?;
    put(&mut cf, "/VBA/dir", &compress(&dir_stream(p)))?;
    for m in &p.modules {
        put(&mut cf, &format!("/VBA/{}", m.name), &compress(module_source(m).as_bytes()))?;
    }
    // 存储的时间戳固定下来：同样的输入得到同样的字节
    for path in ["/", "/VBA"] {
        cf.set_created_time(path, std::time::UNIX_EPOCH).map_err(io)?;
        cf.set_modified_time(path, std::time::UNIX_EPOCH).map_err(io)?;
    }
    cf.flush().map_err(io)?;
    Ok(cf.into_inner().into_inner())
}

/// 读回（测试与自检用）：PROJECT 流的文字、dir 流里的模块（名称、类型）、各模块源码
pub struct ReadBack {
    pub project: String,
    pub modules: Vec<(String, bool, String)>,
}

pub fn read_back(bytes: &[u8]) -> Result<ReadBack, String> {
    let mut cf = cfb::CompoundFile::open(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let mut read = |path: &str| -> Result<Vec<u8>, String> {
        let mut s = cf.open_stream(path).map_err(|e| format!("{path}：{e}"))?;
        let mut v = Vec::new();
        s.read_to_end(&mut v).map_err(|e| e.to_string())?;
        Ok(v)
    };
    let project = String::from_utf8(read("/PROJECT")?).map_err(|e| e.to_string())?;
    if read("/VBA/_VBA_PROJECT")? != [0xCC, 0x61, 0xFF, 0xFF, 0x00, 0x00, 0x00] {
        return Err("_VBA_PROJECT 不对".into());
    }
    let dir = decompress(&read("/VBA/dir")?)?;
    // 逐条读记录；PROJECTVERSION 的 Size 字段不是真实长度
    let mut pos = 0;
    let mut modules = Vec::new();
    let mut current: Option<(String, bool)> = None;
    let mut stream = String::new();
    while pos + 6 <= dir.len() {
        let id = u16::from_le_bytes([dir[pos], dir[pos + 1]]);
        let size = u32::from_le_bytes(dir[pos + 2..pos + 6].try_into().unwrap()) as usize;
        let size = if id == 0x0009 { 6 } else { size };
        let data = dir.get(pos + 6..pos + 6 + size).ok_or("dir 流截断")?;
        match id {
            0x0019 => current = Some((String::from_utf8_lossy(data).into_owned(), false)),
            0x001A => stream = String::from_utf8_lossy(data).into_owned(),
            0x0022 => {
                if let Some(c) = current.as_mut() {
                    c.1 = true;
                }
            }
            0x0031 if data != [0, 0, 0, 0] => return Err("模块偏移应为 0（不带缓存）".into()),
            0x002B => {
                let (name, doc) = current.take().ok_or("模块结束标记前没有模块名")?;
                let src = decompress(&read(&format!("/VBA/{stream}"))?)?;
                modules.push((name, doc, String::from_utf8(src).map_err(|e| e.to_string())?));
            }
            0x0010 => break,
            _ => {}
        }
        pos += 6 + size;
    }
    Ok(ReadBack { project, modules })
}

/// 调试用：列出 PROJECT 流、dir 流的全部记录、各存储与流（对照 Word 自己存的工程）
pub fn dump(bytes: &[u8]) -> Result<String, String> {
    let mut cf = cfb::CompoundFile::open(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let mut out = String::new();
    let entries: Vec<(String, u64)> = cf.walk().map(|e| (e.path().display().to_string(), e.len())).collect();
    for (p, len) in &entries {
        out.push_str(&format!("entry {p} ({len} bytes)\n"));
    }
    let mut read = |path: &str| -> Result<Vec<u8>, String> {
        let mut s = cf.open_stream(path).map_err(|e| format!("{path}：{e}"))?;
        let mut v = Vec::new();
        s.read_to_end(&mut v).map_err(|e| e.to_string())?;
        Ok(v)
    };
    out.push_str(&format!("--- PROJECT\n{}\n", String::from_utf8_lossy(&read("/PROJECT")?)));
    let vba = read("/VBA/_VBA_PROJECT")?;
    out.push_str(&format!("--- _VBA_PROJECT {} bytes, head {:02X?}\n", vba.len(), &vba[..vba.len().min(8)]));
    let dir = decompress(&read("/VBA/dir")?)?;
    out.push_str("--- dir\n");
    let (mut stream, mut offset, mut modules) = (String::new(), 0usize, Vec::new());
    let mut pos = 0;
    while pos + 6 <= dir.len() {
        let id = u16::from_le_bytes([dir[pos], dir[pos + 1]]);
        let size = u32::from_le_bytes(dir[pos + 2..pos + 6].try_into().unwrap()) as usize;
        let size = if id == 0x0009 { 6 } else { size };
        let data = &dir[pos + 6..(pos + 6 + size).min(dir.len())];
        let text: String = data.iter().map(|&b| if (0x20..0x7f).contains(&b) { b as char } else { '.' }).collect();
        out.push_str(&format!("{id:04X} [{size}] {:02X?} {text}\n", &data[..data.len().min(24)]));
        match id {
            0x001A => stream = String::from_utf8_lossy(data).into_owned(),
            0x0031 => offset = u32::from_le_bytes(data.try_into().unwrap_or([0; 4])) as usize,
            0x002B => modules.push((stream.clone(), offset)),
            _ => {}
        }
        pos += 6 + size;
        if id == 0x0010 {
            break;
        }
    }
    for (name, offset) in modules {
        let raw = read(&format!("/VBA/{name}"))?;
        let src = decompress(raw.get(offset..).unwrap_or(&[]))?;
        let text = String::from_utf8_lossy(&src);
        out.push_str(&format!("--- module {name} (offset {offset})\n{}\n", text.chars().take(1500).collect::<String>()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compression_round_trips() {
        let text = "Public Sub Hello()\r\n    MsgBox \"Hello\"\r\nEnd Sub\r\n".repeat(400);
        for data in [Vec::new(), b"a".to_vec(), b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_vec(), text.into_bytes(), (0..20000u32).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8).collect()] {
            let c = compress(&data);
            assert_eq!(decompress(&c).unwrap(), data);
            if data.len() > 1000 && data[0] == b'P' {
                assert!(c.len() < data.len() / 5, "重复的文字应当压得很小：{} → {}", data.len(), c.len());
            }
        }
    }

    /// 规范里的例子（MS-OVBA 3.2.2）：一段有重复的文字
    #[test]
    fn spec_example_decompresses() {
        let compressed = [
            0x01, 0x2F, 0xB0, 0x00, 0x23, 0x61, 0x61, 0x61, 0x62, 0x63, 0x64, 0x65, 0x82, 0x66, 0x00, 0x70, 0x61, 0x67, 0x68, 0x69, 0x6A, 0x01, 0x38, 0x08, 0x61, 0x6B, 0x6C, 0x00, 0x30, 0x6D, 0x6E, 0x6F, 0x70, 0x06, 0x71, 0x02, 0x70, 0x04, 0x10, 0x72, 0x73, 0x74, 0x75, 0x76, 0x10, 0x77, 0x78, 0x79, 0x7A, 0x00, 0x3C,
        ];
        assert_eq!(decompress(&compressed).unwrap(), b"#aaabcdefaaaaghijaaaaaklaaamnopqaaaaaaaaaaaarstuvwxyzaaa");
        let ours = compress(b"#aaabcdefaaaaghijaaaaaklaaamnopqaaaaaaaaaaaarstuvwxyzaaa");
        assert_eq!(decompress(&ours).unwrap(), b"#aaabcdefaaaaghijaaaaaklaaamnopqaaaaaaaaaaaarstuvwxyzaaa");
    }

    #[test]
    fn encryption_round_trips() {
        for seed in [0x00, 0x06, 0x31, 0x59, 0xA8, 0xFF] {
            for data in [vec![0u8, 0, 0, 0], vec![0], vec![0xFF]] {
                let hex = encrypt("{01234567-89AB-CDEF-0123-456789ABCDEF}", seed, &data);
                assert_eq!(decrypt(&hex).unwrap(), data);
            }
        }
        // 长度符合 PROJECT 流的语法：CMG 22–28 位十六进制，DPB / GC 16–22 位
        let id = "{00000000-0000-0000-0000-000000000000}";
        assert!((22..=28).contains(&encrypt(id, 0x31, &[0; 4]).len()));
        assert!((16..=22).contains(&encrypt(id, 0xA8, &[0xFF]).len()));
    }

    fn sample() -> Project {
        Project::word("Pith", vec![Module { name: "PithMain".into(), kind: ModuleKind::Standard, code: "Public Sub PithHello(ByVal control As Object)\n    MsgBox \"Hello from Pith\"\nEnd Sub".into() }])
    }

    #[test]
    fn project_reads_back() {
        let p = sample();
        let bin = build(&p).unwrap();
        assert_eq!(build(&p).unwrap(), bin, "同样的输入得到同样的字节");
        let r = read_back(&bin).unwrap();
        assert!(r.project.starts_with("ID=\"{") && r.project.contains("Document=ThisDocument/&H00000000\r\nModule=PithMain\r\nName=\"Pith\"\r\n"));
        let line = |k: &str| r.project.lines().find(|l| l.starts_with(k)).unwrap().split('"').nth(1).unwrap().to_string();
        assert_eq!(decrypt(&line("CMG=")).unwrap(), [0, 0, 0, 0]);
        assert_eq!(decrypt(&line("DPB=")).unwrap(), [0]);
        assert_eq!(decrypt(&line("GC=")).unwrap(), [0xFF]);
        assert_eq!(r.modules.len(), 2);
        assert_eq!((r.modules[0].0.as_str(), r.modules[0].1), ("ThisDocument", true));
        assert!(r.modules[0].2.contains("Attribute VB_Base = \"0{00020906-0000-0000-C000-000000000046}\"\r\n"));
        assert_eq!((r.modules[1].0.as_str(), r.modules[1].1), ("PithMain", false));
        assert_eq!(r.modules[1].2, "Attribute VB_Name = \"PithMain\"\r\nPublic Sub PithHello(ByVal control As Object)\r\n    MsgBox \"Hello from Pith\"\r\nEnd Sub\r\n");
    }

    #[test]
    fn rejects_non_ascii_source() {
        let mut p = sample();
        p.modules[1].code.push_str("' 中文注释");
        assert!(build(&p).is_err());
    }
}
