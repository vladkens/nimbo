// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Images from RGBA pixels ([Ui::rgba_image]): added to the window's [Images] the first frame a
//! view builds one, by their pixels' hash, and dropped after a frame that doesn't, so a view
//! shows a logo without keeping an [crate::ImageId].

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
