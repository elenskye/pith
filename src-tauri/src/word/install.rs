//! Word 集成的安装与卸载（`decisions/0015`、`0016`）：
//! - 全局模板 Pith.dotm（RibbonX「Pith」选项卡 + VBA）→ Word 启动文件夹，Word 每次启动自动加载
//! - Pith.scpt（AppleScriptTask 调用的脚本）+ Pith-bridge.sh（连本机服务、没开就后台启动 Pith）
//!   → `~/Library/Application Scripts/com.microsoft.Word/`
//!
//! Word 启动文件夹在 Office 的组容器里，macOS（App 数据保护）不让 Pith 直接写（实测「Operation not permitted」），
//! 请访达代写也不行（访达报 -5000「没有权限」，2026-11-10 实测）。macOS 只认用户本人的动作：在「打开」对话框里选中
//! 这个文件夹，或在访达里拖进去。所以写不进去时，模板先存进数据目录，设置页打开一个已经指向启动文件夹的对话框，
//! 用户点一下「打开」，Pith 就写进去（`install_into`）；也可以照步骤手动拖。安装记录写在数据目录的 `word/installed.json`，
//! 状态检查只看记录与脚本，不去碰组容器（免得每次打开设置页都弹系统提示）。

use std::path::{Path, PathBuf};
use std::process::Command;

use docx_engine::template::{self, Template};
use docx_engine::vba::{Module, ModuleKind, Project};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const TEMPLATE: &str = "Pith.dotm";
pub const SCRIPT: &str = "Pith.scpt";
pub const HELPER: &str = "Pith-bridge.sh";
const RECORD: &str = "installed.json";

const VBA: &str = include_str!("../../word/Pith.bas");
const RIBBON: &str = include_str!("../../word/customUI.xml");
const APPLESCRIPT: &str = include_str!("../../word/Pith.applescript");
const HELPER_SH: &str = include_str!("../../word/pith-bridge.sh");
/// 选项卡图标（lucide 图标生成，`scripts/word-icons.mjs`）
const IMAGES: &[(&str, &[u8])] = &[
    ("citation", include_bytes!("../../word/images/citation.png")),
    ("bibliography", include_bytes!("../../word/images/bibliography.png")),
    ("refresh", include_bytes!("../../word/images/refresh.png")),
    ("style", include_bytes!("../../word/images/style.png")),
    ("zotero", include_bytes!("../../word/images/zotero.png")),
    ("open", include_bytes!("../../word/images/open.png")),
];
const NOTE: &str = "Pith 的 Word 全局模板：放在 Word 启动文件夹里，Word 顶栏会出现「Pith」选项卡。由 Pith 安装与更新，请不要手改。";

/// 装到哪里
#[derive(Clone, Debug)]
pub struct Places {
    /// Word 启动文件夹（…/Startup.localized/Word）
    pub startup: PathBuf,
    /// `~/Library/Application Scripts/com.microsoft.Word`
    pub scripts: PathBuf,
    /// Pith 数据目录下的 `word/`：安装记录、手动安装用的模板副本
    pub own: PathBuf,
}

fn pick(base: &Path, names: [&str; 2]) -> PathBuf {
    names.iter().map(|n| base.join(n)).find(|p| p.exists()).unwrap_or_else(|| base.join(names[0]))
}

impl Places {
    pub fn for_home(home: &Path, data_dir: &Path) -> Self {
        let office = home.join("Library/Group Containers/UBF8T346G9.Office");
        let content = pick(&office, ["User Content.localized", "User Content"]);
        let startup = pick(&content, ["Startup.localized", "Startup"]).join("Word");
        Places { startup, scripts: home.join("Library/Application Scripts/com.microsoft.Word"), own: data_dir.join("word") }
    }
}

/// 桥接脚本里写死的东西：连哪个数据目录的服务、Pith 没开时怎么启动
#[derive(Clone, Debug)]
pub struct BridgeConfig {
    pub data_dir: PathBuf,
    /// 正式版：Pith.app 的路径；开发版：可执行文件的路径；None：不自动启动
    pub launch: Option<Launch>,
    /// 用 PITH_DATA_DIR 指定了数据目录时，自动启动的 Pith 也用它
    pub data_env: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub enum Launch {
    App(PathBuf),
    Binary(PathBuf),
}

impl Launch {
    /// 由正在运行的可执行文件推出：在 .app 里就启动整个 App，否则（开发版）直接起可执行文件
    pub fn from_exe(exe: &Path) -> Self {
        match exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app")) {
            Some(app) => Launch::App(app.to_path_buf()),
            None => Launch::Binary(exe.to_path_buf()),
        }
    }
}

/// 单引号包起来给 sh 用
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

pub fn helper_script(cfg: &BridgeConfig) -> String {
    let (kind, launch) = match &cfg.launch {
        Some(Launch::App(p)) => ("app", p.display().to_string()),
        Some(Launch::Binary(p)) => ("bin", p.display().to_string()),
        None => ("", String::new()),
    };
    HELPER_SH
        .replace("@DATA_DIR@", &sh_quote(&cfg.data_dir.display().to_string()))
        .replace("@LAUNCH_KIND@", &sh_quote(kind))
        .replace("@LAUNCH@", &sh_quote(&launch))
        .replace("@DATA_ENV@", &sh_quote(&cfg.data_env.as_ref().map(|p| p.display().to_string()).unwrap_or_default()))
}

/// 读回已安装的桥接脚本连的是哪个数据目录
fn helper_var(text: &str, name: &str) -> Option<String> {
    let v = text.lines().find_map(|l| l.strip_prefix(name)?.strip_prefix('='))?;
    Some(v.trim_matches('\'').replace(r"'\''", "'"))
}

fn helper_data_dir(text: &str) -> Option<String> {
    helper_var(text, "DATA_DIR")
}

/// 已安装的桥接脚本要启动的 Pith 已经不在原处（例如清了编译目录，或 Pith 挪进了「应用程序」）
fn helper_launch_missing(text: &str) -> bool {
    helper_var(text, "LAUNCH").is_some_and(|p| !p.is_empty() && !Path::new(&p).exists())
}

fn applescript(helper: &Path) -> String {
    APPLESCRIPT.replace("@HELPER@", &helper.display().to_string().replace('\\', "\\\\").replace('"', "\\\""))
}

/// 由 `word/Pith.bas` 与 `word/customUI.xml` 生成模板（同样的源码得到同样的字节）
pub fn template_bytes() -> Result<Vec<u8>, String> {
    let code: Vec<&str> = VBA.lines().filter(|l| !l.starts_with("Attribute VB_")).collect();
    let project = Project::word("Pith", vec![Module { name: "PithMain".into(), kind: ModuleKind::Standard, code: code.join("\n") }]);
    template::build_dotm(&Template { project: &project, ribbon: RIBBON, images: IMAGES, note: NOTE })
}

fn hash(b: &[u8]) -> String {
    Sha256::digest(b).iter().take(12).map(|x| format!("{x:02x}")).collect()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub version: String,
    pub hash: String,
    /// 模板实际放在哪；None：写不进 Word 启动文件夹，等用户手动放
    pub template: Option<String>,
    pub installed_at: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub record: Option<Record>,
    /// 已装的模板就是这个版本的 Pith 生成的
    pub up_to_date: bool,
    pub script_installed: bool,
    /// 已装的桥接脚本连的数据目录（开发版 / 正式版装的不同）
    pub bridge_data_dir: Option<String>,
    /// 桥接脚本里写的 Pith 已经不在那个位置：Word 里点按钮启动不了 Pith，要「更新」
    pub bridge_launch_missing: bool,
    pub startup_dir: String,
    pub scripts_dir: String,
    /// 手动安装用的模板副本（写不进启动文件夹时）
    pub manual_template: Option<String>,
}

pub fn status(places: &Places) -> Status {
    let record: Option<Record> = std::fs::read(places.own.join(RECORD)).ok().and_then(|b| serde_json::from_slice(&b).ok());
    let current = template_bytes().map(|b| hash(&b)).ok();
    let helper = std::fs::read_to_string(places.scripts.join(HELPER)).ok();
    let manual = places.own.join(TEMPLATE);
    Status {
        up_to_date: record.as_ref().is_some_and(|r| Some(&r.hash) == current.as_ref()),
        script_installed: places.scripts.join(SCRIPT).is_file() && helper.is_some(),
        bridge_data_dir: helper.as_deref().and_then(helper_data_dir),
        bridge_launch_missing: helper.as_deref().is_some_and(helper_launch_missing),
        startup_dir: places.startup.display().to_string(),
        scripts_dir: places.scripts.display().to_string(),
        manual_template: (record.as_ref().is_some_and(|r| r.template.is_none()) && manual.is_file()).then(|| manual.display().to_string()),
        record,
    }
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("无法创建 {}：{e}", dir.display()))?;
    }
    std::fs::write(path, bytes).map_err(|e| format!("无法写入 {}：{e}", path.display()))
}

/// 编译 AppleScript（Word 的 AppleScriptTask 要编译好的 .scpt）
fn compile_applescript(source: &str, out: &Path, work: &Path) -> Result<(), String> {
    let src = work.join("Pith.applescript");
    write(&src, source.as_bytes())?;
    let tmp = out.with_extension("scpt.tmp");
    let o = Command::new("/usr/bin/osacompile").arg("-o").arg(&tmp).arg(&src).output().map_err(|e| format!("无法运行 osacompile：{e}"))?;
    let _ = std::fs::remove_file(&src);
    if !o.status.success() {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("AppleScript 编译失败：{}", String::from_utf8_lossy(&o.stderr).trim()));
    }
    std::fs::rename(&tmp, out).map_err(|e| format!("无法写入 {}：{e}", out.display()))
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallReport {
    pub status: Status,
    /// 写不进 Word 启动文件夹的原因（这时模板存在数据目录里，要用户选一下文件夹或手动放）
    pub template_error: Option<String>,
}

pub fn install(places: &Places, cfg: &BridgeConfig, version: &str, now: &str) -> Result<InstallReport, String> {
    let bytes = template_bytes()?;
    std::fs::create_dir_all(&places.own).map_err(|e| format!("无法创建 {}：{e}", places.own.display()))?;
    // 桥接脚本先装：模板放不进去时，用户放好模板也能用
    let helper = places.scripts.join(HELPER);
    write(&helper, helper_script(cfg).as_bytes())?;
    compile_applescript(&applescript(&helper), &places.scripts.join(SCRIPT), &places.own)?;
    let target = places.startup.join(TEMPLATE);
    let manual = places.own.join(TEMPLATE);
    let template_error = match write(&target, &bytes) {
        Ok(()) => {
            let _ = std::fs::remove_file(&manual);
            None
        }
        Err(e) => {
            write(&manual, &bytes)?;
            Some(e)
        }
    };
    let record = Record { version: version.to_string(), hash: hash(&bytes), template: template_error.is_none().then(|| target.display().to_string()), installed_at: now.to_string() };
    write(&places.own.join(RECORD), &serde_json::to_vec_pretty(&record).map_err(|e| e.to_string())?)?;
    Ok(InstallReport { status: status(places), template_error })
}

/// 用户在「打开」对话框里选中了 Word 启动文件夹：把模板写进去（macOS 认这个动作）。
/// 只认 Office 组容器里叫 Word 的文件夹，免得选错地方。
pub fn install_into(places: &Places, dir: &Path, version: &str, now: &str) -> Result<InstallReport, String> {
    let s = dir.display().to_string();
    if !s.contains("UBF8T346G9.Office") || dir.file_name().is_none_or(|n| n != "Word") {
        return Err(format!("选的不是 Word 启动文件夹（应在 Office 的 Startup 里、名叫 Word）：{s}"));
    }
    let bytes = template_bytes()?;
    let target = dir.join(TEMPLATE);
    std::fs::write(&target, &bytes).map_err(|e| format!("还是写不进 {s}：{e}"))?;
    let _ = std::fs::remove_file(places.own.join(TEMPLATE));
    let record = Record { version: version.to_string(), hash: hash(&bytes), template: Some(target.display().to_string()), installed_at: now.to_string() };
    write(&places.own.join(RECORD), &serde_json::to_vec_pretty(&record).map_err(|e| e.to_string())?)?;
    Ok(InstallReport { status: status(places), template_error: None })
}

/// 卸载：删模板、脚本、记录。启动文件夹里的模板删不掉（macOS 不让）时报出来，告诉用户去访达里删；其余照删
pub fn uninstall(places: &Places) -> Result<Status, String> {
    let mut problems = Vec::new();
    let record: Option<Record> = std::fs::read(places.own.join(RECORD)).ok().and_then(|b| serde_json::from_slice(&b).ok());
    let mut files = vec![places.startup.join(TEMPLATE)];
    if let Some(t) = record.and_then(|r| r.template).map(PathBuf::from).filter(|t| !files.contains(t)) {
        files.push(t);
    }
    files.extend([places.scripts.join(SCRIPT), places.scripts.join(HELPER), places.own.join(TEMPLATE), places.own.join(RECORD)]);
    for p in files {
        match std::fs::remove_file(&p) {
            Ok(()) => {}
            Err(e) if matches!(e.kind(), std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory) => {}
            Err(e) => problems.push(format!("{}：{e}", p.display())),
        }
    }
    if problems.is_empty() { Ok(status(places)) } else { Err(format!("有文件删不掉（Word 启动文件夹里的请在访达里删）：{}", problems.join("；"))) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::temp_dir;

    fn places(dir: &Path) -> Places {
        Places::for_home(&dir.join("home"), &dir.join("data"))
    }

    #[test]
    fn template_is_deterministic_and_has_the_macros() {
        let a = template_bytes().unwrap();
        assert_eq!(a, template_bytes().unwrap());
        let pkg = docx_engine::package::Package::read(&a).unwrap();
        let rb = docx_engine::vba::read_back(pkg.get("word/vbaProject.bin").unwrap()).unwrap();
        let main = &rb.modules.iter().find(|m| m.0 == "PithMain").unwrap().2;
        assert!(main.starts_with("Attribute VB_Name = \"PithMain\"\r\n"));
        for sub in ["PithRibbonAction", "PithAddEditCitation", "PithAddBibliography", "PithRefresh", "PithOpen", "PithFromZotero", "PithStyleMenu", "PithStyleChosen"] {
            assert!(main.contains(&format!("Public Sub {sub}(")), "{sub}");
        }
        // 选项卡里每个按钮的回调都在模块里；图片都打包了
        let ui = pkg.get_str("customUI/customUI.xml").unwrap();
        let rels = pkg.get_str("customUI/_rels/customUI.xml.rels").unwrap();
        let doc = roxmltree::Document::parse(&ui).unwrap();
        let mut n = 0;
        for b in doc.descendants().filter(|n| n.has_tag_name((template::RIBBON_NS, "button")) || n.has_tag_name((template::RIBBON_NS, "dynamicMenu"))) {
            n += 1;
            let id = b.attribute("id").unwrap();
            assert!(b.attribute("label").unwrap().is_ascii());
            if let Some(img) = b.attribute("image") {
                assert!(rels.contains(&format!(r#"Id="{img}""#)), "图片 {img}");
                assert!(pkg.get(&format!("customUI/images/{img}.png")).is_some());
            }
            if b.has_tag_name((template::RIBBON_NS, "button")) {
                assert_eq!(b.attribute("onAction"), Some("PithMain.PithRibbonAction"));
                assert!(main.contains(&format!("Case \"{id}\"")), "按钮 {id} 没有处理");
            } else {
                assert_eq!(b.attribute("getContent"), Some("PithMain.PithStyleMenu"));
            }
        }
        assert_eq!(n, 6, "6 个按钮 / 菜单");
    }

    #[test]
    fn places_follow_the_localized_folders() {
        let dir = temp_dir("word-places");
        let p = places(&dir);
        assert!(p.startup.ends_with("UBF8T346G9.Office/User Content.localized/Startup.localized/Word"));
        let plain = dir.join("home2/Library/Group Containers/UBF8T346G9.Office/User Content/Startup");
        std::fs::create_dir_all(&plain).unwrap();
        let p2 = Places::for_home(&dir.join("home2"), &dir.join("data"));
        assert_eq!(p2.startup, plain.join("Word"));
    }

    #[test]
    fn helper_bakes_paths_safely() {
        let cfg = BridgeConfig { data_dir: "/Users/x/Pith Dev".into(), launch: Some(Launch::App("/Applications/Pith.app".into())), data_env: Some("/tmp/it's here".into()) };
        let s = helper_script(&cfg);
        assert!(s.contains("DATA_DIR='/Users/x/Pith Dev'\n"));
        assert!(s.contains("LAUNCH_KIND='app'\n") && s.contains("LAUNCH='/Applications/Pith.app'\n"));
        assert!(s.contains(r"DATA_ENV='/tmp/it'\''s here'"));
        for ph in ["@DATA_DIR@", "@LAUNCH_KIND@", "@LAUNCH@", "@DATA_ENV@"] {
            assert!(!s.contains(ph), "占位符 {ph} 没替换");
        }
        assert_eq!(helper_data_dir(&s).as_deref(), Some("/Users/x/Pith Dev"));
        assert_eq!(helper_var(&s, "LAUNCH").as_deref(), Some("/Applications/Pith.app"));
        assert!(helper_launch_missing(&s.replace("/Applications/Pith.app", "/nowhere/target/release/bundle/macos/Pith.app")));
        assert!(matches!(Launch::from_exe(Path::new("/Applications/Pith.app/Contents/MacOS/pith")), Launch::App(p) if p == Path::new("/Applications/Pith.app")));
        assert!(matches!(Launch::from_exe(Path::new("/r/target/debug/pith")), Launch::Binary(_)));
        let none = helper_script(&BridgeConfig { data_dir: "/d".into(), launch: None, data_env: None });
        assert!(none.contains("LAUNCH_KIND=''"));
    }

    /// 装进临时的「家目录」：模板、脚本、记录都在；再装一次是最新；卸载干净
    #[test]
    fn install_and_uninstall() {
        let dir = temp_dir("word-install");
        let p = places(&dir);
        let s = status(&p);
        assert!(s.record.is_none() && !s.script_installed && !s.up_to_date);
        let cfg = BridgeConfig { data_dir: dir.join("data"), launch: None, data_env: None };
        let r = install(&p, &cfg, "0.2.0", "2026-11-10T00:00:00.000Z").unwrap();
        assert!(r.template_error.is_none());
        assert!(r.status.up_to_date && r.status.script_installed);
        assert_eq!(std::fs::read(p.startup.join(TEMPLATE)).unwrap(), template_bytes().unwrap());
        assert_eq!(r.status.bridge_data_dir.as_deref(), Some(dir.join("data").display().to_string().as_str()));
        // 编译出来的脚本能被 osascript 读：handler 在
        let o = Command::new("/usr/bin/osadecompile").arg(p.scripts.join(SCRIPT)).output().unwrap();
        let text = String::from_utf8_lossy(&o.stdout);
        assert!(text.contains("on pithCall(payload)") && text.contains(HELPER), "{text}");
        let s = uninstall(&p).unwrap();
        assert!(s.record.is_none() && !s.script_installed);
        assert!(!p.startup.join(TEMPLATE).exists());
    }

    /// 写不进启动文件夹：先存数据目录；用户在对话框里选中文件夹后写进去；选错文件夹拒绝
    #[test]
    fn install_into_the_folder_the_user_chose() {
        let dir = temp_dir("word-install-into");
        let mut p = places(&dir);
        std::fs::create_dir_all(dir.join("blocked")).unwrap();
        std::fs::write(dir.join("blocked/Word"), b"not a folder").unwrap();
        p.startup = dir.join("blocked/Word");
        let cfg = BridgeConfig { data_dir: dir.join("data"), launch: None, data_env: None };
        let r = install(&p, &cfg, "0.2.0", "2026-11-10T00:00:00.000Z").unwrap();
        assert!(r.template_error.is_some() && r.status.manual_template.is_some());
        let chosen = dir.join("home/Library/Group Containers/UBF8T346G9.Office/User Content/Startup/Word");
        std::fs::create_dir_all(&chosen).unwrap();
        assert!(install_into(&p, &dir.join("elsewhere/Word"), "0.2.0", "t").unwrap_err().contains("不是 Word 启动文件夹"));
        assert!(install_into(&p, chosen.parent().unwrap(), "0.2.0", "t").is_err());
        let r = install_into(&p, &chosen, "0.2.0", "2026-11-10T00:00:01.000Z").unwrap();
        assert!(r.template_error.is_none() && r.status.up_to_date && r.status.manual_template.is_none());
        assert_eq!(std::fs::read(chosen.join(TEMPLATE)).unwrap(), template_bytes().unwrap());
        assert!(!p.own.join(TEMPLATE).exists(), "数据目录里的副本删掉");
        // 卸载也删记录里的那份
        uninstall(&p).unwrap();
        assert!(!chosen.join(TEMPLATE).exists());
    }

    /// Word 启动文件夹写不进去：模板存进数据目录，等用户手动放
    #[test]
    fn falls_back_to_manual_template() {
        let dir = temp_dir("word-install-manual");
        let mut p = places(&dir);
        std::fs::create_dir_all(dir.join("blocked")).unwrap();
        std::fs::write(dir.join("blocked/Word"), b"a file where the folder should be").unwrap();
        p.startup = dir.join("blocked/Word");
        let cfg = BridgeConfig { data_dir: dir.join("data"), launch: None, data_env: None };
        let r = install(&p, &cfg, "0.2.0", "2026-11-10T00:00:00.000Z").unwrap();
        assert!(r.template_error.is_some());
        assert!(r.status.manual_template.is_some() && r.status.script_installed);
        assert!(r.status.record.as_ref().unwrap().template.is_none());
    }
}
