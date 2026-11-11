# Photos through the OS's image decoders

Done: 2026-11-11

- [x] `Bitmap`: premultiplied RGBA pixels decoded by the OS (`Bitmap::decode(bytes, side)`), from any thread, errors as values
- [x] macOS: ImageIO's thumbnail at the side, EXIF orientation applied, drawn in sRGB (raw FFI, no new crate)
- [x] Windows: WIC from memory, scaled (Fant), converted to 32bppPRGBA, turned by `System.Photo.Orientation` in Rust (`orient`)
- [x] Linux: gdk-pixbuf loaded at run time, a `GdkPixbufLoader` sized in `size-prepared`, `gdk_pixbuf_apply_embedded_orientation`
- [x] `ui.image` takes an `ImageId`, a `Png` or a `&Bitmap`; `png_image` folded into it
- [x] Shown bitmaps share their pixels with the image store and the renderer (`Arc<Vec<u8>>`), decoders allocate them once
- [x] `ui.aspect_ratio(ContentMode::Fit | Fill)` for the next image: fit letterboxes, fill covers and crops (by the texture rect); one side set (or none) gives the other in its aspect
- [x] Limits read from the header (a gigapixel declared, 128 Mi pixels decoded), fallible allocation
- [x] Fixtures in `tests/images/`, unit tests (size, colours, orientation, downscale, a corrupt file), harness tests, the gallery's entry
- [x] Measured: a 6000 × 4000 JPEG to 300 px and at full size, time and peak memory
- [x] Linux run in a container; Windows compiled by clippy
- [x] `make check`, `make test`

## Result

`Bitmap::decode(&file, side)` decodes JPEG, PNG, GIF's first frame, and HEIC and WebP where the OS reads them, upright and at most `side` pixels on the longer side; `ui.image(&bitmap, style)` shows it. No new crate (13 / 4 / 2). A 6000 × 4000 JPEG (4.9 MB) on an M-series Mac: to 300 px in 25 ms and ~3.6 MB above the file, at full size in 83 ms and ~290 MB; in an arm64 Linux container 31 ms and ~5 MB against 101 ms and ~190 MB. A shown photo is held once: the app's `Bitmap`, the image store and the renderer share one `Arc<Vec<u8>>` (the renderer cloned every image's pixels before). `ui.aspect_ratio(ContentMode::Fit)` letterboxes an image in its box, `Fill` covers it, cropping the texture rect rather than clipping. Decoding refuses a file declaring over a gigapixel or a result over 128 Mi pixels, read from the header first, and allocates fallibly. Open: WIC never ran (no Windows machine), and with it whether its HEIF decoder turns a picture by `irot`/`imir` and still reports the orientation (turned twice); colour profiles are converted on macOS only; a GIF on Windows is its first frame alone, not placed in its screen.
