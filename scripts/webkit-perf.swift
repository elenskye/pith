// 在真 WebKit（和 Tauri 同一个 WKWebView）里跑 src/dev/perfProbe.ts：开一个浮在最前面的 1280×800 窗口（看不见的窗口 WebKit 会停掉动画帧），
// 打开网址，等页面 postMessage("done") 把结果打印出来就退出。用法见 docs/testing.md「换配色的帧」。
import Cocoa
import WebKit
final class D: NSObject, NSApplicationDelegate, WKScriptMessageHandler {
  var w: NSWindow!
  var v: WKWebView!
  func applicationDidFinishLaunching(_ n: Notification) {
    let cfg = WKWebViewConfiguration()
    cfg.userContentController.add(self, name: "done")
    v = WKWebView(frame: NSRect(x: 0, y: 0, width: 1280, height: 800), configuration: cfg)
    w = NSWindow(contentRect: NSRect(x: 120, y: 60, width: 1280, height: 800), styleMask: [.titled, .closable], backing: .buffered, defer: false)
    w.title = "Pith perf"
    w.contentView = v
    w.level = .floating
    w.makeKeyAndOrderFront(nil)
    NSApp.activate(ignoringOtherApps: true)
    v.load(URLRequest(url: URL(string: CommandLine.arguments[1])!))
    DispatchQueue.main.asyncAfter(deadline: .now() + 240) { print("timeout"); NSApp.terminate(nil) }
  }
  func userContentController(_ u: WKUserContentController, didReceive m: WKScriptMessage) {
    print(m.body); fflush(stdout); NSApp.terminate(nil)
  }
}
let app = NSApplication.shared
app.setActivationPolicy(.regular)
let d = D()
app.delegate = d
app.run()
