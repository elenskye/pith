//! Anthropic API 密钥放哪（`decisions/0004`、`0018`）。
//!
//! - 正式版（「应用程序」里的 Pith，正式数据目录）：存 macOS 钥匙串。钥匙串按 App 的签名认人，
//!   所以正式版用固定证书签名（`scripts/install-app.sh`）。
//! - 开发版、指定了数据目录的打包版、测试：不碰钥匙串。密钥只在内存里（退出就没了），
//!   或由 `PITH_API_KEY_FILE` 指向的文件给（真实验收用）。
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

pub struct Secret {
    place: Place,
    memory: Mutex<Option<String>>,
}

/// 只做形状检查（不联网）：Anthropic 的密钥以 sk-ant- 开头
pub fn check_key(key: &str) -> Result<String, String> {
    let k = key.trim();
    if k.starts_with("sk-ant-") && k.len() >= 40 && k.len() <= 300 && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        Ok(k.to_string())
    } else {
        Err("这看起来不是 Anthropic 的 API 密钥（应以 sk-ant- 开头）。到 console.anthropic.com 的 API Keys 里复制完整的一串。".into())
    }
}

impl Secret {
    pub fn new(official: bool) -> Self {
        let place = if official {
            Place::Keychain
        } else if std::env::var_os(KEY_FILE_ENV).is_some_and(|v| !v.is_empty()) {
            Place::File
        } else {
            Place::Memory
        };
        Secret { place, memory: Mutex::new(None) }
    }

    pub fn place(&self) -> Place {
        self.place
    }

    pub fn save(&self, key: &str) -> Result<(), String> {
        let key = check_key(key)?;
        match self.place {
            Place::Keychain => keychain::save(&key),
            Place::File => Err(format!("现在的密钥来自 {KEY_FILE_ENV} 指的文件，不在这里改。")),
            Place::Memory => {
                *self.memory.lock().map_err(|_| "内部错误".to_string())? = Some(key);
                Ok(())
            }
        }
    }

    pub fn clear(&self) -> Result<(), String> {
        match self.place {
            Place::Keychain => keychain::delete(),
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
            Place::Keychain => keychain::load(),
            Place::Memory => Ok(self.memory.lock().map_err(|_| "内部错误".to_string())?.clone()),
            Place::File => {
                let path = std::env::var_os(KEY_FILE_ENV).unwrap_or_default();
                let text = std::fs::read_to_string(&path).map_err(|e| format!("读不了 {KEY_FILE_ENV} 指的文件：{e}"))?;
                // 文件里可以只有密钥，也可以是「名字=密钥」的一行
                let found = text.lines().map(|l| l.rsplit(['=', ' ', '"', '\'']).find(|p| !p.is_empty()).unwrap_or("").trim()).find(|l| l.starts_with("sk-ant-"));
                match found {
                    Some(k) => check_key(k).map(Some),
                    None => Err(format!("{KEY_FILE_ENV} 指的文件里没有 sk-ant- 开头的密钥")),
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

#[cfg(not(test))]
mod keychain {
    use security_framework::passwords::{delete_generic_password, get_generic_password, set_generic_password};

    const SERVICE: &str = "Pith";
    const ACCOUNT: &str = "anthropic-api-key";
    /// errSecItemNotFound
    const NOT_FOUND: i32 = -25300;

    pub fn save(key: &str) -> Result<(), String> {
        let put = || set_generic_password(SERVICE, ACCOUNT, key.as_bytes());
        if put().is_ok() {
            return Ok(());
        }
        // 旧版本写的条目先要让这个版本读得到（系统弹一次授权），再写
        let _ = get_generic_password(SERVICE, ACCOUNT);
        put().map_err(|e| format!("存不进钥匙串：{e}"))
    }

    pub fn load() -> Result<Option<String>, String> {
        match get_generic_password(SERVICE, ACCOUNT) {
            Ok(b) => Ok(Some(String::from_utf8_lossy(&b).trim().to_string()).filter(|k| !k.is_empty())),
            Err(e) if e.code() == NOT_FOUND => Ok(None),
            Err(e) => Err(format!("读不了钥匙串里的密钥：{e}。系统询问时请输入登录密码并点「始终允许」。")),
        }
    }

    pub fn delete() -> Result<(), String> {
        match delete_generic_password(SERVICE, ACCOUNT) {
            Ok(()) => Ok(()),
            Err(e) if e.code() == NOT_FOUND => Ok(()),
            Err(e) => Err(format!("删不掉钥匙串里的密钥：{e}")),
        }
    }
}

/// 测试从不碰钥匙串
#[cfg(test)]
mod keychain {
    pub fn save(_: &str) -> Result<(), String> {
        Err("测试不碰钥匙串".into())
    }
    pub fn load() -> Result<Option<String>, String> {
        Err("测试不碰钥匙串".into())
    }
    pub fn delete() -> Result<(), String> {
        Err("测试不碰钥匙串".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAKE: &str = "sk-ant-api03-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

    #[test]
    fn key_shape() {
        assert_eq!(check_key(&format!("  {FAKE}\n")).unwrap(), FAKE);
        assert!(check_key("sk-proj-abc").is_err());
        assert!(check_key("sk-ant-short").is_err());
        assert!(check_key(&format!("{FAKE} extra")).is_err());
        // 报错里不带输入的内容
        assert!(!check_key("sk-ant-secret value").unwrap_err().contains("secret"));
    }

    #[test]
    fn memory_place_keeps_the_key_only_in_memory() {
        let s = Secret { place: Place::Memory, memory: Mutex::new(None) };
        assert_eq!(s.load().unwrap(), None);
        assert!(!s.known_present(true));
        s.save(FAKE).unwrap();
        assert_eq!(s.load().unwrap().as_deref(), Some(FAKE));
        assert!(s.known_present(false));
        s.clear().unwrap();
        assert_eq!(s.load().unwrap(), None);
        assert!(s.save("不是密钥").is_err());
    }
}
