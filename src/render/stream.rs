/// "The Rotunda" — Rotating Panorama Hall for OZ Mode.
///
/// 「あなたは静止し、世界が回る。」
///
/// User stands at the center (0,0,0) of a cylindrical hall. Text is mounted
/// on the inner wall at radius R, rotating in three parallax layers:
///
/// - **Upper Ring**: Trend words / headings. Slow rotation.
/// - **Eye Level**: News / content. Comfortable reading speed.
/// - **Lower Ring**: Tags / details. Fast reverse rotation.
///
/// All text faces the center (billboarding), so it's always readable.
/// Drag to look around; click to grab & inspect.

use crate::render::layout::LayoutNode;
use crate::render::sdf_ui::SdfScene;

// ── Category ──

#[derive(Debug, Clone)]
pub struct StreamCategory {
    pub name: String,
    pub color: [f32; 4],
}

// ── TextMeta: rich info from the DOM ──

#[derive(Debug, Clone)]
pub struct TextMeta {
    /// Short display text
    pub display: String,
    /// Full original text
    pub full_text: String,
    /// HTML tag (h1, a, p, etc.)
    pub tag: String,
    /// Link URL if this is an <a> tag
    pub href: Option<String>,
    /// Category index
    pub category_index: usize,
    /// Importance score
    pub importance: f32,
}

// ── GrabbedInfo ──

pub struct GrabbedInfo<'a> {
    pub particle: &'a TextParticle,
    pub meta: &'a TextMeta,
    pub category_name: &'a str,
}

// ── Rotunda Layer ──

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RotundaLayer {
    /// Trend words, headings — slow rotation
    Upper,
    /// Main content — comfortable reading speed
    Eye,
    /// Tags, details — fast reverse rotation
    Lower,
}

// ── TextParticle ──

#[derive(Debug, Clone)]
pub struct TextParticle {
    pub text: String,
    /// Base angle on the cylinder wall (radians, 0–2π)
    pub angle: f32,
    /// Y position on the cylinder wall
    pub y_pos: f32,
    /// Time alive (seconds)
    pub age: f32,
    /// Total lifetime before respawn (seconds)
    pub lifetime: f32,
    pub category_index: usize,
    /// 0.0 = niche, 1.0 = trending
    pub importance: f32,
    /// If true, particle is frozen
    pub grabbed: bool,
    pub id: usize,
    /// Index into the text_pool for rich metadata
    pub pool_index: usize,
    /// Which layer (Upper / Eye / Lower)
    pub layer: RotundaLayer,
    /// Slot within the layer
    pub slot_index: usize,
}

// ── StreamState ──

#[derive(Debug, Clone)]
pub struct StreamState {
    pub particles: Vec<TextParticle>,
    pub categories: Vec<StreamCategory>,
    pub text_pool: Vec<TextMeta>,
    pool_cursor: usize,
    next_id: usize,
    /// Elapsed time
    pub time: f32,
    /// Currently grabbed particle
    pub grabbed_index: Option<usize>,
}

// ── Constants ──

/// Radius of the rotunda wall
pub const ROTUNDA_RADIUS: f32 = 12.0;

/// Upper ring: y range and rotation speed (rad/s)
const UPPER_Y_MIN: f32 = 3.0;
const UPPER_Y_MAX: f32 = 5.5;
const UPPER_SPEED: f32 = 0.08;
const UPPER_SLOTS: usize = 16;

/// Eye-level ring: y range and rotation speed
const EYE_Y_MIN: f32 = -1.8;
const EYE_Y_MAX: f32 = 1.8;
const EYE_SPEED: f32 = 0.20;
const EYE_SLOTS: usize = 24;
/// Number of rows at eye level
const EYE_ROWS: usize = 3;

/// Lower ring: y range and rotation speed (negative = reverse)
const LOWER_Y_MIN: f32 = -5.5;
const LOWER_Y_MAX: f32 = -3.0;
const LOWER_SPEED: f32 = -0.35;
const LOWER_SLOTS: usize = 20;

/// Lifecycle
const LIFETIME_MIN: f32 = 15.0;
const LIFETIME_MAX: f32 = 30.0;
const FADE_IN_DURATION: f32 = 1.5;
const FADE_OUT_DURATION: f32 = 2.5;

/// Angular jitter
const ANGULAR_JITTER: f32 = 0.04;
/// Y jitter
const Y_JITTER: f32 = 0.15;

/// Category colors — dark/saturated for white background
const CATEGORY_COLORS: &[[f32; 4]] = &[
    [0.75, 0.12, 0.12, 1.0], // Dark Red
    [0.08, 0.30, 0.70, 1.0], // Dark Blue
    [0.65, 0.50, 0.00, 1.0], // Dark Gold
    [0.08, 0.50, 0.22, 1.0], // Dark Green
    [0.50, 0.12, 0.65, 1.0], // Dark Purple
    [0.75, 0.30, 0.00, 1.0], // Dark Orange
    [0.00, 0.45, 0.50, 1.0], // Dark Cyan
    [0.65, 0.18, 0.35, 1.0], // Dark Pink
];

fn stream_hash(seed: usize) -> f32 {
    let x = seed.wrapping_mul(2654435761) ^ seed.wrapping_mul(340573321);
    ((x & 0xFFFF) as f32) / 65535.0
}

// ── Layer classification based on tag/importance ──

fn classify_layer(meta: &TextMeta) -> RotundaLayer {
    match meta.tag.as_str() {
        "h1" | "h2" => RotundaLayer::Upper,
        "h3" | "h4" | "h5" | "h6" => RotundaLayer::Upper,
        "a" | "p" | "li" | "button" => RotundaLayer::Eye,
        "span" | "em" | "strong" | "b" | "i" | "u" | "small" => RotundaLayer::Lower,
        "td" | "th" | "dt" | "dd" | "figcaption" | "summary" | "time" => RotundaLayer::Lower,
        _ => {
            if meta.importance >= 0.5 {
                RotundaLayer::Upper
            } else if meta.importance >= 0.2 {
                RotundaLayer::Eye
            } else {
                RotundaLayer::Lower
            }
        }
    }
}

// ── Build ──

impl StreamState {
    pub fn from_layout(root: &LayoutNode) -> Self {
        let mut categories = Vec::new();
        let mut text_pool: Vec<TextMeta> = Vec::new();

        let top_children: Vec<&LayoutNode> = root
            .children
            .iter()
            .filter(|c| !c.tag.is_empty() && c.bounds.height > 0.0)
            .collect();

        for (ci, child) in top_children.iter().enumerate() {
            let name = extract_category_name(child);
            let color = CATEGORY_COLORS[ci % CATEGORY_COLORS.len()];
            categories.push(StreamCategory { name, color });
            collect_rich_texts(child, ci, &mut text_pool);
        }

        if categories.is_empty() {
            categories.push(StreamCategory {
                name: "INFO".into(),
                color: [0.3, 0.3, 0.3, 1.0],
            });
            collect_rich_texts(root, 0, &mut text_pool);
        }

        // Classify texts into 3 layers
        let mut upper_pool: Vec<usize> = Vec::new();
        let mut eye_pool: Vec<usize> = Vec::new();
        let mut lower_pool: Vec<usize> = Vec::new();

        for (i, meta) in text_pool.iter().enumerate() {
            match classify_layer(meta) {
                RotundaLayer::Upper => upper_pool.push(i),
                RotundaLayer::Eye => eye_pool.push(i),
                RotundaLayer::Lower => lower_pool.push(i),
            }
        }

        // Ensure each layer has content (redistribute if empty)
        if upper_pool.is_empty() && !eye_pool.is_empty() {
            let take = eye_pool.len().min(8);
            upper_pool.extend_from_slice(&eye_pool[..take]);
        }
        if lower_pool.is_empty() && !eye_pool.is_empty() {
            let skip = eye_pool.len().saturating_sub(8);
            lower_pool.extend_from_slice(&eye_pool[skip..]);
        }
        if eye_pool.is_empty() && !text_pool.is_empty() {
            eye_pool = (0..text_pool.len()).collect();
        }

        let mut particles = Vec::new();
        let mut next_id: usize = 0;
        let mut pool_cursor: usize = 0;

        // ── Upper Ring ──
        let upper_count = UPPER_SLOTS.min(upper_pool.len().max(1) * 2);
        for slot in 0..upper_count {
            if upper_pool.is_empty() { break; }
            let pool_idx = upper_pool[slot % upper_pool.len()];
            let meta = &text_pool[pool_idx];
            let base_angle = (slot as f32 / upper_count as f32) * std::f32::consts::TAU;
            let jitter_a = (stream_hash(next_id * 37) - 0.5) * 2.0 * ANGULAR_JITTER;
            let y = UPPER_Y_MIN + stream_hash(next_id * 53) * (UPPER_Y_MAX - UPPER_Y_MIN);
            let lifetime = LIFETIME_MIN + meta.importance * (LIFETIME_MAX - LIFETIME_MIN)
                + stream_hash(next_id * 71) * 3.0;
            let age = stream_hash(next_id * 19) * lifetime;

            particles.push(TextParticle {
                text: meta.display.clone(),
                angle: base_angle + jitter_a,
                y_pos: y,
                age,
                lifetime,
                category_index: meta.category_index,
                importance: meta.importance,
                grabbed: false,
                id: next_id,
                pool_index: pool_idx,
                layer: RotundaLayer::Upper,
                slot_index: slot,
            });
            next_id += 1;
        }

        // ── Eye Level (multiple rows) ──
        let eye_total = (EYE_SLOTS * EYE_ROWS).min(eye_pool.len().max(1) * 3);
        for slot in 0..eye_total {
            if eye_pool.is_empty() { break; }
            let pool_idx = eye_pool[slot % eye_pool.len()];
            let meta = &text_pool[pool_idx];
            let row = slot / EYE_SLOTS;
            let col = slot % EYE_SLOTS;
            let slots_in_row = EYE_SLOTS;
            let base_angle = (col as f32 / slots_in_row as f32) * std::f32::consts::TAU;
            let jitter_a = (stream_hash(next_id * 37) - 0.5) * 2.0 * ANGULAR_JITTER;
            let row_frac = if EYE_ROWS <= 1 { 0.5 } else { row as f32 / (EYE_ROWS - 1) as f32 };
            let y = EYE_Y_MIN + row_frac * (EYE_Y_MAX - EYE_Y_MIN)
                + (stream_hash(next_id * 53) - 0.5) * 2.0 * Y_JITTER;
            let lifetime = LIFETIME_MIN + meta.importance * (LIFETIME_MAX - LIFETIME_MIN)
                + stream_hash(next_id * 71) * 3.0;
            let age = stream_hash(next_id * 19) * lifetime;

            particles.push(TextParticle {
                text: meta.display.clone(),
                angle: base_angle + jitter_a,
                y_pos: y,
                age,
                lifetime,
                category_index: meta.category_index,
                importance: meta.importance,
                grabbed: false,
                id: next_id,
                pool_index: pool_idx,
                layer: RotundaLayer::Eye,
                slot_index: slot,
            });
            next_id += 1;
        }

        // ── Lower Ring ──
        let lower_count = LOWER_SLOTS.min(lower_pool.len().max(1) * 2);
        for slot in 0..lower_count {
            if lower_pool.is_empty() { break; }
            let pool_idx = lower_pool[slot % lower_pool.len()];
            let meta = &text_pool[pool_idx];
            let base_angle = (slot as f32 / lower_count as f32) * std::f32::consts::TAU;
            let jitter_a = (stream_hash(next_id * 37) - 0.5) * 2.0 * ANGULAR_JITTER;
            let y = LOWER_Y_MIN + stream_hash(next_id * 53) * (LOWER_Y_MAX - LOWER_Y_MIN);
            let lifetime = LIFETIME_MIN + meta.importance * (LIFETIME_MAX - LIFETIME_MIN)
                + stream_hash(next_id * 71) * 3.0;
            let age = stream_hash(next_id * 19) * lifetime;

            particles.push(TextParticle {
                text: meta.display.clone(),
                angle: base_angle + jitter_a,
                y_pos: y,
                age,
                lifetime,
                category_index: meta.category_index,
                importance: meta.importance,
                grabbed: false,
                id: next_id,
                pool_index: pool_idx,
                layer: RotundaLayer::Lower,
                slot_index: slot,
            });
            next_id += 1;
        }

        pool_cursor = next_id;

        StreamState {
            particles,
            categories,
            text_pool,
            pool_cursor,
            next_id,
            time: 0.0,
            grabbed_index: None,
        }
    }

    /// Update: rotate each layer at its own speed, respawn expired particles.
    pub fn update_flow(&mut self, dt: f32) -> bool {
        if self.particles.is_empty() {
            return false;
        }

        self.time += dt;
        let mut respawn_indices = Vec::new();

        for (i, p) in self.particles.iter_mut().enumerate() {
            if p.grabbed {
                continue;
            }

            // Rotate based on layer
            let speed = match p.layer {
                RotundaLayer::Upper => UPPER_SPEED,
                RotundaLayer::Eye => EYE_SPEED,
                RotundaLayer::Lower => LOWER_SPEED,
            };
            p.angle += speed * dt;

            // Age & respawn
            p.age += dt;
            if p.age >= p.lifetime {
                respawn_indices.push(i);
            }
        }

        for i in respawn_indices {
            self.respawn_at(i);
        }

        true
    }

    fn respawn_at(&mut self, pi: usize) {
        if self.text_pool.is_empty() {
            return;
        }

        let idx = self.pool_cursor % self.text_pool.len();
        self.pool_cursor = self.pool_cursor.wrapping_add(1);

        let meta = &self.text_pool[idx];
        let display = meta.display.clone();
        let cat_idx = meta.category_index;
        let importance = meta.importance;

        let seed = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);

        let p = &mut self.particles[pi];
        let layer = p.layer;
        let slot = p.slot_index;

        p.text = display;
        p.category_index = cat_idx;
        p.importance = importance;
        p.pool_index = idx;

        // Maintain structural slot position with fresh jitter
        let slots_total = match layer {
            RotundaLayer::Upper => UPPER_SLOTS,
            RotundaLayer::Eye => EYE_SLOTS,
            RotundaLayer::Lower => LOWER_SLOTS,
        };
        let effective_slot = slot % slots_total;
        let base_angle = (effective_slot as f32 / slots_total as f32) * std::f32::consts::TAU;
        let jitter_a = (stream_hash(seed * 37) - 0.5) * 2.0 * ANGULAR_JITTER;

        // Current rotation offset (so new text appears in-phase with layer)
        let layer_speed = match layer {
            RotundaLayer::Upper => UPPER_SPEED,
            RotundaLayer::Eye => EYE_SPEED,
            RotundaLayer::Lower => LOWER_SPEED,
        };
        let rotation_offset = layer_speed * self.time;

        p.angle = base_angle + jitter_a + rotation_offset;

        let (y_min, y_max) = match layer {
            RotundaLayer::Upper => (UPPER_Y_MIN, UPPER_Y_MAX),
            RotundaLayer::Eye => (EYE_Y_MIN, EYE_Y_MAX),
            RotundaLayer::Lower => (LOWER_Y_MIN, LOWER_Y_MAX),
        };
        p.y_pos = y_min + stream_hash(seed * 53) * (y_max - y_min);
        p.lifetime = LIFETIME_MIN + importance * (LIFETIME_MAX - LIFETIME_MIN)
            + stream_hash(seed * 71) * 3.0;
        p.age = 0.0;
        p.grabbed = false;
        p.id = seed;
    }

    /// Append new texts from background prefetch into the text pool.
    /// These will naturally appear as particles respawn.
    pub fn append_texts(&mut self, new_texts: Vec<TextMeta>) {
        self.text_pool.extend(new_texts);
    }

    /// Get 3D world position on the cylinder wall.
    /// Billboarding: x = R*cos(angle), z = R*sin(angle), y = y_pos.
    pub fn particle_world_pos(p: &TextParticle, time: f32) -> [f32; 3] {
        let phase = p.id as f32 * 1.618;
        let drift_y = (time * 0.2 + phase * 0.7).sin() * 0.08;

        let a = p.angle;

        [
            ROTUNDA_RADIUS * a.cos(),
            p.y_pos + drift_y,
            ROTUNDA_RADIUS * a.sin(),
        ]
    }

    /// Lifecycle-based opacity (fade in / visible / fade out).
    pub fn particle_opacity(p: &TextParticle) -> f32 {
        if p.grabbed {
            return 1.0;
        }
        let fade_out_start = p.lifetime - FADE_OUT_DURATION;
        if p.age < FADE_IN_DURATION {
            p.age / FADE_IN_DURATION
        } else if p.age < fade_out_start {
            1.0
        } else {
            let t = (p.age - fade_out_start) / FADE_OUT_DURATION;
            (1.0 - t).max(0.0)
        }
    }

    /// Layer-based font size multiplier.
    pub fn layer_font_scale(layer: RotundaLayer) -> f32 {
        match layer {
            RotundaLayer::Upper => 1.3,  // big headings
            RotundaLayer::Eye => 1.0,    // normal
            RotundaLayer::Lower => 0.75, // small tags
        }
    }

    /// Try to grab the nearest visible particle to a screen click.
    /// `aspect` = screen width / height (must match rendering projection).
    pub fn try_grab_screen(&mut self, click_ndc_x: f32, click_ndc_y: f32,
                            cam_az: f32, cam_el: f32,
                            fov_h: f32, _fov_v: f32,
                            aspect: f32) -> Option<usize> {
        let mut best_idx = None;
        let mut best_dist = f32::MAX;

        let sin_az = cam_az.sin();
        let cos_az = cam_az.cos();
        let sin_el = cam_el.sin();
        let cos_el = cam_el.cos();
        let tan_fov_h = fov_h.tan();

        for (i, p) in self.particles.iter().enumerate() {
            if Self::particle_opacity(p) < 0.15 { continue; }

            let world = Self::particle_world_pos(p, self.time);
            let wx = world[0];
            let wy = world[1];
            let wz = world[2];

            // Camera rotation: azimuth (Y-axis) then elevation (X-axis)
            let rx1 = wx * cos_az + wz * sin_az;
            let ry1 = wy;
            let rz1 = -wx * sin_az + wz * cos_az;

            let rx = rx1;
            let ry = ry1 * cos_el - rz1 * sin_el;
            let rz = ry1 * sin_el + rz1 * cos_el;

            // Skip particles behind camera
            if rz < 1.0 { continue; }

            // Perspective projection (must match rendering in main.rs)
            let ndc_x = rx / (rz * tan_fov_h);
            let ndc_y = -ry / (rz * tan_fov_h / aspect);

            let dx = ndc_x - click_ndc_x;
            let dy = ndc_y - click_ndc_y;
            let dist = dx * dx + dy * dy;

            // Hit radius: generous for usability
            let hit_radius = 0.04 + p.importance * 0.06;
            if dist < hit_radius * hit_radius && dist < best_dist {
                best_dist = dist;
                best_idx = Some(i);
            }
        }

        // Release previous grab
        if let Some(old) = self.grabbed_index {
            if old < self.particles.len() {
                self.particles[old].grabbed = false;
            }
        }

        if let Some(idx) = best_idx {
            self.particles[idx].grabbed = true;
            self.grabbed_index = Some(idx);
        } else {
            self.grabbed_index = None;
        }
        best_idx
    }

    /// Release all grabbed particles.
    pub fn release_all(&mut self) {
        for p in &mut self.particles {
            p.grabbed = false;
        }
        self.grabbed_index = None;
    }

    /// Get rich info about the currently grabbed particle.
    pub fn grabbed_info(&self) -> Option<GrabbedInfo<'_>> {
        let idx = self.grabbed_index?;
        let p = self.particles.get(idx)?;
        if !p.grabbed { return None; }
        let meta = self.text_pool.get(p.pool_index)?;
        let cat_name = self.categories.get(p.category_index)
            .map(|c| c.name.as_str())
            .unwrap_or("INFO");
        Some(GrabbedInfo {
            particle: p,
            meta,
            category_name: cat_name,
        })
    }

    /// Return an empty SdfScene with white background.
    pub fn to_sdf_scene(&self) -> SdfScene {
        SdfScene {
            primitives: Vec::new(),
            background_color: [1.0, 1.0, 1.0, 1.0],
        }
    }
}

// ── Text extraction (unchanged) ──

fn extract_category_name(node: &LayoutNode) -> String {
    for child in &node.children {
        if matches!(child.tag.as_str(), "h1" | "h2" | "h3" | "h4" | "h5" | "h6") {
            let t = collect_text_content(child);
            if !t.is_empty() {
                return t.chars().take(16).collect();
            }
        }
    }
    match node.tag.as_str() {
        "nav" => "NAVIGATION".into(),
        "header" => "HEADER".into(),
        "footer" => "FOOTER".into(),
        "main" => "MAIN".into(),
        "aside" => "SIDEBAR".into(),
        _ => {
            let t = collect_text_content(node);
            if !t.is_empty() {
                t.split_whitespace().take(2).collect::<Vec<_>>().join(" ")
                    .chars().take(16).collect()
            } else {
                node.tag.to_uppercase()
            }
        }
    }
}

fn collect_rich_texts(
    node: &LayoutNode,
    category_index: usize,
    out: &mut Vec<TextMeta>,
) {
    let (importance, is_leaf) = match node.tag.as_str() {
        "h1" | "h2" => (1.0, true),
        "h3" | "h4" | "h5" | "h6" => (0.6, true),
        "a" => (0.5, true),
        "button" | "label" => (0.4, true),
        "p" | "li" | "span" | "em" | "strong" | "b" | "i" | "u" => (0.2, true),
        "td" | "th" | "dt" | "dd" | "figcaption" | "summary" | "time" => (0.2, true),
        _ => (0.15, false),
    };

    if is_leaf {
        let full = collect_text_content(node);
        if full.len() > 1 {
            let href = if node.href.is_some() {
                node.href.clone()
            } else {
                find_child_href(node)
            };

            if full.chars().count() > 60 {
                let words: Vec<&str> = full.split_whitespace().collect();
                let mut chunk = String::new();
                for w in &words {
                    if !chunk.is_empty() && chunk.chars().count() + w.chars().count() > 50 {
                        push_text_meta(out, &chunk, &full, &node.tag, &href, category_index, importance);
                        chunk.clear();
                    }
                    if !chunk.is_empty() { chunk.push(' '); }
                    chunk.push_str(w);
                }
                if !chunk.is_empty() {
                    push_text_meta(out, &chunk, &full, &node.tag, &href, category_index, importance);
                }
            } else {
                push_text_meta(out, &full, &full, &node.tag, &href, category_index, importance);
            }
        }
        return;
    }

    if node.tag.is_empty() && !node.text.is_empty() {
        let t = node.text.trim();
        if t.len() > 1 {
            push_text_meta(out, t, t, "", &None, category_index, 0.15);
        }
    }

    if !node.tag.is_empty() && !is_leaf {
        let direct_text: String = node.text.trim().to_string();
        if direct_text.len() > 1 && direct_text.chars().count() < 60 {
            push_text_meta(out, &direct_text, &direct_text, &node.tag, &node.href, category_index, 0.15);
        }
    }

    for child in &node.children {
        collect_rich_texts(child, category_index, out);
    }
}

fn push_text_meta(
    out: &mut Vec<TextMeta>,
    display_src: &str,
    full_src: &str,
    tag: &str,
    href: &Option<String>,
    category_index: usize,
    importance: f32,
) {
    let display: String = display_src.chars().take(40).collect();
    if display.trim().is_empty() { return; }
    out.push(TextMeta {
        display,
        full_text: full_src.chars().take(300).collect(),
        tag: tag.to_string(),
        href: href.clone(),
        category_index,
        importance,
    });
}

fn find_child_href(node: &LayoutNode) -> Option<String> {
    for child in &node.children {
        if child.tag == "a" {
            if child.href.is_some() {
                return child.href.clone();
            }
        }
        if let Some(href) = find_child_href(child) {
            return Some(href);
        }
    }
    None
}

fn collect_text_content(node: &LayoutNode) -> String {
    let mut text = String::new();
    if !node.text.is_empty() {
        text.push_str(node.text.trim());
    }
    for child in &node.children {
        let ct = collect_text_content(child);
        if !ct.is_empty() {
            if !text.is_empty() { text.push(' '); }
            text.push_str(&ct);
        }
    }
    text
}
