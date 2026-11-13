//! API 密钥放哪（`decisions/0004`、`0018`、`0020`）。
//!
//! - 正式版（「应用程序」里的 Pith，正式数据目录）：存 macOS 钥匙串，每把密钥一个条目（`ApiKey::account`）。
//!   钥匙串按 App 的签名认人，所以正式版用固定证书签名（`scripts/install-app.sh`）。
//! - 开发版、指定了数据目录的打包版、测试：不碰钥匙串。密钥只在内存里（退出就没了），
//!   或由 `PITH_API_KEY_FILE` 指向的文件给（真实验收用；一个文件可以放几把，按形状认）。
//!
//! 密钥不写进数据库、日志、报错和任何文件。

use std::sync::Mutex;

pub const KEY_FILE_ENV: &str = "PITH_API_KEY_FILE";

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Place {
    Keychain,
    Memory,
    File,
}

/// 哪一把密钥。v0.2 排版 Agent 只用 Anthropic；v0.3 事实调查加 DeepSeek（便宜的执行模型）和 Tavily（搜索）；
/// v0.4 概念研究加 OpenAI（GPT-6.1 Sol）和 Mathpix（PDF 转文字，`app_id:app_key` 一对存在一个条目里）
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiKey {
    Anthropic,
    Deepseek,
    Tavily,
    Openai,
    Mathpix,
}

impl ApiKey {
    /// 钥匙串里的条目名（服务名都是 Pith）。Anthropic 的沿用 v0.2 的名字，已存的密钥不用重填
    pub fn account(self) -> &'static str {
        match self {
            ApiKey::Anthropic => "anthropic-api-key",
            ApiKey::Deepseek => "deepseek-api-key",
            ApiKey::Tavily => "tavily-api-key",
            ApiKey::Openai => "openai-api-key",
            ApiKey::Mathpix => "mathpix-api-key",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ApiKey::Anthropic => "Anthropic",
            ApiKey::Deepseek => "DeepSeek",
            ApiKey::Tavily => "Tavily",
            ApiKey::Openai => "OpenAI",
            ApiKey::Mathpix => "Mathpix",
        }
    }

    /// 密钥文件里「名字=密钥」的名字（不分大小写，含这个词就算：OPENAI_API_KEY=…、openai=…）
    fn names(self) -> &'static [&'static str] {
        match self {
            ApiKey::Anthropic => &["anthropic", "claude"],
            ApiKey::Deepseek => &["deepseek"],
            ApiKey::Tavily => &["tavily"],
            ApiKey::Openai => &["openai"],
            ApiKey::Mathpix => &["mathpix"],
        }
    }

    pub const ALL: [ApiKey; 5] = [ApiKey::Anthropic, ApiKey::Deepseek, ApiKey::Tavily, ApiKey::Openai, ApiKey::Mathpix];

    /// 形状对不对（不联网）。DeepSeek 的也以 sk- 开头，所以要排除 Anthropic 的 sk-ant-；
    /// OpenAI 的也是 sk- 开头（sk-proj-…），和 DeepSeek 按形状分不开，密钥文件里要写名字
    fn shape_ok(self, k: &str) -> bool {
        if self == ApiKey::Mathpix {
            // app_id:app_key
            let Some((id, key)) = k.split_once(':') else { return false };
            let ok = |s: &str, min: usize, max: usize| (min..=max).contains(&s.len()) && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
            return ok(id, 3, 120) && ok(key, 16, 200);
        }
        let chars_ok = k.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        let len = k.len();
        chars_ok
            && match self {
                ApiKey::Anthropic => k.starts_with("sk-ant-") && (40..=300).contains(&len),
                ApiKey::Deepseek => k.starts_with("sk-") && !k.starts_with("sk-ant-") && (20..=200).contains(&len),
                ApiKey::Tavily => k.starts_with("tvly-") && (20..=200).contains(&len),
                ApiKey::Openai => k.starts_with("sk-") && !k.starts_with("sk-ant-") && (20..=400).contains(&len),
                ApiKey::Mathpix => false,
            }
    }

    /// 只做形状检查（不联网）；报错里不带输入的内容
    pub fn check(self, key: &str) -> Result<String, String> {
        let k = key.trim();
        if self.shape_ok(k) {
            return Ok(k.to_string());
        }
        Err(match self {
            ApiKey::Anthropic => "这看起来不是 Anthropic 的 API 密钥（应以 sk-ant- 开头）。到 console.anthropic.com 的 API Keys 里复制完整的一串。",
            ApiKey::Deepseek => "这看起来不是 DeepSeek 的 API 密钥（应以 sk- 开头）。到 platform.deepseek.com 的 API keys 里复制完整的一串。",
            ApiKey::Tavily => "这看起来不是 Tavily 的 API 密钥（应以 tvly- 开头）。到 app.tavily.com 复制完整的一串。",
            ApiKey::Openai => "这看起来不是 OpenAI 的 API 密钥（应以 sk- 开头）。到 platform.openai.com 的 API keys 里复制完整的一串。",
            ApiKey::Mathpix => "这看起来不是 Mathpix 的 app_id 和 app_key。到 console.mathpix.com 复制两项，分别填进两个框。",
        }
        .into())
    }

    /// 从密钥文件的文字里找这把密钥。每行写「名字=密钥」（`openai=sk-…`、`OPENAI_API_KEY="sk-…"`、`mathpix=app_id:app_key`）；
    /// 写了名字的只给那一把。旧写法（一行只有密钥，按形状认）照认，但 OpenAI 和 Mathpix 必须写名字：
    /// OpenAI 和 DeepSeek 都是 sk- 开头，分不开（没写名字的 sk- 照旧当成 DeepSeek 的）
    pub fn find_in(self, text: &str) -> Option<String> {
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
            let (name, value) = match line.split_once('=') {
                Some((n, v)) => (Some(n.trim().trim_start_matches("export ").trim().to_ascii_lowercase()), v.trim()),
                None => (None, line),
            };
            let value = value.trim_matches(['"', '\'']).trim();
            // 名字不是哪一家的（「KEY=…」）：当成没写名字，照旧按形状认
            let name = name.filter(|n| ApiKey::ALL.iter().any(|k| k.names().iter().any(|w| n.contains(w))));
            match name {
                Some(n) => {
                    let mine = self.names().iter().any(|w| n.contains(w));
                    if mine && self.shape_ok(value) {
                        return Some(value.to_string());
                    }
                }
                None if !matches!(self, ApiKey::Openai | ApiKey::Mathpix) => {
                    let k = value.rsplit([' ', '"', '\'']).find(|p| !p.is_empty()).unwrap_or("").trim();
                    if self.shape_ok(k) {
                        return Some(k.to_string());
                    }
                }
                None => {}
            }
        }
        None
    }
}

pub struct Secret {
    kind: ApiKey,
    place: Place,
    memory: Mutex<Option<String>>,
}

impl Secret {
    /// Anthropic 的密钥（v0.2 起的用法）
    pub fn new(official: bool) -> Self {
        Self::for_key(ApiKey::Anthropic, official)
    }

    pub fn for_key(kind: ApiKey, official: bool) -> Self {
        let place = if official {
            Place::Keychain
        } else if std::env::var_os(KEY_FILE_ENV).is_some_and(|v| !v.is_empty()) {
            Place::File
        } else {
            Place::Memory
        };
        Secret { kind, place, memory: Mutex::new(None) }
    }


    pub fn place(&self) -> Place {
        self.place
    }

    pub fn save(&self, key: &str) -> Result<(), String> {
        let key = self.kind.check(key)?;
        match self.place {
            Place::Keychain => keychain::save(self.kind.account(), &key),
            Place::File => Err(format!("现在的密钥来自 {KEY_FILE_ENV} 指的文件，不在这里改。")),
            Place::Memory => {
                *self.memory.lock().map_err(|_| "内部错误".to_string())? = Some(key);
                Ok(())
            }
        }
    }

    pub fn clear(&self) -> Result<(), String> {
        match self.place {
            Place::Keychain => keychain::delete(self.kind.account()),
            Place::File => Err(format!("现在的密钥来自 {KEY_FILE_ENV} 指的文件，不在这里改。")),
            Place::Memory => {
                *self.memory.lock().map_err(|_| "内部错误".to_string())? = None;
                Ok(())
            }
        }
    }

    /// 取密钥（正式版会读钥匙串：新装的版本第一次读时系统会问一次密码）
    pub fn load(&self) -> Result<Option<String>, String> {
        match self.place {
            Place::Keychain => keychain::load(self.kind.account()),
            Place::Memory => Ok(self.memory.lock().map_err(|_| "内部错误".to_string())?.clone()),
            Place::File => {
                let path = std::env::var_os(KEY_FILE_ENV).unwrap_or_default();
                let text = std::fs::read_to_string(&path).map_err(|e| format!("读不了 {KEY_FILE_ENV} 指的文件：{e}"))?;
                match self.kind.find_in(&text) {
                    Some(k) => Ok(Some(k)),
                    None => Err(format!("{KEY_FILE_ENV} 指的文件里没有 {} 的密钥", self.kind.label())),
                }
            }
        }
    }

    /// 不读钥匙串地判断「填过没有」（设置页用：免得每次打开都弹系统提示）
    pub fn known_present(&self, saved_flag: bool) -> bool {
        match self.place {
            Place::Keychain => saved_flag,
            Place::Memory => self.memory.lock().map(|m| m.is_some()).unwrap_or(false),
            Place::File => true,
        }
    }
}

#[cfg(all(not(test), target_os = "macos"))]
mod keychain {
    use security_framework::passwords::{delete_generic_password, get_generic_password, set_generic_password};

    const SERVICE: &str = "Pith";
    /// errSecItemNotFound
    const NOT_FOUND: i32 = -25300;

    pub fn save(account: &str, key: &str) -> Result<(), String> {
        let put = || set_generic_password(SERVICE, account, key.as_bytes());
        if put().is_ok() {
            return Ok(());
        }
        // 旧版本写的条目先要让这个版本读得到（系统弹一次授权），再写
        let _ = get_generic_password(SERVICE, account);
        put().map_err(|e| format!("存不进钥匙串：{e}"))
    }

    pub fn load(account: &str) -> Result<Option<String>, String> {
        match get_generic_password(SERVICE, account) {
            Ok(b) => Ok(Some(String::from_utf8_lossy(&b).trim().to_string()).filter(|k| !k.is_empty())),
            Err(e) if e.code() == NOT_FOUND => Ok(None),
            Err(e) => Err(format!("读不了钥匙串里的密钥：{e}。系统询问时请输入登录密码并点「始终允许」。")),
        }
    }

    pub fn delete(account: &str) -> Result<(), String> {
        match delete_generic_password(SERVICE, account) {
            Ok(()) => Ok(()),
            Err(e) if e.code() == NOT_FOUND => Ok(()),
            Err(e) => Err(format!("删不掉钥匙串里的密钥：{e}")),
        }
    }
}

/// 钥匙串只在 macOS 上有。其他系统（云端 Linux 开发环境）只为编得过，正式版不会走到这里
#[cfg(all(not(test), not(target_os = "macos")))]
mod keychain {
    const UNSUPPORTED: &str = "钥匙串只在 macOS 上可用";
    pub fn save(_: &str, _: &str) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }
    pub fn load(_: &str) -> Result<Option<String>, String> {
        Err(UNSUPPORTED.into())
    }
    pub fn delete(_: &str) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }
}

/// 测试从不碰钥匙串
#[cfg(test)]
mod keychain {
    pub fn save(_: &str, _: &str) -> Result<(), String> {
        Err("测试不碰钥匙串".into())
    }
    pub fn load(_: &str) -> Result<Option<String>, String> {
        Err("测试不碰钥匙串".into())
    }
    pub fn delete(_: &str) -> Result<(), String> {
        Err("测试不碰钥匙串".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAKE: &str = "sk-ant-api03-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

    #[test]
    fn key_shape() {
        assert_eq!(ApiKey::Anthropic.check(&format!("  {FAKE}\n")).unwrap(), FAKE);
        assert!(ApiKey::Anthropic.check("sk-proj-abc").is_err());
        assert!(ApiKey::Anthropic.check("sk-ant-short").is_err());
        assert!(ApiKey::Anthropic.check(&format!("{FAKE} extra")).is_err());
        // 报错里不带输入的内容
        assert!(!ApiKey::Anthropic.check("sk-ant-secret value").unwrap_err().contains("secret"));
    }

    #[test]
    fn memory_place_keeps_the_key_only_in_memory() {
        let s = Secret { kind: ApiKey::Anthropic, place: Place::Memory, memory: Mutex::new(None) };
        assert_eq!(s.load().unwrap(), None);
        assert!(!s.known_present(true));
        s.save(FAKE).unwrap();
        assert_eq!(s.load().unwrap().as_deref(), Some(FAKE));
        assert!(s.known_present(false));
        s.clear().unwrap();
        assert_eq!(s.load().unwrap(), None);
        assert!(s.save("不是密钥").is_err());
    }

    const DEEPSEEK: &str = "sk-0123456789abcdef0123456789abcdef";
    const TAVILY: &str = "tvly-dev-AAAAAAAAAAAAAAAAAAAAAAAA";

    #[test]
    fn each_key_has_its_own_shape_and_entry() {
        assert_eq!(ApiKey::Deepseek.check(&format!(" {DEEPSEEK} ")).unwrap(), DEEPSEEK);
        assert_eq!(ApiKey::Tavily.check(TAVILY).unwrap(), TAVILY);
        // 互相认不错：Anthropic 的 sk-ant- 不当成 DeepSeek 的
        assert!(ApiKey::Deepseek.check(FAKE).is_err());
        assert!(ApiKey::Anthropic.check(DEEPSEEK).is_err());
        assert!(ApiKey::Tavily.check(DEEPSEEK).is_err());
        assert!(!ApiKey::Tavily.check("tvly-secret value").unwrap_err().contains("secret"));
        // 钥匙串条目各不相同；Anthropic 的沿用 v0.2 的名字
        assert_eq!(ApiKey::Anthropic.account(), "anthropic-api-key");
        let all = [ApiKey::Anthropic, ApiKey::Deepseek, ApiKey::Tavily];
        let accounts: std::collections::HashSet<_> = all.iter().map(|k| k.account()).collect();
        assert_eq!(accounts.len(), all.len());
    }

    #[test]
    fn one_key_file_can_hold_several_keys() {
        let text = format!("ANTHROPIC_API_KEY={FAKE}\nDEEPSEEK_API_KEY=\"{DEEPSEEK}\"\n{TAVILY}\n");
        assert_eq!(ApiKey::Anthropic.find_in(&text).as_deref(), Some(FAKE));
        assert_eq!(ApiKey::Deepseek.find_in(&text).as_deref(), Some(DEEPSEEK));
        assert_eq!(ApiKey::Tavily.find_in(&text).as_deref(), Some(TAVILY));
        assert_eq!(ApiKey::Deepseek.find_in(&format!("x={FAKE}")), None);
    }

    const OPENAI: &str = "sk-proj-0123456789abcdefABCDEF0123456789";
    const MATHPIX: &str = "pith_test_app:0123456789abcdef0123456789abcdef";

    #[test]
    fn named_lines_tell_openai_and_deepseek_apart() {
        // 新写法：每行「名字=密钥」
        let text = format!("anthropic={FAKE}\nopenai={OPENAI}\ndeepseek={DEEPSEEK}\nTAVILY_API_KEY=\"{TAVILY}\"\nmathpix={MATHPIX}\n");
        assert_eq!(ApiKey::Openai.find_in(&text).as_deref(), Some(OPENAI));
        assert_eq!(ApiKey::Deepseek.find_in(&text).as_deref(), Some(DEEPSEEK));
        assert_eq!(ApiKey::Anthropic.find_in(&text).as_deref(), Some(FAKE));
        assert_eq!(ApiKey::Tavily.find_in(&text).as_deref(), Some(TAVILY));
        assert_eq!(ApiKey::Mathpix.find_in(&text).as_deref(), Some(MATHPIX));
        // 名字是 openai 的那一行不会被当成 DeepSeek 的
        let only_openai = format!("OPENAI_API_KEY={OPENAI}\n");
        assert_eq!(ApiKey::Deepseek.find_in(&only_openai), None);
        // 旧写法照认；没写名字的 sk- 不当成 OpenAI 的
        let old = format!("{FAKE}\n{DEEPSEEK}\n{TAVILY}\n");
        assert_eq!(ApiKey::Deepseek.find_in(&old).as_deref(), Some(DEEPSEEK));
        assert_eq!(ApiKey::Openai.find_in(&old), None);
        assert_eq!(ApiKey::Openai.find_in(&format!("export OPENAI_API_KEY='{OPENAI}'")).as_deref(), Some(OPENAI));
        assert_eq!(ApiKey::Anthropic.find_in(&format!("MY_KEY={FAKE}")).as_deref(), Some(FAKE));
    }

    #[test]
    fn new_keys_have_their_own_shape_and_entry() {
        assert_eq!(ApiKey::Openai.check(&format!(" {OPENAI} ")).unwrap(), OPENAI);
        assert!(ApiKey::Openai.check(FAKE).is_err());
        assert_eq!(ApiKey::Mathpix.check(MATHPIX).unwrap(), MATHPIX);
        assert!(ApiKey::Mathpix.check("no-colon-here-0123456789").is_err());
        assert!(ApiKey::Mathpix.check("ab:short").is_err());
        assert!(!ApiKey::Mathpix.check("app:secret value!").unwrap_err().contains("secret"));
        let accounts: std::collections::HashSet<_> = ApiKey::ALL.iter().map(|k| k.account()).collect();
        assert_eq!(accounts.len(), ApiKey::ALL.len());
        assert_eq!((ApiKey::Openai.account(), ApiKey::Mathpix.account()), ("openai-api-key", "mathpix-api-key"));
    }

    #[test]
    fn memory_place_per_key() {
        let s = Secret { kind: ApiKey::Tavily, place: Place::Memory, memory: Mutex::new(None) };
        assert!(s.save(FAKE).is_err());
        s.save(TAVILY).unwrap();
        assert_eq!(s.load().unwrap().as_deref(), Some(TAVILY));
    }
}
