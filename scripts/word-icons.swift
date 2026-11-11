import AppKit
// SVG → 64×64 PNG（透明底），AppKit 画；见 scripts/word-icons.mjs
let args = CommandLine.arguments
for path in args.dropFirst() {
  guard let img = NSImage(contentsOfFile: path) else { print("cannot read \(path)"); exit(1) }
  let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 64, pixelsHigh: 64, bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
  rep.size = NSSize(width: 64, height: 64)
  NSGraphicsContext.saveGraphicsState()
  NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
  img.draw(in: NSRect(x: 0, y: 0, width: 64, height: 64))
  NSGraphicsContext.restoreGraphicsState()
  let png = rep.representation(using: .png, properties: [:])!
  try! png.write(to: URL(fileURLWithPath: path.replacingOccurrences(of: ".svg", with: ".png")))
}
