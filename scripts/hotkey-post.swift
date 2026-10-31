// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo
//
// Posts key events at the HID level (kCGHIDEventTap, below every session event tap), as a
// keyboard would, and reads and clears the text of an app's focused field, for the hotkeys' checks
// in the macOS test VM (built and run by scripts/hotkey-check.py; it needs the Post Event and
// Accessibility grants of its responsible process).
//
//   hotkey-post <op>...
//
// Ops run in order: `<key>+` presses a key, `<key>*` repeats it (the autorepeat field set),
// `<key>-` releases it, each followed by the gap (`gap <s>`, 25 ms); with `!` before the key
// (`!f2-`) the event is marked as nimbo's own posted key, which its event tap lets through
// unjudged: a press or release the tap missed. A modifier (`fn`, `alt`, `ralt`, `shift`, `rshift`,
// `cmd`, `ctrl`) posts its flagsChanged and is held in the flags of the events after it; the keys
// that carry fn anyway (F1–F12) carry it. `source private|hid|session` picks the event source's
// state (private by default); `@<s>` waits; `front <pid>` makes the app frontmost,
// `frame <pid> <x> <y> <w> <h>` moves and sizes its first window; `text <pid>` prints the value of
// its focused element, `clear <pid>` empties it; `state <key>` prints whether the key is down as
// the HID system and the session have it, and the HID system's modifier flags; `secure on|off`
// turns Secure Input on or off (this process's, until it exits); `trust` prints this process's
// Accessibility, Listen Event and Post Event grants (asking for none); `release` releases every
// modifier and the keys these checks use, held or not.

import AppKit
import ApplicationServices
import Carbon
import Foundation

/// Each key's code and, for a modifier, its flags: the general one and the side's.
let keys: [String: (CGKeyCode, UInt64)] = [
  "a": (0x00, 0), "s": (0x01, 0), "d": (0x02, 0), "f": (0x03, 0), "h": (0x04, 0),
  "g": (0x05, 0), "z": (0x06, 0), "x": (0x07, 0), "c": (0x08, 0), "v": (0x09, 0),
  "b": (0x0B, 0), "q": (0x0C, 0), "w": (0x0D, 0), "e": (0x0E, 0), "r": (0x0F, 0),
  "y": (0x10, 0), "t": (0x11, 0), "1": (0x12, 0), "2": (0x13, 0), "3": (0x14, 0),
  "o": (0x1F, 0), "u": (0x20, 0), "i": (0x22, 0), "p": (0x23, 0), "l": (0x25, 0),
  "j": (0x26, 0), "k": (0x28, 0), "n": (0x2D, 0), "m": (0x2E, 0),
  "space": (0x31, 0), "escape": (0x35, 0), "return": (0x24, 0),
  "f1": (0x7A, 0), "f2": (0x78, 0), "f3": (0x63, 0), "f5": (0x60, 0),
  "fn": (0x3F, 0x80_0000),
  "alt": (0x3A, 0x08_0000 | 0x20), "ralt": (0x3D, 0x08_0000 | 0x40),
  "shift": (0x38, 0x02_0000 | 0x02), "rshift": (0x3C, 0x02_0000 | 0x04),
  "cmd": (0x37, 0x10_0000 | 0x08), "ctrl": (0x3B, 0x04_0000 | 0x01),
]
let modifiers = ["fn", "alt", "ralt", "shift", "rshift", "cmd", "ctrl"]
let fnKeys: Set<CGKeyCode> = [0x7A, 0x78, 0x63, 0x60]
var gap = 0.025
/// nimbo's mark on the keys it posts itself (`POSTED` in src/platform/macos/app/post.rs).
let posted: Int64 = 0x6E6D_626F

func fail(_ message: String) -> Never {
  FileHandle.standardError.write("hotkey-post: \(message)\n".data(using: .utf8)!)
  exit(2)
}

func key(_ name: String) -> (CGKeyCode, UInt64) {
  guard let k = keys[name] else { fail("unknown key \(name)") }

  return k
}

var source = CGEventSource(stateID: .privateState)
/// The modifiers held, as flags.
var held: UInt64 = 0

func post(_ name: String, down: Bool, repeating: Bool = false, unjudged: Bool = false) {
  let (code, flag) = key(name)
  guard let e = CGEvent(keyboardEventSource: source, virtualKey: code, keyDown: down) else {
    fail("no event for \(name)")
  }

  if flag != 0 {
    held = down ? held | flag : held & ~flag
    e.type = .flagsChanged
  }

  let fn: UInt64 = fnKeys.contains(code) ? 0x80_0000 : 0
  e.flags = CGEventFlags(rawValue: held | fn)
  if repeating { e.setIntegerValueField(.keyboardEventAutorepeat, value: 1) }

  if unjudged { e.setIntegerValueField(.eventSourceUserData, value: posted) }

  e.post(tap: .cghidEventTap)
  Thread.sleep(forTimeInterval: gap)
}

func attr(_ e: AXUIElement, _ name: String) -> AnyObject? {
  var v: AnyObject?
  return AXUIElementCopyAttributeValue(e, name as CFString, &v) == .success ? v : nil
}

func focused(_ pid: String) -> AXUIElement {
  guard let pid = pid_t(pid) else { fail("bad pid \(pid)") }

  let app = AXUIElementCreateApplication(pid)
  guard let e = attr(app, "AXFocusedUIElement") else { fail("\(pid) has no focused element") }

  return e as! AXUIElement
}

let args = Array(CommandLine.arguments.dropFirst())
var i = 0
func next() -> String {
  guard i < args.count else { fail("missing argument") }

  i += 1
  return args[i - 1]
}

while i < args.count {
  let op = next()
  switch op {
  case "front":
    let pid = next()
    let app = AXUIElementCreateApplication(pid_t(pid) ?? 0)
    let err = AXUIElementSetAttributeValue(app, "AXFrontmost" as CFString, kCFBooleanTrue)
    Thread.sleep(forTimeInterval: 0.3)
    let front = NSWorkspace.shared.frontmostApplication?.processIdentifier ?? 0
    print("front \(pid): \(err == .success ? "ok" : "error \(err.rawValue)"), frontmost \(front)")
  case "frame":
    let app = AXUIElementCreateApplication(pid_t(next()) ?? 0)
    let n = (0..<4).map { _ in Double(next()) ?? 0 }
    guard let window = (attr(app, "AXWindows") as? [AXUIElement])?.first else { fail("no window") }

    var (at, size) = (CGPoint(x: n[0], y: n[1]), CGSize(width: n[2], height: n[3]))
    let moved = AXUIElementSetAttributeValue(
      window, "AXPosition" as CFString, AXValueCreate(.cgPoint, &at)!)
    let sized = AXUIElementSetAttributeValue(
      window, "AXSize" as CFString, AXValueCreate(.cgSize, &size)!)
    print("frame: moved \(moved.rawValue), sized \(sized.rawValue) (0 is ok)")
  case "scroll":
    let app = AXUIElementCreateApplication(pid_t(next()) ?? 0)
    guard let window = (attr(app, "AXWindows") as? [AXUIElement])?.first,
      let p = attr(window, "AXPosition"), let z = attr(window, "AXSize")
    else { fail("no window") }

    var (at, size) = (CGPoint.zero, CGSize.zero)
    AXValueGetValue(p as! AXValue, .cgPoint, &at)
    AXValueGetValue(z as! AXValue, .cgSize, &size)
    let centre = CGPoint(x: at.x + size.width / 2, y: at.y + size.height / 2)
    let lines = Int32(next()) ?? 0
    let move = CGEvent(
      mouseEventSource: source, mouseType: .mouseMoved, mouseCursorPosition: centre,
      mouseButton: .left)!
    move.post(tap: .cghidEventTap)
    Thread.sleep(forTimeInterval: 0.1)
    let wheel = CGEvent(
      scrollWheelEvent2Source: source, units: .line, wheelCount: 1, wheel1: lines, wheel2: 0,
      wheel3: 0)!
    wheel.location = centre
    wheel.post(tap: .cghidEventTap)
    Thread.sleep(forTimeInterval: 0.3)
    print("scroll: \(lines) lines at \(Int(centre.x)),\(Int(centre.y))")
  case "text":
    let value = attr(focused(next()), "AXValue") as? String
    print("text: \(value.map { "\"\($0)\"" } ?? "none")")
  case "clear":
    let err = AXUIElementSetAttributeValue(focused(next()), "AXValue" as CFString, "" as CFString)
    print("clear: \(err == .success ? "ok" : "error \(err.rawValue)")")
  case "state":
    let name = next()
    let (code, _) = key(name)
    let hid = CGEventSource.keyState(.hidSystemState, key: code)
    let session = CGEventSource.keyState(.combinedSessionState, key: code)
    let flags = String(CGEventSource.flagsState(.hidSystemState).rawValue, radix: 16)
    let (h, c) = (hid ? "down" : "up", session ? "down" : "up")
    print("state \(name): hid \(h), session \(c), flags 0x\(flags)")
  case "secure":
    let on = next() == "on"
    let err = on ? EnableSecureEventInput() : DisableSecureEventInput()
    print("secure \(on ? "on" : "off"): \(err), enabled \(IsSecureEventInputEnabled())")
  case "trust":
    let (ax, listen, post) =
      (AXIsProcessTrusted(), CGPreflightListenEventAccess(), CGPreflightPostEventAccess())
    print("trust: accessibility \(ax), listen \(listen), post \(post)")
  case "release":
    for m in modifiers { post(m, down: false) }

    for k in ["z", "x", "1", "j", "space", "f2", "escape"] { post(k, down: false) }

    print("released")
  case _ where op.hasPrefix("@"):
    guard let s = Double(op.dropFirst()) else { fail("bad wait \(op)") }

    Thread.sleep(forTimeInterval: s)
  case "gap":
    guard let s = Double(next()) else { fail("bad gap") }

    gap = s
  case "source":
    let states: [String: CGEventSourceStateID] =
      ["private": .privateState, "hid": .hidSystemState, "session": .combinedSessionState]
    guard let state = states[next()] else { fail("bad source") }

    source = CGEventSource(stateID: state)
  case _ where op.count > 1 && "+*-".contains(op.last!):
    let unjudged = op.hasPrefix("!")
    let name = String(op.dropFirst(unjudged ? 1 : 0).dropLast())
    post(name, down: op.last != "-", repeating: op.last == "*", unjudged: unjudged)
  default:
    fail("unknown op \(op)")
  }
}
