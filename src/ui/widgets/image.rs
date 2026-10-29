// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Images from RGBA pixels ([Ui::rgba_image]) or a PNG file ([Ui::png_image]): added to the
//! window's [Images] the first frame a view builds one, by their pixels' hash, and dropped after a
//! frame that doesn't, so a view shows a logo without keeping an [crate::ImageId].

use crate::scene::Images;
use crate::ui::{Id, Response, Style, Ui, UiState};

impl Ui<'_> {
  /// An image from straight (not premultiplied) RGBA pixels, `w * h * 4` bytes, top row first:
  /// [Ui::image] with no [Images] to keep. Added the first frame a view builds it and kept while
  /// each frame does (the same pixels share one image), then dropped. The pixels are hashed each
  /// time to tell, which suits an icon or a logo; pictures that change go through [Images].
  pub fn rgba_image(&mut self, w: usize, h: usize, rgba: &[u8], style: Style) -> Response {
    let key = Id::of((w, h, rgba)).0;
    let image = match self.st.pictures.iter_mut().find(|p| p.0 == key) {
      Some(p) => {
        p.2 = true;
        p.1
      }
      None => {
        let id = self.images.add(w, h, rgba);
        self.st.pictures.push((key, id, true));
        id
      }
    };

    self.image(image, style)
  }

  /// [Ui::rgba_image] of a PNG the app ships ([crate::Png]), decoded the first frame a view
  /// builds it: `Png::new(include_bytes!("logo.png")).side(128)` for a logo 64 points wide.
  pub fn png_image(&mut self, png: crate::Png, style: Style) -> Response {
    let (w, h, px) = png.rgba();
    self.rgba_image(w as usize, h as usize, px, style)
  }
}

impl UiState {
  /// Drops the images [Ui::rgba_image] added that no view built since the last call: once a
  /// frame.
  pub(crate) fn sweep_images(&mut self, images: &mut Images) {
    self.pictures.retain_mut(|(_, id, built)| {
      if !*built {
        images.remove(*id);
      }

      std::mem::replace(built, false)
    });
  }
}
