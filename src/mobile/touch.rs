//! Touch Gesture Recognition
//!
//! Recognizes mobile gestures from raw touch events:
//! - Tap: quick touch + release
//! - Double-tap: two taps within 300ms → zoom
//! - Long-press: hold > 500ms → link preview
//! - Swipe left from edge → back
//! - Swipe right from edge → forward
//! - Swipe up → hide bottom bar (fullscreen)
//! - Swipe down → show status bar
//! - Pinch → zoom (two-finger)

use std::time::Instant;

/// Touch point
#[derive(Debug, Clone, Copy)]
pub struct TouchPoint {
    pub x: f32,
    pub y: f32,
    pub id: u64,
    pub time: Instant,
}

/// Recognized gesture
#[derive(Debug, Clone)]
pub enum Gesture {
    /// Single tap at position
    Tap { x: f32, y: f32 },
    /// Double-tap at position → zoom toggle
    DoubleTap { x: f32, y: f32 },
    /// Long press at position → link preview
    LongPress { x: f32, y: f32 },
    /// Swipe with direction and velocity
    Swipe { direction: SwipeDirection, velocity: f32 },
    /// Pinch zoom with scale factor
    Pinch { scale: f32, center_x: f32, center_y: f32 },
    /// Scroll (drag) with delta
    Scroll { dx: f32, dy: f32 },
    /// No gesture detected yet
    None,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SwipeDirection {
    Left,
    Right,
    Up,
    Down,
}

/// Touch gesture state machine.
///
/// Processes raw touch events and emits high-level gestures.
/// Uses branchless state transitions where possible.
pub struct GestureRecognizer {
    /// Current active touches
    touches: Vec<TouchPoint>,
    /// First touch point (for gesture start)
    start_point: Option<TouchPoint>,
    /// Last tap time (for double-tap detection)
    last_tap_time: Option<Instant>,
    /// Last tap position
    last_tap_pos: Option<(f32, f32)>,
    /// Long press threshold in milliseconds
    long_press_ms: u64,
    /// Double tap threshold in milliseconds
    double_tap_ms: u64,
    /// Minimum swipe distance in pixels
    swipe_threshold: f32,
    /// Edge zone width for back/forward gestures
    edge_zone: f32,
    /// Screen dimensions
    pub screen_width: f32,
    pub screen_height: f32,
    /// Whether a gesture is in progress
    is_dragging: bool,
    /// Total drag distance (for distinguishing tap from scroll)
    drag_distance: f32,
}

impl GestureRecognizer {
    pub fn new(screen_width: f32, screen_height: f32) -> Self {
        Self {
            touches: Vec::with_capacity(4),
            start_point: None,
            last_tap_time: None,
            last_tap_pos: None,
            long_press_ms: 500,
            double_tap_ms: 300,
            swipe_threshold: 50.0,
            edge_zone: 30.0,
            screen_width,
            screen_height,
            is_dragging: false,
            drag_distance: 0.0,
        }
    }

    pub fn set_screen_size(&mut self, width: f32, height: f32) {
        self.screen_width = width;
        self.screen_height = height;
    }

    /// Process touch start event
    pub fn touch_start(&mut self, x: f32, y: f32, id: u64) {
        let point = TouchPoint {
            x, y, id,
            time: Instant::now(),
        };
        self.touches.push(point);
        if self.touches.len() == 1 {
            self.start_point = Some(point);
            self.is_dragging = false;
            self.drag_distance = 0.0;
        }
    }

    /// Process touch move event. Returns Scroll gesture for drag.
    pub fn touch_move(&mut self, x: f32, y: f32, id: u64) -> Gesture {
        // Find and update the touch point
        if let Some(touch) = self.touches.iter_mut().find(|t| t.id == id) {
            let dx = x - touch.x;
            let dy = y - touch.y;

            // Track total drag distance for tap vs scroll disambiguation
            self.drag_distance += (dx * dx + dy * dy).sqrt(); // Using actual sqrt here is fine (rare path)

            touch.x = x;
            touch.y = y;

            // Two-finger pinch detection
            if self.touches.len() == 2 {
                let t0 = self.touches[0];
                let t1 = self.touches[1];
                let current_dist = ((t0.x - t1.x).powi(2) + (t0.y - t1.y).powi(2)).sqrt();

                if let Some(start) = &self.start_point {
                    let start_dist = ((start.x - t1.x).powi(2) + (start.y - t1.y).powi(2)).sqrt();
                    if start_dist > 1.0 {
                        let scale = current_dist / start_dist;
                        let cx = (t0.x + t1.x) * 0.5;
                        let cy = (t0.y + t1.y) * 0.5;
                        return Gesture::Pinch { scale, center_x: cx, center_y: cy };
                    }
                }
            }

            // Single finger drag → scroll
            if self.drag_distance > 10.0 {
                self.is_dragging = true;
                return Gesture::Scroll { dx, dy };
            }
        }

        Gesture::None
    }

    /// Process touch end event. Returns the recognized gesture.
    pub fn touch_end(&mut self, x: f32, y: f32, id: u64) -> Gesture {
        self.touches.retain(|t| t.id != id);

        let start = match self.start_point.take() {
            Some(s) => s,
            None => return Gesture::None,
        };

        let duration = start.time.elapsed();
        let dx = x - start.x;
        let dy = y - start.y;
        let dist = (dx * dx + dy * dy).sqrt();

        // Long press detection
        if duration.as_millis() as u64 >= self.long_press_ms && dist < self.swipe_threshold {
            return Gesture::LongPress { x, y };
        }

        // Swipe detection
        if dist >= self.swipe_threshold {
            let velocity = dist / duration.as_secs_f32().max(0.001);
            let abs_dx = dx.abs();
            let abs_dy = dy.abs();

            let direction = if abs_dx > abs_dy {
                // Horizontal swipe
                if dx > 0.0 {
                    // Swipe right — if started from left edge, it's "back"
                    if start.x < self.edge_zone {
                        SwipeDirection::Right // Back gesture
                    } else {
                        SwipeDirection::Right
                    }
                } else {
                    // Swipe left — if started from right edge, it's "forward"
                    if start.x > self.screen_width - self.edge_zone {
                        SwipeDirection::Left // Forward gesture
                    } else {
                        SwipeDirection::Left
                    }
                }
            } else {
                // Vertical swipe
                if dy > 0.0 {
                    SwipeDirection::Down // Show status bar
                } else {
                    SwipeDirection::Up // Hide bottom bar (fullscreen)
                }
            };

            return Gesture::Swipe { direction, velocity };
        }

        // Tap detection (short touch, no significant movement)
        if dist < 20.0 && duration.as_millis() < self.long_press_ms as u128 {
            // Check for double-tap
            if let (Some(last_time), Some(last_pos)) = (self.last_tap_time, self.last_tap_pos) {
                let time_diff = last_time.elapsed().as_millis() as u64;
                let pos_dist = ((x - last_pos.0).powi(2) + (y - last_pos.1).powi(2)).sqrt();

                if time_diff < self.double_tap_ms && pos_dist < 50.0 {
                    self.last_tap_time = None;
                    self.last_tap_pos = None;
                    return Gesture::DoubleTap { x, y };
                }
            }

            self.last_tap_time = Some(Instant::now());
            self.last_tap_pos = Some((x, y));
            return Gesture::Tap { x, y };
        }

        Gesture::None
    }

    /// Check for long press (called periodically from UI update loop)
    pub fn check_long_press(&self) -> Option<(f32, f32)> {
        if self.touches.len() == 1 && !self.is_dragging {
            let touch = &self.touches[0];
            if touch.time.elapsed().as_millis() as u64 >= self.long_press_ms {
                return Some((touch.x, touch.y));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn test_tap_gesture() {
        let mut gr = GestureRecognizer::new(400.0, 800.0);
        gr.touch_start(200.0, 400.0, 1);
        let gesture = gr.touch_end(201.0, 401.0, 1);
        match gesture {
            Gesture::Tap { x, y } => {
                assert!((x - 201.0).abs() < 1.0);
                assert!((y - 401.0).abs() < 1.0);
            }
            _ => panic!("Expected Tap gesture, got {:?}", gesture),
        }
    }

    #[test]
    fn test_swipe_right() {
        let mut gr = GestureRecognizer::new(400.0, 800.0);
        gr.touch_start(10.0, 400.0, 1); // Start from left edge
        let gesture = gr.touch_end(200.0, 400.0, 1);
        match gesture {
            Gesture::Swipe { direction, .. } => {
                assert_eq!(direction, SwipeDirection::Right);
            }
            _ => panic!("Expected Swipe gesture, got {:?}", gesture),
        }
    }
}
