//! ALICE Browser Mobile — Touch-First UI
//!
//! Mobile-specific features gated behind `#[cfg(feature = "mobile")]`:
//! - Touch gesture recognition (swipe, pinch, long-press, double-tap)
//! - Bottom operation bar (thumb-friendly)
//! - Fullscreen mode with auto-hide UI
//! - Block statistics overlay

pub mod touch;
pub mod ui;
