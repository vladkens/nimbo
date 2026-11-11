// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Images ([Ui::image]): one the app keeps in [Images], a PNG it ships ([Png]), a picture the
//! OS decoded ([Bitmap]), or RGBA pixels ([Ui::rgba_image]). The last three are added to the
//! window's [Images] the first frame a view builds one and dropped after a frame that doesn't, so
//! a view shows a logo or a photo without keeping an [ImageId].

use crate::bitmap::Bitmap;
use crate::png::Png;
use crate::scene::{ImageId, Images, Rect};
use crate::ui::{Flow, Id, Kind, Response, Style, Ui, UiState};

/// What [Ui::image] shows, as SwiftUI's `Image` is made from a name or an `NSImage`: from an
/// [ImageId], a [Png] or a `&`[Bitmap].
#[derive(Clone, Copy)]
pub enum ImageSource<'a> {
  /// An image the app added to [Images] and keeps.
  Id(ImageId),
  /// A PNG the app ships, decoded the first time ([Png::rgba]).
  Png(Png),
  /// A picture the OS decoded ([Bitmap::decode]).
  Bitmap(&'a Bitmap),
}

/// How an image fills the box it is given ([Ui::aspect_ratio]), as SwiftUI's `ContentMode`;
/// without one it is stretched to the box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentMode {
  /// Whole and in its own aspect, centred: letterboxed where the box has another (`scaledToFit`).
  Fit,
  /// Covering the box in its own aspect, centred, the overflow cut off (`scaledToFill`).
  Fill,
}

impl From<ImageId> for ImageSource<'_> {
  fn from(id: ImageId) -> Self {
    Self::Id(id)
  }
}

impl From<Png> for ImageSource<'_> {
  fn from(png: Png) -> Self {
    Self::Png(png)
  }
}

impl<'a> From<&'a Bitmap> for ImageSource<'a> {
  fn from(b: &'a Bitmap) -> Self {
    Self::Bitmap(b)
  }
}

impl Ui<'_> {
  /// An image, stretched to the box `style` gives it; a fit size is its pixel size over the
  /// backing scale. From an [ImageId], a [Png] the app ships (`Png::new(include_bytes!
  /// ("logo.png")).side(128)` for a logo 64 points wide) or a photo the OS decoded at the size it
  /// shows (`ui.image(&self.photo, Style::new().w(px(300.0)).h(px(200.0)))` of a
  /// `Bitmap::decode(&file, 600)?`).
  pub fn image<'b>(&mut self, image: impl Into<ImageSource<'b>>, style: Style) -> Response {
    let id = match image.into() {
      ImageSource::Id(id) => id,
      ImageSource::Png(png) => {
        let (w, h, px) = png.rgba();
        self.picture(Id::of((w as usize, h as usize, px)).0, |i| i.add(w as usize, h as usize, px))
      }
      ImageSource::Bitmap(b) => {
        let (w, h) = (b.width() as usize, b.height() as usize);
        self.picture(b.key, |i| i.add_premultiplied(w, h, b.px.clone()))
      }
    };

    let mode = self.set_slot::<ContentMode>(None);
    self.push(Kind::Image(id, mode), style, Flow::Row).1
  }

  /// How the next image fills its box, in its own aspect, as SwiftUI's
  /// `.aspectRatio(contentMode:)`: `ui.aspect_ratio(ContentMode::Fill).image(&photo, style)` for
  /// a thumbnail that covers its square. With one side set, or none, the box takes the image's
  /// aspect for the other, as SwiftUI's frame does (a 300 × 200 picture 150 points wide is 100
  /// high); with both set the image fits in or fills that box.
  pub fn aspect_ratio(&mut self, mode: ContentMode) -> &mut Self {
    self.set_slot(Some(mode));
    self
  }

  /// An image from straight (not premultiplied) RGBA pixels, `w * h * 4` bytes, top row first:
  /// [Ui::image] with no [Images] to keep. Added the first frame a view builds it and kept while
  /// each frame does (the same pixels share one image), then dropped. The pixels are hashed each
  /// time to tell, which suits an icon or a logo; pictures that change go through [Images].
  pub fn rgba_image(&mut self, w: usize, h: usize, rgba: &[u8], style: Style) -> Response {
    let id = self.picture(Id::of((w, h, rgba)).0, |i| i.add(w, h, rgba));
    self.image(id, style)
  }

  /// The image of `key` this frame: the one added before, or `add`ed now.
  fn picture(&mut self, key: u64, add: impl FnOnce(&mut Images) -> ImageId) -> ImageId {
    if let Some(p) = self.st.pictures.iter_mut().find(|p| p.0 == key) {
      p.2 = true;
      return p.1;
    }

    let id = add(&mut self.images);
    self.st.pictures.push((key, id, true));
    id
  }
}

/// Where an image of `w` × `h` pixels draws in its box `r` as `mode` says, and the part of it
/// shown there (in pixels).
pub(crate) fn placed(mode: Option<ContentMode>, r: Rect, w: f32, h: f32) -> (Rect, Rect) {
  let all = Rect::new(0.0, 0.0, w, h);
  let (sx, sy) = (r.w / w, r.h / h);
  match mode {
    None => (r, all),
    Some(ContentMode::Fit) => {
      let (dw, dh) = (w * sx.min(sy), h * sx.min(sy));
      (Rect::new(r.x + (r.w - dw) / 2.0, r.y + (r.h - dh) / 2.0, dw, dh), all)
    }
    Some(ContentMode::Fill) => {
      let (uw, uh) = (r.w / sx.max(sy), r.h / sx.max(sy));
      (r, Rect::new((w - uw) / 2.0, (h - uh) / 2.0, uw, uh))
    }
  }
}

impl UiState {
  /// Drops the images [Ui::image] and [Ui::rgba_image] added that no view built since the last
  /// call: once a frame.
  pub(crate) fn sweep_images(&mut self, images: &mut Images) {
    self.pictures.retain_mut(|(_, id, built)| {
      if !*built {
        images.remove(*id);
      }

      std::mem::replace(built, false)
    });
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{App, Harness, Size, px};

  const RED: [u8; 4] = [255, 0, 0, 255];
  const GREEN: [u8; 4] = [0, 255, 0, 255];
  const BLUE: [u8; 4] = [0, 0, 255, 255];
  const NONE: [u8; 4] = [0, 0, 0, 255];

  /// An 8 × 2 picture (red, then green, then blue columns: 2, 4, 2) in a box of the style.
  struct Wide(Option<ContentMode>, Style);

  impl App for Wide {
    fn view(&mut self, ui: &mut Ui) {
      let row = [[RED; 2], [GREEN; 2], [GREEN; 2], [BLUE; 2]].concat().concat();
      if let Some(mode) = self.0 {
        ui.aspect_ratio(mode);
      }

      ui.id("pic").rgba_image(8, 2, &[row.clone(), row].concat(), self.1);
    }
  }

  /// The pixels at the left, middle and right of the square's middle row, and at its top.
  fn shown(mode: Option<ContentMode>) -> [[u8; 4]; 4] {
    let square = Style::new().w(px(20.0)).h(px(20.0));
    let mut h = Harness::new(Wide(mode, square), Size::new(20.0, 20.0), 1.0);
    h.frame();
    let img = h.render();
    [img.pixel(1.0, 10.0), img.pixel(10.0, 10.0), img.pixel(19.0, 10.0), img.pixel(10.0, 2.0)]
  }

  /// Fit: whole, letterboxed above and below.
  #[test]
  fn fit_letterboxes_in_its_aspect() {
    assert_eq!(shown(Some(ContentMode::Fit)), [RED, GREEN, BLUE, NONE]);
  }

  /// Fill: the square covered by the middle of the picture, its sides cut off.
  #[test]
  fn fill_covers_and_cuts_off() {
    assert_eq!(shown(Some(ContentMode::Fill)), [GREEN; 4]);
  }

  /// The box's size with the image built in `style`.
  fn laid_out(mode: Option<ContentMode>, style: Style) -> (f32, f32) {
    let mut h = Harness::new(Wide(mode, style), Size::new(100.0, 100.0), 1.0);
    h.frame();
    let r = h.rect(Id::of("pic")).unwrap();
    (r.w, r.h)
  }

  /// In its aspect, a side set gives the other; both set are kept; without a mode a side set
  /// leaves the other the picture's own.
  #[test]
  fn a_side_set_gives_the_other_in_its_aspect() {
    let fit = Some(ContentMode::Fit);
    assert_eq!(laid_out(fit, Style::new().w(px(40.0))), (40.0, 10.0));
    assert_eq!(laid_out(fit, Style::new().h(px(10.0))), (40.0, 10.0));
    assert_eq!(laid_out(Some(ContentMode::Fill), Style::new().w(px(20.0))), (20.0, 5.0));
    assert_eq!(laid_out(fit, Style::new()), (8.0, 2.0));
    assert_eq!(laid_out(fit, Style::new().w(px(20.0)).h(px(20.0))), (20.0, 20.0));
    assert_eq!(laid_out(None, Style::new().w(px(40.0))), (40.0, 2.0));
  }

  /// Without a mode: stretched to the box.
  #[test]
  fn stretches_by_default() {
    assert_eq!(shown(None), [RED, GREEN, BLUE, GREEN]);
  }
}
