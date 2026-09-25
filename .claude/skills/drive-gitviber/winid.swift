import CoreGraphics
import Foundation
// Usage: winid <owner name>  -> prints "id x y w h" for each on-screen window of that owner
let owner = CommandLine.arguments[1]
let list = CGWindowListCopyWindowInfo([.optionAll], kCGNullWindowID) as! [[String: Any]]
for w in list where (w[kCGWindowOwnerName as String] as? String) == owner && (w[kCGWindowLayer as String] as? Int) == 0 {
  let b = w[kCGWindowBounds as String] as! [String: Any]
  print(w[kCGWindowNumber as String]!, b["X"]!, b["Y"]!, b["Width"]!, b["Height"]!, w[kCGWindowOwnerPID as String]!)
}
