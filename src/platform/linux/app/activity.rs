// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Activities on Linux ([crate::activity]): none yet; the desktops don't slow a background app.

pub struct Token;

pub fn begin(_reason: &str) -> Token {
  Token
}

pub fn end(_: Token) {}
