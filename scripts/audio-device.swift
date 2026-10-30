// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo
//
// Lists and changes the audio devices through CoreAudio, to make the changes `nimbo::system`
// watches in the macOS test VM (built with `xcrun swiftc -O` on the host and copied to the guest,
// as scripts/ax-dump.swift; the guest has no toolchain).
//
//   audio-device list
//   audio-device create <uid> [<sub-device-uid>]
//   audio-device destroy <uid>
//   audio-device default <uid>
//
// `list` prints each input device as `<uid>\t<name>`, then `default\t<uid>` for the default
// input; `create` makes a public aggregate device (it outlives this process) with the sub-device
// when given, and prints its id; `destroy` removes an aggregate device; `default` makes a device
// the default input. Each exits non-zero with CoreAudio's status on failure.

import CoreAudio
import Foundation

func fail(_ message: String) -> Never {
  FileHandle.standardError.write("audio-device: \(message)\n".data(using: .utf8)!)
  exit(1)
}

func address(_ selector: AudioObjectPropertySelector, _ scope: AudioObjectPropertyScope = kAudioObjectPropertyScopeGlobal) -> AudioObjectPropertyAddress {
  AudioObjectPropertyAddress(mSelector: selector, mScope: scope, mElement: kAudioObjectPropertyElementMain)
}

func devices() -> [AudioObjectID] {
  var at = address(kAudioHardwarePropertyDevices)
  var size: UInt32 = 0
  var status = AudioObjectGetPropertyDataSize(AudioObjectID(kAudioObjectSystemObject), &at, 0, nil, &size)
  if status != noErr { fail("devices' size (\(status))") }

  var ids = [AudioObjectID](repeating: 0, count: Int(size) / MemoryLayout<AudioObjectID>.size)
  status = AudioObjectGetPropertyData(AudioObjectID(kAudioObjectSystemObject), &at, 0, nil, &size, &ids)
  if status != noErr { fail("devices (\(status))") }

  return ids
}

func string(_ id: AudioObjectID, _ selector: AudioObjectPropertySelector) -> String? {
  var at = address(selector)
  var value: Unmanaged<CFString>?
  var size = UInt32(MemoryLayout<Unmanaged<CFString>?>.size)
  guard AudioObjectGetPropertyData(id, &at, 0, nil, &size, &value) == noErr, let value else { return nil }

  return value.takeRetainedValue() as String
}

func hasInput(_ id: AudioObjectID) -> Bool {
  var at = address(kAudioDevicePropertyStreams, kAudioObjectPropertyScopeInput)
  var size: UInt32 = 0
  return AudioObjectGetPropertyDataSize(id, &at, 0, nil, &size) == noErr && size > 0
}

func device(_ uid: String) -> AudioObjectID {
  guard let id = devices().first(where: { string($0, kAudioDevicePropertyDeviceUID) == uid }) else {
    fail("no device \(uid)")
  }

  return id
}

func defaultInput() -> AudioObjectID {
  var at = address(kAudioHardwarePropertyDefaultInputDevice)
  var id = AudioObjectID(0)
  var size = UInt32(MemoryLayout<AudioObjectID>.size)
  let status = AudioObjectGetPropertyData(AudioObjectID(kAudioObjectSystemObject), &at, 0, nil, &size, &id)
  if status != noErr { fail("default input (\(status))") }

  return id
}

let args = Array(CommandLine.arguments.dropFirst())
switch (args.first, args.count) {
case ("list", 1):
  for id in devices() where hasInput(id) {
    print("\(string(id, kAudioDevicePropertyDeviceUID) ?? "?")\t\(string(id, kAudioObjectPropertyName) ?? "?")")
  }

  let id = defaultInput()
  print("default\t\(id == kAudioObjectUnknown ? "none" : string(id, kAudioDevicePropertyDeviceUID) ?? "?")")

case ("create", 2), ("create", 3):
  var desc: [String: Any] = [
    kAudioAggregateDeviceUIDKey: args[1],
    kAudioAggregateDeviceNameKey: args[1],
    kAudioAggregateDeviceIsPrivateKey: 0,
  ]
  if args.count == 3 {
    desc[kAudioAggregateDeviceSubDeviceListKey] = [[kAudioSubDeviceUIDKey: args[2]]]
    desc[kAudioAggregateDeviceMainSubDeviceKey] = args[2]
  }

  var id = AudioObjectID(0)
  let status = AudioHardwareCreateAggregateDevice(desc as CFDictionary, &id)
  if status != noErr { fail("create \(args[1]) (\(status))") }

  print(id)

case ("destroy", 2):
  let status = AudioHardwareDestroyAggregateDevice(device(args[1]))
  if status != noErr { fail("destroy \(args[1]) (\(status))") }

case ("default", 2):
  var id = device(args[1])
  var at = address(kAudioHardwarePropertyDefaultInputDevice)
  let size = UInt32(MemoryLayout<AudioObjectID>.size)
  let status = AudioObjectSetPropertyData(AudioObjectID(kAudioObjectSystemObject), &at, 0, nil, size, &id)
  if status != noErr { fail("default \(args[1]) (\(status))") }

default:
  fail("usage: audio-device list | create <uid> [<sub-device-uid>] | destroy <uid> | default <uid>")
}
