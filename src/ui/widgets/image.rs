// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Images from RGBA pixels ([Ui::rgba_image]): added to the window's [Images] the first frame a
//! view builds one, by their pixels' hash, and dropped after a frame that doesn't, so a view
//! shows a logo without keeping an [crate::ImageId].

use std::hash::{Hash, Hasher};

use crate::scene::Images;
use crate::text::FxHasher;
use crate::ui::{Response, Style, Ui, UiState};

impl Ui<'_> {
  /// An image from straight (not premultiplied) RGBA pixels, `w * h * 4` bytes, top row first:
  /// [Ui::image] with no [Images] to keep. Added the first frame a view builds it and kept while
  /// each frame does (the same pixels share one image), then dropped. The pixels are hashed each
  /// time to tell, which suits an icon or a logo; pictures that change go through [Images].
  pub fn rgba_image(&mut self, w: usize, h: usize, rgba: &[u8], style: Style) -> Response {
    let mut hasher = FxHasher::default();
    (w, h, rgba).hash(&mut hasher);
    let key = hasher.finish();
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
