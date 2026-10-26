// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo
//
// Dumps and drives an app's accessibility tree, as VoiceOver sees it (built and run by
// scripts/ax-check.py; it needs the Accessibility grant of its responsible process).
//
//   ax-dump <pid> <op>...
//
// Ops run in order: `dump` prints the app's windows and their elements, one indented line each
// (retried once when a window has no children yet); `press | inc | dec | focus <sel>` performs
// AXPress, AXIncrement, AXDecrement or sets AXFocused; `set <sel> <text>` sets AXValue; `sleep <s>`
// runs the run loop; `count` prints the notifications seen since the last count, by the
// element's role; `focused` prints the app's focused element; `hit <sel>` the element at the
// centre of the selected one's frame and its parent; `watch <sel>` observes when that element is
// destroyed. A selector is `role`, `role:title` or `:title`, the role or the subrole (first match
// in the dump's order). Notifications are observed from the start: focus, value, layout, title,
// selected rows / text / children, destroyed.

import ApplicationServices
import Foundation

func attr(_ e: AXUIElement, _ name: String) -> AnyObject? {
  var v: AnyObject?
  return AXUIElementCopyAttributeValue(e, name as CFString, &v) == .success ? v : nil
}

func children(_ e: AXUIElement) -> [AXUIElement] {
  attr(e, "AXChildren") as? [AXUIElement] ?? []
}

func text(_ v: AnyObject?) -> String? {
  guard let v else { return nil }

  if CFGetTypeID(v) == AXValueGetTypeID() {
    let ax = v as! AXValue
    var r = CGRect.zero
    var range = CFRange()
    if AXValueGetValue(ax, .cgRect, &r) {
      return "\(Int(r.minX)),\(Int(r.minY)) \(Int(r.width))x\(Int(r.height))"
    }

    if AXValueGetValue(ax, .cfRange, &range) {
      return "\(range.location)+\(range.length)"
    }

    return "\(ax)"
  }

  if CFGetTypeID(v) == AXUIElementGetTypeID() { return "<element>" }

  if let s = v as? String { return "\"\(s)\"" }

  if let a = v as? [AnyObject] { return "[\(a.count)]" }

  return "\(v)"
}

func line(_ e: AXUIElement) -> String {
  var out = (attr(e, "AXRole") as? String) ?? "?"
  if let s = attr(e, "AXSubrole") as? String { out += "/" + s }

  for (key, name) in [
    ("AXTitle", "title"), ("AXValue", "value"), ("AXDescription", "desc"), ("AXHelp", "help"),
    ("AXMinValue", "min"), ("AXMaxValue", "max"), ("AXNumberOfCharacters", "chars"),
    ("AXSelectedTextRange", "sel"), ("AXMenuItemMarkChar", "mark"),
  ] {
    if let v = text(attr(e, key)), v != "\"\"" { out += " \(name)=\(v)" }
  }

  if (attr(e, "AXEnabled") as? Bool) == false { out += " disabled" }

  if (attr(e, "AXFocused") as? Bool) == true { out += " focused" }

  if (attr(e, "AXSelected") as? Bool) == true { out += " selected" }

  if (attr(e, "AXModal") as? Bool) == true { out += " modal" }

  if (attr(e, "AXExpanded") as? Bool) == true { out += " expanded" }

  var names: CFArray?
  if AXUIElementCopyActionNames(e, &names) == .success, let a = names as? [String], !a.isEmpty {
    let short = a.filter { !$0.hasPrefix("AXShowMenu") && !$0.hasPrefix("AXScrollToVisible") }
    let shown = short.map { $0.replacingOccurrences(of: "AX", with: "") }
    if !shown.isEmpty { out += " {" + shown.joined(separator: ",") + "}" }
  }

  if let f = text(attr(e, "AXFrame")) { out += " @" + f }

  return out
}

/// Elements in pre-order with their depth.
func walk(_ e: AXUIElement, _ depth: Int = 0, _ into: inout [(Int, AXUIElement)]) {
  into.append((depth, e))
  for c in children(e) { walk(c, depth + 1, &into) }
}

func windows(_ app: AXUIElement) -> [AXUIElement] {
  attr(app, "AXWindows") as? [AXUIElement] ?? []
}

func all(_ app: AXUIElement) -> [(Int, AXUIElement)] {
  var out: [(Int, AXUIElement)] = []
  for w in windows(app) { walk(w, 0, &out) }

  return out
}

func dump(_ app: AXUIElement) {
  let ws = windows(app)
  if ws.isEmpty { print("dump: no windows") }

  for w in ws {
    if children(w).isEmpty {
      print("dump: window has no children, retrying once")
      CFRunLoopRunInMode(.defaultMode, 0.5, false)
    }

    var out: [(Int, AXUIElement)] = []
    walk(w, 0, &out)
    for (d, e) in out { print(String(repeating: "  ", count: d) + line(e)) }
  }

  print("dump: \(all(app).count) elements")
}

func find(_ app: AXUIElement, _ sel: String) -> AXUIElement? {
  let parts = sel.split(separator: ":", maxSplits: 1, omittingEmptySubsequences: false)
    .map(String.init)
  let role = parts[0]
  let title = parts.count > 1 ? parts[1] : nil
  for (_, e) in all(app) {
    let roles = [attr(e, "AXRole") as? String, attr(e, "AXSubrole") as? String]
    if !role.isEmpty && !roles.contains(role) { continue }

    if let title, (attr(e, "AXTitle") as? String) != title { continue }

    return e
  }

  return nil
}

var seen: [String: Int] = [:]
let callback: AXObserverCallback = { _, e, name, _ in
  let role = (attr(e, "AXRole") as? String) ?? "gone"
  seen["\(name as String)(\(role.replacingOccurrences(of: "AX", with: "")))", default: 0] += 1
}

let args = Array(CommandLine.arguments.dropFirst())
guard let pid = args.first.flatMap({ pid_t($0) }) else {
  FileHandle.standardError.write("usage: ax-dump <pid> <op>...\n".data(using: .utf8)!)
  exit(2)
}

print("trusted: \(AXIsProcessTrusted())")
let app = AXUIElementCreateApplication(pid)
AXUIElementSetMessagingTimeout(app, 5)

var observer: AXObserver?
if AXObserverCreate(pid, callback, &observer) == .success, let observer {
  for n in [
    "AXFocusedUIElementChanged", "AXValueChanged", "AXLayoutChanged", "AXTitleChanged",
    "AXSelectedRowsChanged", "AXSelectedTextChanged", "AXSelectedChildrenChanged",
    "AXUIElementDestroyed",
  ] {
    let err = AXObserverAddNotification(observer, app, n as CFString, nil)
    if err != .success { print("observe \(n): error \(err.rawValue)") }
  }

  CFRunLoopAddSource(CFRunLoopGetCurrent(), AXObserverGetRunLoopSource(observer), .defaultMode)
}

/// "ok", or the error's code.
func result(_ err: AXError) -> String {
  err == .success ? "ok" : "error \(err.rawValue)"
}

var i = 1
func next() -> String {
  i += 1
  return args[i - 1]
}

while i < args.count {
  let op = next()
  switch op {
  case "dump":
    dump(app)
  case "sleep":
    CFRunLoopRunInMode(.defaultMode, Double(next()) ?? 1, false)
  case "count":
    let list = seen.sorted { $0.key < $1.key }.map { "\($0.key)=\($0.value)" }
    print("count: " + (list.isEmpty ? "none" : list.joined(separator: " ")))
    seen = [:]
  case "watch":
    let sel = next()
    let gone = "AXUIElementDestroyed" as CFString
    let err = find(app, sel).map { AXObserverAddNotification(observer!, $0, gone, nil) }
    print("watch \(sel): " + (err.map(result) ?? "not found"))
  case "focused":
    let e = attr(app, "AXFocusedUIElement")
    print("focused: " + (e.map { line($0 as! AXUIElement) } ?? "none"))
  case "hit":
    let sel = next()
    var r = CGRect.zero
    guard let f = find(app, sel).flatMap({ attr($0, "AXFrame") }),
      AXValueGetValue(f as! AXValue, .cgRect, &r)
    else {
      print("hit \(sel): not found")
      continue
    }

    var e: AXUIElement?
    let (x, y) = (Float(r.midX), Float(r.midY))
    let err = AXUIElementCopyElementAtPosition(AXUIElementCreateSystemWide(), x, y, &e)
    let up = e.flatMap { attr($0, "AXParent") }.map { line($0 as! AXUIElement) } ?? "none"
    let hit = e.map(line) ?? "error \(err.rawValue)"
    print("hit \(sel) at \(Int(x)),\(Int(y)): \(hit)\n  parent: \(up)")
  case "press", "inc", "dec", "focus", "set":
    let sel = next()
    let value = op == "set" ? next() : ""
    guard let e = find(app, sel) else {
      print("\(op) \(sel): not found")
      continue
    }

    let err: AXError
    switch op {
    case "press": err = AXUIElementPerformAction(e, "AXPress" as CFString)
    case "inc": err = AXUIElementPerformAction(e, "AXIncrement" as CFString)
    case "dec": err = AXUIElementPerformAction(e, "AXDecrement" as CFString)
    case "focus": err = AXUIElementSetAttributeValue(e, "AXFocused" as CFString, kCFBooleanTrue)
    default: err = AXUIElementSetAttributeValue(e, "AXValue" as CFString, value as CFString)
    }

    print("\(op) \(sel)\(op == "set" ? " \"\(value)\"" : ""): \(result(err))")
  default:
    print("unknown op \(op)")
    exit(2)
  }
}
