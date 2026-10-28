// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The computer's hardware as an app sees it: how an audio device is attached ([transport]) and
//! whether a laptop's lid is closed ([lid_closed]), for a recorder to pick a microphone that
//! hears. Both from any thread.
//!
//! | | transport | lid |
//! |---|---|---|
//! | macOS | CoreAudio's transport type by the device's UID, the computer's own microphone told from a headset on the jack | IOPMrootDomain's `AppleClamshellState` |
//! | Windows | the endpoint's PnP enumerator by its IMMDevice id: Bluetooth or USB, else unknown | none yet |
//! | Linux | unknown yet | none yet |

use crate::platform::hardware as imp;

/// How an audio device is attached.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Transport {
  /// The computer's own microphone, not a headset on its jack: a closed lid cuts it off on Apple's
  /// laptops.
  BuiltIn,
  Usb,
  /// Recording turns a Bluetooth headset to its call profile: worse sound, the first moments lost.
  Bluetooth,
  /// Software that routes audio: a loopback, an aggregate device.
  Virtual,
  /// Attached some other way the OS tells: HDMI, Thunderbolt, a headset on the jack.
  Other,
  /// The OS doesn't tell, or no device has the id: the audio library's own description is next.
  #[default]
  Unknown,
}

/// The transport of the audio device with this id as cpal gives it: the CoreAudio UID on macOS,
/// the IMMDevice endpoint id on Windows.
pub fn transport(id: &str) -> Transport {
  imp::transport(id)
}

/// Whether the laptop's lid is closed; `None` without a lid (a desktop) and on Windows and Linux,
/// which aren't read yet.
pub fn lid_closed() -> Option<bool> {
  imp::lid_closed()
}

/// An audio device's transport on macOS by its CoreAudio `kAudioDevicePropertyTransportType`;
/// `internal` the computer's own microphone, as a headset on the jack is `'bltn'` too.
#[cfg(any(test, target_os = "macos"))]
pub(crate) fn core_audio(kind: [u8; 4], internal: bool) -> Transport {
  use Transport::*;
  match &kind {
    b"bltn" if internal => BuiltIn,
    b"usb " => Usb,
    b"blue" | b"blea" => Bluetooth,
    b"virt" | b"grup" | b"fgrp" => Virtual,
    _ => Other,
  }
}

/// An audio endpoint's transport on Windows by its PnP enumerator
/// (`DEVPKEY_Device_EnumeratorName`): Bluetooth's (`BTHENUM`, a hands-free headset's `BTHHFENUM`)
/// and USB's; the rest (`HDAUDIO`: the board's microphone and its jack alike) unknown.
#[cfg(any(test, target_os = "windows"))]
pub(crate) fn pnp(enumerator: &str) -> Transport {
  use Transport::*;
  let e = enumerator.to_ascii_uppercase();
  match e.as_str() {
    "USB" => Usb,
    _ if e.starts_with("BTH") => Bluetooth,
    _ => Unknown,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  /// The host's own devices vary: only an id no device has is sure, and the lid is read at all.
  #[test]
  fn an_unknown_device_is_unknown() {
    assert_eq!(transport("nimbo-no-such-device"), Transport::Unknown);
    let _ = lid_closed();
  }

  /// The RØDE read `'usb '` and the MacBook's microphone `'bltn'`; a headset on the jack too.
  #[test]
  fn core_audio_transports() {
    use Transport::*;
    let cases = [
      (b"bltn", true, BuiltIn),
      (b"bltn", false, Other),
      (b"usb ", false, Usb),
      (b"blue", false, Bluetooth),
      (b"blea", false, Bluetooth),
      (b"virt", false, Virtual),
      (b"grup", false, Virtual),
      (b"fgrp", false, Virtual),
      (b"hdmi", false, Other),
    ];
    for (kind, internal, want) in cases {
      assert_eq!(core_audio(*kind, internal), want, "{kind:?}");
    }
  }

  #[test]
  fn pnp_enumerators() {
    use Transport::*;
    let cases = [
      ("BTHENUM", Bluetooth),
      ("BTHHFENUM", Bluetooth),
      ("BthLEEnum", Bluetooth),
      ("USB", Usb),
      ("usb", Usb),
      ("HDAUDIO", Unknown),
      ("SWD", Unknown),
      ("", Unknown),
    ];
    for (e, want) in cases {
      assert_eq!(pnp(e), want, "{e}");
    }
  }
}
