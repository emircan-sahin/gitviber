// The native app's window for browser-check-native.sh: found, read and clicked, nothing else.
//   gvwin win <pid>              "id x y w h" of each window of the pid on a display, largest first
//                                (not an agent tab's panel, which is kept off every display)
//   gvwin front <pid>            exit 0 when that pid is the frontmost app
//   gvwin ocr <png> [scale]      "x y w h text" for each line of text in the picture, in its
//                                pixels divided by scale (2 for a Retina window shot: points)
//   gvwin click <pid> <x> <y> [right|double]
//                                a real click at a screen point, only when the pid is frontmost
//                                and its window is the topmost one there; exit 3 when it isn't
//   gvwin drag <pid> <x> <y> <x2> <y2>
//                                pressed at one point, moved to the other in steps, let go; the
//                                same checks as click, at the start
//   gvwin unhide <pid>           the app shown again after ⌘H (another app can't hide it)
//   gvwin type <pid> <text>      text typed into that pid only, a character at a time
//   gvwin key <pid> <code> [mods]
//                                one key to that pid only (mods: cmd,shift,alt,ctrl), by its code:
//                                the app reads it through the keyboard layout the Mac has on
import AppKit
import CoreGraphics
import Foundation
import Vision

let args = CommandLine.arguments
func fail(_ s: String, _ code: Int32 = 2) -> Never {
  FileHandle.standardError.write((s + "\n").data(using: .utf8)!)
  exit(code)
}
guard args.count >= 3 else { fail("usage: gvwin win|front|ocr|click|drag|key|type|unhide …") }

func windows(_ pid: pid_t, onScreen: Bool) -> [[String: Any]] {
  let opts: CGWindowListOption = onScreen ? [.optionOnScreenOnly] : [.optionAll]
  return (CGWindowListCopyWindowInfo(opts, kCGNullWindowID) as? [[String: Any]] ?? [])
    .filter { ($0[kCGWindowOwnerPID as String] as? pid_t) == pid && ($0[kCGWindowLayer as String] as? Int) == 0 }
}
func bounds(_ w: [String: Any]) -> CGRect {
  let b = w[kCGWindowBounds as String] as? [String: Double] ?? [:]
  return CGRect(x: b["X"] ?? 0, y: b["Y"] ?? 0, width: b["Width"] ?? 0, height: b["Height"] ?? 0)
}
func frontmost(_ pid: pid_t) -> Bool { NSWorkspace.shared.frontmostApplication?.processIdentifier == pid }
func onDisplay(_ r: CGRect) -> Bool {
  var ids = [CGDirectDisplayID](repeating: 0, count: 16)
  var n: UInt32 = 0
  CGGetActiveDisplayList(16, &ids, &n)
  return ids.prefix(Int(n)).contains { CGDisplayBounds($0).intersects(r) }
}

switch args[1] {
case "win":
  guard let pid = pid_t(args[2]) else { fail("pid?") }
  for w in windows(pid, onScreen: true).filter({ onDisplay(bounds($0)) }).sorted(by: { bounds($0).width * bounds($0).height > bounds($1).width * bounds($1).height }) {
    let b = bounds(w)
    print(w[kCGWindowNumber as String] ?? 0, Int(b.minX), Int(b.minY), Int(b.width), Int(b.height))
  }
case "front":
  guard let pid = pid_t(args[2]) else { fail("pid?") }
  exit(frontmost(pid) ? 0 : 1)
case "ocr":
  let scale = args.count > 3 ? Double(args[3]) ?? 1 : 1
  guard let img = NSImage(contentsOfFile: args[2]),
        let cg = img.cgImage(forProposedRect: nil, context: nil, hints: nil) else { fail("no picture") }
  let req = VNRecognizeTextRequest()
  req.recognitionLevel = .accurate
  req.usesLanguageCorrection = false
  try? VNImageRequestHandler(cgImage: cg).perform([req])
  let (w, h) = (Double(cg.width), Double(cg.height))
  for o in req.results ?? [] {
    guard let t = o.topCandidates(1).first?.string else { continue }
    let b = o.boundingBox
    print(Int(b.minX * w / scale), Int((1 - b.maxY) * h / scale), Int(b.width * w / scale), Int(b.height * h / scale), t)
  }
case "click", "drag":
  guard args.count >= 5, let pid = pid_t(args[2]), let x = Double(args[3]), let y = Double(args[4]) else { fail("click <pid> <x> <y>") }
  let p = CGPoint(x: x, y: y)
  // The topmost visible window at the point, of any app or layer (a banner, a menu), must be ours.
  let all = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] ?? []
  let top = all.first { bounds($0).contains(p) && (($0[kCGWindowAlpha as String] as? Double) ?? 1) > 0 }
  guard frontmost(pid), let top, (top[kCGWindowOwnerPID as String] as? pid_t) == pid else {
    fail("not clicking: the app isn't frontmost or its window isn't on top at \(Int(x)),\(Int(y))", 3)
  }
  if args[1] == "drag" {
    guard args.count >= 7, let x2 = Double(args[5]), let y2 = Double(args[6]) else { fail("drag <pid> <x> <y> <x2> <y2>") }
    let post = { (t: CGEventType, at: CGPoint) in
      CGEvent(mouseEventSource: nil, mouseType: t, mouseCursorPosition: at, mouseButton: .left)?.post(tap: .cghidEventTap)
      usleep(16_000)
    }
    post(.mouseMoved, p)
    post(.leftMouseDown, p)
    for i in 1...20 {
      let f = Double(i) / 20
      post(.leftMouseDragged, CGPoint(x: x + (x2 - x) * f, y: y + (y2 - y) * f))
    }
    post(.leftMouseUp, CGPoint(x: x2, y: y2))
    exit(0)
  }
  let right = args.count > 5 && args[5] == "right"
  let clicks = args.count > 5 && args[5] == "double" ? 2 : 1
  let (down, up): (CGEventType, CGEventType) = right ? (.rightMouseDown, .rightMouseUp) : (.leftMouseDown, .leftMouseUp)
  CGEvent(mouseEventSource: nil, mouseType: .mouseMoved, mouseCursorPosition: p, mouseButton: .left)?.post(tap: .cghidEventTap)
  usleep(60_000)
  for n in 1...clicks {
    for t in [down, up] {
      let e = CGEvent(mouseEventSource: nil, mouseType: t, mouseCursorPosition: p, mouseButton: right ? .right : .left)
      e?.setIntegerValueField(.mouseEventClickState, value: Int64(n))
      e?.post(tap: .cghidEventTap)
      usleep(40_000)
    }
  }
case "key":
  guard args.count >= 4, let pid = pid_t(args[2]), let code = CGKeyCode(args[3]) else { fail("key <pid> <code> [mods]") }
  let mods = args.count > 4 ? args[4] : ""
  var flags: CGEventFlags = []
  for (name, flag) in [("cmd", CGEventFlags.maskCommand), ("shift", .maskShift), ("alt", .maskAlternate), ("ctrl", .maskControl)] where mods.contains(name) {
    flags.insert(flag)
  }
  let src = CGEventSource(stateID: .hidSystemState)
  for down in [true, false] {
    let e = CGEvent(keyboardEventSource: src, virtualKey: code, keyDown: down)
    e?.flags = flags
    e?.postToPid(pid)
    usleep(12_000)
  }
case "unhide":
  guard let pid = pid_t(args[2]), let app = NSRunningApplication(processIdentifier: pid) else { fail("no such app") }
  _ = app.unhide()
case "type":
  guard args.count >= 4, let pid = pid_t(args[2]) else { fail("type <pid> <text>") }
  let src = CGEventSource(stateID: .hidSystemState)
  for ch in args[3] {
    let u = Array(String(ch).utf16)
    for down in [true, false] {
      let e = CGEvent(keyboardEventSource: src, virtualKey: 0, keyDown: down)
      e?.keyboardSetUnicodeString(stringLength: u.count, unicodeString: u)
      e?.postToPid(pid)
      usleep(12_000)
    }
  }
default:
  fail("unknown: \(args[1])")
}
