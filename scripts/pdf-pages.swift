// 用法：swift scripts/pdf-pages.swift <in.pdf> <输出目录> [倍数]
// 每页渲染成 <输出目录>/pNNN.png；标准输出每页一行：页眉区、页脚区（上下各 12%）的文字和正文区第一行。
// 正文区没有墨迹的页另起一行标 BLANK-BODY（Word 为「奇数页起」插的空白页、多出来的空白页）。
// 只用 macOS 自带的 PDFKit，不装别的工具。
import AppKit
import PDFKit

let args = CommandLine.arguments
guard args.count >= 3, let doc = PDFDocument(url: URL(fileURLWithPath: args[1])) else {
    print("用法：swift pdf-pages.swift <in.pdf> <输出目录> [倍数]"); exit(1)
}
let outDir = URL(fileURLWithPath: args[2])
let scale = args.count > 3 ? CGFloat(Double(args[3]) ?? 1.0) : 1.0
try? FileManager.default.createDirectory(at: outDir, withIntermediateDirectories: true)

func text(_ page: PDFPage, _ rect: CGRect) -> String {
    (page.selection(for: rect)?.string ?? "").replacingOccurrences(of: "\n", with: " ⏎ ").trimmingCharacters(in: .whitespaces)
}

print("pages=\(doc.pageCount)")
for i in 0..<doc.pageCount {
    guard let page = doc.page(at: i) else { continue }
    let b = page.bounds(for: .mediaBox)
    let band = b.height * 0.12
    let head = text(page, CGRect(x: b.minX, y: b.maxY - band, width: b.width, height: band))
    let foot = text(page, CGRect(x: b.minX, y: b.minY, width: b.width, height: band))
    let first = (page.string ?? "").split(separator: "\n").first(where: { !$0.trimmingCharacters(in: .whitespaces).isEmpty }).map(String.init) ?? ""
    print(String(format: "%03d", i + 1) + " | head: \(head) | foot: \(foot) | first: \(first.prefix(30))")
    let w = Int(b.width * scale), h = Int(b.height * scale)
    guard let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: w, pixelsHigh: h, bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0) else { continue }
    NSGraphicsContext.saveGraphicsState()
    let ctx = NSGraphicsContext(bitmapImageRep: rep)!
    NSGraphicsContext.current = ctx
    ctx.cgContext.setFillColor(NSColor.white.cgColor)
    ctx.cgContext.fill(CGRect(x: 0, y: 0, width: w, height: h))
    ctx.cgContext.scaleBy(x: scale, y: scale)
    page.draw(with: .mediaBox, to: ctx.cgContext)
    NSGraphicsContext.restoreGraphicsState()
    var ink = 0, total = 0
    var y = Int(Double(h) * 0.12)
    while y < Int(Double(h) * 0.88) {
        var x = 0
        while x < w {
            if let c = rep.colorAt(x: x, y: y), c.brightnessComponent < 0.9 { ink += 1 }
            total += 1
            x += 3
        }
        y += 3
    }
    if total > 0 && Double(ink) / Double(total) < 0.0005 { print(String(format: "%03d BLANK-BODY", i + 1)) }
    if let png = rep.representation(using: .png, properties: [:]) {
        try? png.write(to: outDir.appendingPathComponent(String(format: "p%03d.png", i + 1)))
    }
}
