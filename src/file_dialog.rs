// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The system's file panel, asked for by a window ([Ui::file_dialog]): an
//! open or a save panel over it, the window drawing on, its answer coming back to the window as
//! [crate::Event::FileDialog]. The harness answers it as a test says
//! ([crate::Harness::file_dialog]).
//!
//! Per OS: macOS `NSOpenPanel` / `NSSavePanel` as a sheet; Windows `IFileOpenDialog` /
//! `IFileSaveDialog` owned by the window; Linux the XDG desktop portal's `FileChooser`.

use std::path::PathBuf;

use crate::Ui;

/// An open or a save panel, plain data: `FileDialog::open("Import").types(&["tar"])`,
/// `FileDialog::save("Export").name("history.tar")`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileDialog {
  /// A save panel, else an open one.
  pub save: bool,
  pub title: String,
  /// The extensions it takes, without the dot; any file when empty.
  pub types: Vec<String>,
  /// The folder it starts in; the OS's choice (the last one used) when `None`.
  pub dir: Option<PathBuf>,
  /// The name a save panel suggests.
  pub name: Option<String>,
}

impl FileDialog {
  pub fn open(title: &str) -> Self {
    Self { title: title.into(), ..Self::default() }
  }

  pub fn save(title: &str) -> Self {
    Self { save: true, ..Self::open(title) }
  }

  pub fn types(self, types: &[&str]) -> Self {
    Self { types: types.iter().map(|t| t.to_string()).collect(), ..self }
  }

  pub fn name(self, name: &str) -> Self {
    Self { name: Some(name.into()), ..self }
  }
}

impl Ui<'_> {
  /// Shows the system's file panel over the window once this frame is built: from the person's
  /// click, never by itself. The answer comes as [crate::Event::FileDialog]. Asked while one is
  /// open, it is dropped on macOS and Windows and replaces it on Linux, where a portal that went
  /// away never answers.
  pub fn file_dialog(&mut self, dialog: FileDialog) {
    self.st.file_dialog = Some(dialog);
  }
}
