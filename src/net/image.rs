//! Asynchronous image fetcher.
//!
//! Spawns background threads to download images and decode them
//! into RGBA pixel buffers ready for egui texture creation.

use std::collections::HashMap;
use std::sync::mpsc;

/// Decoded image data (RGBA).
pub struct ImageData {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Manages background image fetching and decoding.
pub struct ImageLoader {
    pending: HashMap<String, mpsc::Receiver<Option<ImageData>>>,
    loaded: HashMap<String, ImageData>,
    failed: std::collections::HashSet<String>,
}

impl Default for ImageLoader {
    fn default() -> Self {
        Self::new()
    }
}

impl ImageLoader {
    #[must_use]
    pub fn new() -> Self {
        Self {
            pending: HashMap::new(),
            loaded: HashMap::new(),
            failed: std::collections::HashSet::new(),
        }
    }

    /// Request an image to be fetched in the background.
    pub fn request(&mut self, url: &str) {
        if self.loaded.contains_key(url)
            || self.pending.contains_key(url)
            || self.failed.contains(url)
        {
            return;
        }

        let (tx, rx) = mpsc::channel();
        let url_owned = url.to_string();

        std::thread::spawn(move || {
            let result = fetch_and_decode(&url_owned);
            let _ = tx.send(result);
        });

        self.pending.insert(url.to_string(), rx);
    }

    /// Poll for completed downloads. Call every frame.
    pub fn poll(&mut self) {
        let mut completed = Vec::new();
        for (url, rx) in &self.pending {
            if let Ok(result) = rx.try_recv() {
                match result {
                    Some(data) => {
                        self.loaded.insert(url.clone(), data);
                    }
                    None => {
                        self.failed.insert(url.clone());
                    }
                }
                completed.push(url.clone());
            }
        }
        for url in completed {
            self.pending.remove(&url);
        }
    }

    /// Get a loaded image's data.
    #[must_use]
    pub fn get(&self, url: &str) -> Option<&ImageData> {
        self.loaded.get(url)
    }

    /// Get all loaded image URLs.
    #[must_use]
    pub fn loaded_urls(&self) -> Vec<String> {
        self.loaded.keys().cloned().collect()
    }

    /// Number of successfully loaded images.
    #[must_use]
    pub fn loaded_count(&self) -> usize {
        self.loaded.len()
    }

    /// Number of images still being fetched.
    #[must_use]
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }
}

/// The most bytes the decoder may allocate for one image fetched from the web (256 MiB, the ecosystem's limit for one
/// decode)
const MAX_WEB_IMAGE_ALLOC: u64 = 256 * 1024 * 1024;

/// The widest and tallest image accepted from the web (16384 pixels, a common texture limit)
const MAX_WEB_IMAGE_SIDE: u32 = 16_384;

/// Decodes an image fetched from the web (not trusted) within explicit limits: at most [`MAX_WEB_IMAGE_SIDE`] pixels
/// on a side and [`MAX_WEB_IMAGE_ALLOC`] bytes allocated, both refused from the header before any pixel is decoded
fn decode_web_image(bytes: &[u8]) -> Result<image::DynamicImage, image::ImageError> {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_WEB_IMAGE_SIDE);
    limits.max_image_height = Some(MAX_WEB_IMAGE_SIDE);
    limits.max_alloc = Some(MAX_WEB_IMAGE_ALLOC);
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?;
    reader.limits(limits);
    reader.decode()
}

fn fetch_and_decode(url: &str) -> Option<ImageData> {
    let resp = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .ok()?
        .get(url)
        .send()
        .ok()?;

    if !resp.status().is_success() {
        return None;
    }

    let bytes = resp.bytes().ok()?;
    let img = decode_web_image(&bytes).ok()?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();

    // Cap to reasonable size (max 800px wide for browser)
    let (w, h, pixels) = if w > 800 {
        let ratio = 800.0 / w as f32;
        let new_h = (h as f32 * ratio) as u32;
        let resized =
            image::imageops::resize(&rgba, 800, new_h, image::imageops::FilterType::Triangle);
        let (rw, rh) = resized.dimensions();
        (rw, rh, resized.into_raw())
    } else {
        (w, h, rgba.into_raw())
    };

    Some(ImageData {
        width: w,
        height: h,
        rgba: pixels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loader_deduplicates() {
        let mut loader = ImageLoader::new();
        loader.request("https://example.com/img.png");
        loader.request("https://example.com/img.png"); // should not duplicate
        assert_eq!(loader.pending.len(), 1);
    }
}

#[cfg(test)]
mod decode_limit_tests {
    use super::decode_web_image;

    fn crc32(bytes: &[u8]) -> u32 {
        let mut c = 0xFFFF_FFFFu32;
        for &b in bytes {
            c ^= u32::from(b);
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
        }
        !c
    }

    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&u32::try_from(data.len()).unwrap().to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc32(&body).to_be_bytes());
    }

    /// A PNG of `w` x `h` RGBA 8-bit whose image data is an empty zlib stream (the png decoder reads up to the first
    /// IDAT before it reports the size, so the chunk has to be there for the limits to be reached)
    fn header_only_png(w: u32, h: u32) -> Vec<u8> {
        let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&w.to_be_bytes());
        ihdr.extend_from_slice(&h.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
        chunk(&mut out, b"IHDR", &ihdr);
        chunk(
            &mut out,
            b"IDAT",
            &[0x78, 0x9c, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01],
        );
        chunk(&mut out, b"IEND", &[]);
        out
    }

    #[test]
    fn an_image_past_the_limits_is_refused_from_its_header() {
        // a page's image is not trusted: 10000 x 10000 RGBA (400 MB, within image's default 512 MiB) and 20000 x 20
        // (wider than the 16384 limit) are refused from the header, before any pixel is decoded
        let mut compared = 0;
        for (w, h) in [(10_000, 10_000), (20_000, 20)] {
            let e = decode_web_image(&header_only_png(w, h)).unwrap_err();
            assert!(matches!(e, image::ImageError::Limits(_)), "{w}x{h}: {e}");
            compared += 1;
        }
        assert_eq!(compared, 2);
    }

    #[test]
    fn an_image_within_the_limits_reaches_the_decoder() {
        // 4000 x 3000 RGBA is 48 MB: the header passes, and the missing image data then fails in the decoder
        let e = decode_web_image(&header_only_png(4000, 3000)).unwrap_err();
        assert!(!matches!(e, image::ImageError::Limits(_)), "{e}");
    }
}
