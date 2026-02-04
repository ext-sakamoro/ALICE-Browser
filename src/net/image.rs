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

impl ImageLoader {
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
    pub fn get(&self, url: &str) -> Option<&ImageData> {
        self.loaded.get(url)
    }

    /// Get all loaded image URLs.
    pub fn loaded_urls(&self) -> Vec<String> {
        self.loaded.keys().cloned().collect()
    }

    /// Number of successfully loaded images.
    pub fn loaded_count(&self) -> usize {
        self.loaded.len()
    }

    /// Number of images still being fetched.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }
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
    let img = image::load_from_memory(&bytes).ok()?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();

    // Cap to reasonable size (max 800px wide for browser)
    let (w, h, pixels) = if w > 800 {
        let ratio = 800.0 / w as f32;
        let new_h = (h as f32 * ratio) as u32;
        let resized = image::imageops::resize(&rgba, 800, new_h, image::imageops::FilterType::Triangle);
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
