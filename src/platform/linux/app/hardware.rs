// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Hardware on Linux ([crate::hardware]): nothing read yet, ALSA tells no device's transport.

use crate::hardware::Transport;

pub fn transport(_id: &str) -> Transport {
  Transport::Unknown
}

pub fn lid_closed() -> Option<bool> {
  None
}
