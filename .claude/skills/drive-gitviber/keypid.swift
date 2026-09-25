import CoreGraphics
import Foundation
// keypid <pid> type <text>        : types text into that process only
// keypid <pid> key <code> [cmd,shift,alt,ctrl] : one key with modifiers
let pid = pid_t(CommandLine.arguments[1])!
let src = CGEventSource(stateID: .hidSystemState)
func post(_ code: CGKeyCode, _ flags: CGEventFlags, _ text: String? = nil) {
  for down in [true, false] {
    let e = CGEvent(keyboardEventSource: src, virtualKey: code, keyDown: down)!
    e.flags = flags
    if let t = text { let u = Array(t.utf16); e.keyboardSetUnicodeString(stringLength: u.count, unicodeString: u) }
    e.postToPid(pid)
    usleep(12000)
  }
}
let mode = CommandLine.arguments[2]
if mode == "type" {
  for ch in CommandLine.arguments[3] { post(0, [], String(ch)) }
} else {
  var f: CGEventFlags = []
  let mods = CommandLine.arguments.count > 4 ? CommandLine.arguments[4] : ""
  if mods.contains("cmd") { f.insert(.maskCommand) }
  if mods.contains("shift") { f.insert(.maskShift) }
  if mods.contains("alt") { f.insert(.maskAlternate) }
  if mods.contains("ctrl") { f.insert(.maskControl) }
  post(CGKeyCode(CommandLine.arguments[3])!, f)
}
