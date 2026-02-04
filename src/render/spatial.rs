/// Spatial Web: 3D web browsing mode — Phase 2 "Deep Web".
///
/// Converts the DOM hierarchy into a walkable 3D architectural space.
///
/// Architecture:
///   - `SdfElement`        — per-tag shape classification
///   - `SpatialBuilder`    — DOM traversal + SDF primitive builder
///   - `detect_feed_pattern` — heuristic: repeated similar children → corridor
///   - Corridor Transform  — Y-axis list → Z-axis corridor conversion
///
/// Element mapping:
///   - `<body>`              → Ground plane
///   - `<section>/<article>`  → Wall panels (relief backdrop)
///   - `<nav>/<header>`       → Ceiling beams
///   - `<a href>`             → Blue portal (thick, protruding)
///   - `<button>`             → Colored pill (thick, tactile)
///   - `<h1>-<h6>`            → Gold/white text slabs
///   - `<p>/<li>`             → Thin translucent panels
///   - Feed pattern (3+ similar) → Corridor stretching into depth
///   - `<img>`                → Framed picture on wall
///   - `<hr>`                 → Floor line
use crate::render::layout::LayoutNode;
use crate::render::sdf_ui::{SdfPrimitive, SdfScene};

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  SdfElement — HTML tag → 3D shape classification
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Clone)]
enum SdfElement {
    /// Container wall (section, article, main, div)
    Wall { thickness: f32, color: [f32; 4] },
    /// Navigation / header beam
    Beam { color: [f32; 4] },
    /// Heading slab (h1-h6)
    Heading { level: u8, color: [f32; 4] },
    /// Link portal (thick, forward-protruding)
    Portal { thickness: f32, color: [f32; 4] },
    /// Button pill (thick, tactile)
    Button { thickness: f32, color: [f32; 4] },
    /// Text panel (p, span, li)
    Panel { color: [f32; 4] },
    /// Image with frame
    Picture,
    /// Horizontal rule
    Separator,
    /// Bare text node
    Text { color: [f32; 4] },
    /// List container (ul, ol) — may become corridor
    List,
    /// No visual representation (html, body internals, etc.)
    Invisible,
}

fn classify_tag(tag: &str, depth: u32) -> SdfElement {
    match tag {
        "section" | "article" | "main" | "div" => SdfElement::Wall {
            thickness: 0.10 + depth as f32 * 0.03,
            color: [0.93, 0.93, 0.96, 0.85],
        },
        "nav" | "header" | "footer" => SdfElement::Beam {
            color: [0.75, 0.78, 0.85, 1.0],
        },
        "h1" => SdfElement::Heading { level: 1, color: [0.95, 0.85, 0.4, 1.0] },
        "h2" => SdfElement::Heading { level: 2, color: [0.90, 0.82, 0.5, 1.0] },
        "h3" => SdfElement::Heading { level: 3, color: [0.85, 0.83, 0.6, 1.0] },
        "h4" => SdfElement::Heading { level: 4, color: [0.85, 0.83, 0.6, 1.0] },
        "h5" => SdfElement::Heading { level: 5, color: [0.85, 0.83, 0.6, 1.0] },
        "h6" => SdfElement::Heading { level: 6, color: [0.85, 0.83, 0.6, 1.0] },
        "a" => SdfElement::Portal {
            thickness: 0.15,
            color: [0.10, 0.40, 0.95, 1.0],
        },
        "button" | "input" => SdfElement::Button {
            thickness: 0.10,
            color: [0.20, 0.55, 0.95, 1.0],
        },
        "p" | "span" | "li" => SdfElement::Panel {
            color: [0.98, 0.98, 1.0, 0.7],
        },
        "ul" | "ol" => SdfElement::List,
        "img" => SdfElement::Picture,
        "hr" => SdfElement::Separator,
        "" => SdfElement::Text {
            color: [0.96, 0.96, 0.98, 0.6],
        },
        _ => SdfElement::Invisible,
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  SpatialConfig
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Configuration for spatial web rendering
#[derive(Debug, Clone)]
pub struct SpatialConfig {
    /// Scale factor: pixels → meters
    pub pixel_to_meter: f32,
    /// Base floor depth per DOM level
    pub depth_per_level: f32,
    /// Element forward protrusion per level
    pub protrusion: f32,
    /// Z-axis spacing between corridor items (meters)
    pub corridor_item_spacing: f32,
    /// Minimum number of similar children to trigger corridor
    pub corridor_min_items: usize,
}

impl Default for SpatialConfig {
    fn default() -> Self {
        Self {
            pixel_to_meter: 0.005,
            depth_per_level: 0.4,
            protrusion: 0.35,
            corridor_item_spacing: 0.6,
            corridor_min_items: 3,
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  SpatialBuilder
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

struct SpatialBuilder {
    cfg: SpatialConfig,
    primitives: Vec<SdfPrimitive>,
}

impl SpatialBuilder {
    fn new(cfg: SpatialConfig) -> Self {
        Self {
            cfg,
            primitives: Vec::new(),
        }
    }

    /// Entry point: build the full 3D scene from a layout tree
    fn build(mut self, root: &LayoutNode) -> SdfScene {
        let s = self.cfg.pixel_to_meter;
        let width = (root.bounds.width * s).max(1.0);
        let depth = (root.bounds.height * s).max(1.0);

        // Ground plane
        self.primitives.push(SdfPrimitive::RoundedBox {
            center: [width / 2.0, -0.02, -depth / 2.0],
            size: [width + 0.5, 0.04, depth + 0.5],
            radius: 0.02,
            color: [0.88, 0.88, 0.90, 1.0],
        });

        self.traverse(root, 0);

        SdfScene {
            primitives: self.primitives,
            background_color: [0.55, 0.75, 0.95, 1.0],
        }
    }

    /// Traverse the DOM tree, classifying each node and emitting primitives
    fn traverse(&mut self, node: &LayoutNode, depth: u32) {
        let element = classify_tag(node.tag.as_str(), depth);

        // Check for feed pattern on containers and lists
        match &element {
            SdfElement::Wall { .. } | SdfElement::List => {
                if let Some(feed_items) = detect_feed_pattern(node, &self.cfg) {
                    self.emit_element(node, &element, depth);
                    self.emit_corridor(node, &feed_items, depth);
                    return; // corridor handled all children
                }
            }
            _ => {}
        }

        // Emit the element itself
        let is_leaf = self.emit_element(node, &element, depth);

        if is_leaf {
            return;
        }

        // Recurse into children
        for child in &node.children {
            self.traverse(child, depth + 1);
        }
    }

    /// Emit SDF primitives for a single element. Returns true if this is a leaf (no recursion).
    fn emit_element(&mut self, node: &LayoutNode, element: &SdfElement, depth: u32) -> bool {
        let b = &node.bounds;
        let s = self.cfg.pixel_to_meter;
        let z_base = -(b.y * s);
        let z_forward = depth as f32 * self.cfg.protrusion;
        let cx = b.x * s + b.width * s / 2.0;
        let w = (b.width * s).max(0.02);
        let h = (b.height * s).max(0.02);

        match element {
            SdfElement::Wall { thickness, color } => {
                if h > 0.1 && w > 0.1 {
                    let wall_h = h.min(3.0);
                    self.primitives.push(SdfPrimitive::RoundedBox {
                        center: [cx, wall_h / 2.0, z_base + z_forward - 0.05],
                        size: [w, wall_h, *thickness],
                        radius: 0.03,
                        color: *color,
                    });
                }
                false // recurse into children
            }

            SdfElement::Beam { color } => {
                if w > 0.1 {
                    let beam_h = h.min(0.5).max(0.08);
                    self.primitives.push(SdfPrimitive::RoundedBox {
                        center: [cx, beam_h / 2.0 + 0.01, z_base + z_forward],
                        size: [w, beam_h, 0.08],
                        radius: 0.02,
                        color: *color,
                    });
                }
                false
            }

            SdfElement::Heading { level, color } => {
                let text = collect_text(node);
                if !text.is_empty() {
                    let slab_h = (node.font_size * s * 1.8).max(0.08);
                    let slab_w = w.min(2.0);
                    let thickness = 0.04 + (6.0 - *level as f32) * 0.01;
                    self.primitives.push(SdfPrimitive::RoundedBox {
                        center: [cx, slab_h / 2.0 + 0.02, z_base + z_forward + 0.05],
                        size: [slab_w, slab_h, thickness],
                        radius: 0.015,
                        color: *color,
                    });
                }
                true // leaf
            }

            SdfElement::Portal { thickness, color } => {
                let text = collect_text(node);
                if !text.is_empty() {
                    let portal_w = w.min(1.2).max(0.1);
                    let portal_h = (node.font_size * s * 1.5).max(0.06);
                    self.primitives.push(SdfPrimitive::RoundedBox {
                        center: [cx, portal_h / 2.0 + 0.02, z_base + z_forward + 0.32],
                        size: [portal_w, portal_h, *thickness],
                        radius: 0.025,
                        color: *color,
                    });
                }
                true // leaf
            }

            SdfElement::Button { thickness, color } => {
                let btn_w = w.min(0.8).max(0.08);
                let btn_h = h.min(0.3).max(0.06);
                self.primitives.push(SdfPrimitive::RoundedBox {
                    center: [cx, btn_h / 2.0 + 0.02, z_base + z_forward + 0.25],
                    size: [btn_w, btn_h, *thickness],
                    radius: btn_h / 2.0,
                    color: *color,
                });
                true // leaf
            }

            SdfElement::Panel { color } => {
                let text = collect_text(node);
                if !text.is_empty() {
                    let panel_h = h.min(1.0).max(0.04);
                    let panel_w = w.min(2.5);
                    self.primitives.push(SdfPrimitive::RoundedBox {
                        center: [cx, panel_h / 2.0 + 0.01, z_base + z_forward + 0.03],
                        size: [panel_w, panel_h, 0.015],
                        radius: 0.005,
                        color: *color,
                    });
                }
                true // leaf
            }

            SdfElement::Picture => {
                let img_w = w.min(1.5).max(0.1);
                let img_h = h.min(1.0).max(0.1);
                // Frame
                self.primitives.push(SdfPrimitive::RoundedBox {
                    center: [cx, img_h / 2.0 + 0.02, z_base + z_forward + 0.06],
                    size: [img_w + 0.04, img_h + 0.04, 0.025],
                    radius: 0.01,
                    color: [0.3, 0.3, 0.32, 1.0],
                });
                // Picture surface
                self.primitives.push(SdfPrimitive::RoundedBox {
                    center: [cx, img_h / 2.0 + 0.02, z_base + z_forward + 0.075],
                    size: [img_w, img_h, 0.01],
                    radius: 0.005,
                    color: [0.75, 0.78, 0.82, 1.0],
                });
                true // leaf
            }

            SdfElement::Separator => {
                self.primitives.push(SdfPrimitive::Line {
                    start: [b.x * s, 0.005, z_base + z_forward],
                    end: [(b.x + b.width) * s, 0.005, z_base + z_forward],
                    thickness: 0.008,
                    color: [0.6, 0.6, 0.65, 1.0],
                });
                true // leaf
            }

            SdfElement::Text { color } => {
                if !node.text.is_empty() {
                    let panel_h = h.min(0.5).max(0.03);
                    let panel_w = w.min(2.0).max(0.05);
                    self.primitives.push(SdfPrimitive::RoundedBox {
                        center: [cx, panel_h / 2.0 + 0.01, z_base + z_forward + 0.02],
                        size: [panel_w, panel_h, 0.01],
                        radius: 0.003,
                        color: *color,
                    });
                }
                true // leaf (bare text)
            }

            SdfElement::List => {
                // Non-corridor list: no visual of its own, just recurse
                false
            }

            SdfElement::Invisible => false,
        }
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    //  Corridor Transform
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    /// Emit a corridor: feed items arranged along Z-axis
    fn emit_corridor(&mut self, parent: &LayoutNode, items: &[&LayoutNode], depth: u32) {
        let s = self.cfg.pixel_to_meter;
        let pb = &parent.bounds;
        let cx = pb.x * s + pb.width * s / 2.0;
        let w = (pb.width * s).max(0.02);
        let z_base = -(pb.y * s);
        let z_forward = depth as f32 * self.cfg.protrusion;
        let spacing = self.cfg.corridor_item_spacing;
        let corridor_w = w.min(2.5);

        // ── Side walls ──
        let corridor_len = items.len() as f32 * spacing + 0.5;
        for side in [-1.0_f32, 1.0] {
            self.primitives.push(SdfPrimitive::RoundedBox {
                center: [
                    cx + side * (corridor_w / 2.0 + 0.04),
                    0.3,
                    z_base + z_forward - corridor_len / 2.0,
                ],
                size: [0.03, 0.6, corridor_len],
                radius: 0.01,
                color: [0.85, 0.85, 0.90, 0.6],
            });
        }

        // ── Each feed item as a panel + floor divider ──
        for (i, item) in items.iter().enumerate() {
            let item_z = z_base + z_forward - (i as f32 * spacing);
            let ib = &item.bounds;
            let item_h = (ib.height * s).max(0.04).min(0.4);
            let item_w = corridor_w * 0.9;

            // Card panel
            self.primitives.push(SdfPrimitive::RoundedBox {
                center: [cx, item_h / 2.0 + 0.01, item_z],
                size: [item_w, item_h, 0.02],
                radius: 0.008,
                color: [0.96, 0.96, 1.0, 0.8],
            });

            // Floor divider line between items
            if i > 0 {
                let div_z = item_z + spacing * 0.5;
                self.primitives.push(SdfPrimitive::Line {
                    start: [cx - item_w / 2.0, 0.003, div_z],
                    end: [cx + item_w / 2.0, 0.003, div_z],
                    thickness: 0.006,
                    color: [0.70, 0.70, 0.75, 0.5],
                });
            }

            // Recurse into each item's children (links, headings, text, images)
            for child in &item.children {
                self.traverse(child, depth + 2);
            }
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  detect_feed_pattern — heuristic feed detection
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Detect a feed/news-list pattern in a container's children.
///
/// Returns the list of candidate feed items if the pattern is found.
///
/// Two modes:
///   1. **Explicit list**: `<ul>/<ol>` with N+ `<li>` children
///   2. **Implicit feed**: any container with N+ same-tag children that have
///      similar height (±50%) and each spans ≥60% of the parent width
fn detect_feed_pattern<'a>(
    node: &'a LayoutNode,
    cfg: &SpatialConfig,
) -> Option<Vec<&'a LayoutNode>> {
    let min_items = cfg.corridor_min_items;
    let tag = node.tag.as_str();

    // ── Mode 1: explicit <ul>/<ol> ──
    if tag == "ul" || tag == "ol" {
        let li_items: Vec<&LayoutNode> = node
            .children
            .iter()
            .filter(|c| c.tag == "li")
            .collect();
        if li_items.len() >= min_items {
            return Some(li_items);
        }
    }

    // ── Mode 2: implicit feed in any container ──
    if matches!(tag, "section" | "article" | "main" | "div") {
        // Find the most common child tag
        let mut tag_counts: Vec<(&str, Vec<&LayoutNode>)> = Vec::new();
        for child in &node.children {
            if child.tag.is_empty() || child.bounds.height <= 0.0 {
                continue;
            }
            if let Some(entry) = tag_counts.iter_mut().find(|(t, _)| *t == child.tag.as_str()) {
                entry.1.push(child);
            } else {
                tag_counts.push((child.tag.as_str(), vec![child]));
            }
        }

        // Find the largest group of same-tag children
        if let Some((_tag, group)) = tag_counts.into_iter().max_by_key(|(_, g)| g.len()) {
            if group.len() >= min_items {
                // Verify height similarity: each item within ±50% of median height
                let mut heights: Vec<f32> = group.iter().map(|n| n.bounds.height).collect();
                heights.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                let median_h = heights[heights.len() / 2];

                if median_h > 0.0 {
                    let similar = group.iter().all(|n| {
                        let h = n.bounds.height;
                        h >= median_h * 0.5 && h <= median_h * 1.5
                    });

                    // Verify width: each item spans ≥60% of parent
                    let parent_w = node.bounds.width;
                    let wide_enough = parent_w > 0.0
                        && group.iter().all(|n| n.bounds.width >= parent_w * 0.6);

                    if similar && wide_enough {
                        return Some(group);
                    }
                }
            }
        }
    }

    None
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  Public API
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Convert a 2D layout into a 3D spatial scene
pub fn layout_to_spatial(root: &LayoutNode, config: &SpatialConfig) -> SdfScene {
    let builder = SpatialBuilder::new(config.clone());
    builder.build(root)
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  OZ Mode — "True OZ" Orbital / Planetary layout
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//
// Superflat + Celestial Mechanics:
//   - 3D orbital galaxy with inclined orbits
//   - Massive Sun, tiny planets, tinier satellites
//   - Torus rings for orbit paths
//   - Pop-color palette (saturated Cyan/Magenta/Yellow)
//   - Returns (SdfScene, OzAnimState) for animation

use crate::render::animator::{AnimKind, OzAnimMeta, OzAnimState};

/// Configuration for OZ Mode rendering
#[derive(Debug, Clone)]
pub struct OzConfig {
    pub sun_radius: f32,
    pub orbit_base: f32,
    pub orbit_spread: f32,
    pub planet_radius: f32,
    pub satellite_radius: f32,
    pub micro_radius: f32,
    pub ring_thickness: f32,
    pub connector_thickness: f32,
    pub max_depth: u32,
}

impl Default for OzConfig {
    fn default() -> Self {
        Self {
            sun_radius: 1.5,
            orbit_base: 4.0,
            orbit_spread: 1.8,
            planet_radius: 0.18,
            satellite_radius: 0.08,
            micro_radius: 0.04,
            ring_thickness: 0.012,
            connector_thickness: 0.006,
            max_depth: 5,
        }
    }
}

/// OZ Mode color palette — Superflat Pop Colors
struct OzPalette;

impl OzPalette {
    /// Sun: pure glowing white
    fn sun() -> [f32; 4] { [1.0, 1.0, 0.98, 1.0] }
    /// Orbit ring: faint cyan-white
    fn ring() -> [f32; 4] { [0.70, 0.90, 1.0, 0.3] }
    /// Connector lines: subtle white
    fn connector() -> [f32; 4] { [0.75, 0.80, 0.90, 0.35] }

    /// Planet color — saturated pop colors (OZ-style)
    fn planet(index: usize) -> [f32; 4] {
        const COLORS: &[[f32; 4]] = &[
            [0.0, 0.85, 1.0, 1.0],  // Cyan
            [1.0, 0.0, 0.65, 1.0],  // Magenta
            [1.0, 0.95, 0.0, 1.0],  // Yellow
            [0.0, 1.0, 0.5, 1.0],   // Spring Green
            [0.55, 0.0, 1.0, 1.0],  // Violet
            [1.0, 0.45, 0.0, 1.0],  // Vivid Orange
            [0.0, 0.55, 1.0, 1.0],  // Azure
            [1.0, 0.0, 0.35, 1.0],  // Rose
        ];
        COLORS[index % COLORS.len()]
    }

    /// Satellite: slightly desaturated version of parent
    fn satellite(parent_index: usize) -> [f32; 4] {
        let base = Self::planet(parent_index);
        [
            (base[0] * 0.6 + 0.4).min(1.0),
            (base[1] * 0.6 + 0.4).min(1.0),
            (base[2] * 0.6 + 0.4).min(1.0),
            0.9,
        ]
    }

    /// Micro-node: pastel version
    fn micro(parent_index: usize) -> [f32; 4] {
        let base = Self::planet(parent_index);
        [
            (base[0] * 0.35 + 0.65).min(1.0),
            (base[1] * 0.35 + 0.65).min(1.0),
            (base[2] * 0.35 + 0.65).min(1.0),
            0.75,
        ]
    }
}

/// Deterministic hash for orbit inclination
fn oz_hash(seed: usize) -> f32 {
    let x = seed.wrapping_mul(2654435761) ^ seed.wrapping_mul(340573321);
    ((x & 0xFFFF) as f32) / 65535.0
}

/// Headline mapped to its owning planet index.
/// Used for Link Lines on hover.
#[derive(Debug, Clone)]
pub struct OzHeadlineEntry {
    /// Index of the Billboard primitive in the scene
    pub prim_index: usize,
    /// Index of the planet Sphere primitive this headline belongs to
    pub planet_prim_index: usize,
}

/// Result of building the OZ system.
#[derive(Debug, Clone)]
pub struct OzBuildResult {
    pub scene: SdfScene,
    pub anim: OzAnimState,
    /// Mapping from ticker headline to its owning planet
    pub headline_map: Vec<OzHeadlineEntry>,
}

/// Build a "News Ring" OZ scene.
///
/// Structure (planets, orbits, satellites) and Information (headlines on outer ring)
/// are completely separated. Planets are pure colored glass — no text labels.
pub fn build_oz_system(
    root: &LayoutNode,
    config: &OzConfig,
) -> OzBuildResult {
    let mut primitives = Vec::new();
    let mut anim = OzAnimState::new();
    let mut headline_map: Vec<OzHeadlineEntry> = Vec::new();

    // ── Sun ──
    primitives.push(SdfPrimitive::Sphere {
        center: [0.0, 0.0, 0.0],
        radius: config.sun_radius,
        color: OzPalette::sun(),
    });
    anim.push(OzAnimMeta {
        depth: 0,
        orbit_radius: 0.0,
        parent_center: [0.0, 0.0, 0.0],
        angle_offset: 0.0,
        inclination: 0.0,
        kind: AnimKind::Sun,
    });

    // ── Planets (Clean: no text, pure spheres) ──
    let planets: Vec<&LayoutNode> = root
        .children
        .iter()
        .filter(|c| !c.tag.is_empty() && c.bounds.height > 0.0)
        .collect();

    let planet_count = planets.len().max(1);

    // Collect all headlines per planet for the ticker ring
    let mut all_headlines: Vec<(usize, String)> = Vec::new(); // (planet_index, text)
    // Store planet primitive indices for link lines
    let mut planet_prim_indices: Vec<usize> = Vec::new();

    for (pi, planet_node) in planets.iter().enumerate() {
        let orbit_r = config.orbit_base + pi as f32 * config.orbit_spread;
        let inclination = oz_hash(pi * 7 + 3) * std::f32::consts::PI * 0.7 + 0.15;
        let base_angle = 2.0 * std::f32::consts::PI * pi as f32 / planet_count as f32;
        let angle = base_angle + oz_hash(pi * 13 + 1) * 0.4;

        let px = orbit_r * angle.cos() * inclination.cos();
        let py = orbit_r * inclination.sin() * (angle * 0.5).sin();
        let pz = orbit_r * angle.sin() * inclination.cos();
        let planet_center = [px, py, pz];

        // Orbit ring (Torus)
        primitives.push(SdfPrimitive::Torus {
            center: [0.0, 0.0, 0.0],
            major_radius: orbit_r,
            minor_radius: config.ring_thickness,
            axis: [0.0, inclination.cos(), inclination.sin()],
            color: OzPalette::ring(),
        });
        anim.push(OzAnimMeta {
            depth: 1,
            orbit_radius: orbit_r,
            parent_center: [0.0, 0.0, 0.0],
            angle_offset: angle,
            inclination,
            kind: AnimKind::Ring,
        });

        // Planet sphere (Clean: no text attached)
        let planet_idx = primitives.len();
        planet_prim_indices.push(planet_idx);
        primitives.push(SdfPrimitive::Sphere {
            center: planet_center,
            radius: config.planet_radius,
            color: OzPalette::planet(pi),
        });
        anim.push(OzAnimMeta {
            depth: 1,
            orbit_radius: orbit_r,
            parent_center: [0.0, 0.0, 0.0],
            angle_offset: angle,
            inclination,
            kind: AnimKind::Orbiter,
        });

        // Connector: Sun → Planet
        primitives.push(SdfPrimitive::Line {
            start: [0.0, 0.0, 0.0],
            end: planet_center,
            thickness: config.connector_thickness,
            color: OzPalette::connector(),
        });
        anim.push(OzAnimMeta {
            depth: 1,
            orbit_radius: orbit_r,
            parent_center: [0.0, 0.0, 0.0],
            angle_offset: angle,
            inclination,
            kind: AnimKind::Connector { child_index: planet_idx },
        });

        // Satellites (depth 2+): clean spheres only, no text
        emit_oz_children_clean(
            &mut primitives,
            &mut anim,
            planet_node,
            planet_center,
            config,
            pi,
            2,
            orbit_r * 0.35,
        );

        // Collect headlines from this planet's subtree for the ticker
        collect_headlines_recursive(planet_node, pi, &mut all_headlines);
    }

    // ── News Ticker Ring: outer Data Ring ──
    let outermost_orbit = config.orbit_base + (planet_count.max(1) - 1) as f32 * config.orbit_spread;
    let data_ring_radius = outermost_orbit + config.orbit_spread * 1.5 + 2.0;

    // Data Ring Torus (visible ring)
    primitives.push(SdfPrimitive::Torus {
        center: [0.0, 0.0, 0.0],
        major_radius: data_ring_radius,
        minor_radius: config.ring_thickness * 1.5,
        axis: [0.0, 1.0, 0.0], // Flat horizontal ring
        color: [0.3, 0.6, 1.0, 0.25], // Blue glow
    });
    anim.push(OzAnimMeta {
        depth: 0,
        orbit_radius: data_ring_radius,
        parent_center: [0.0, 0.0, 0.0],
        angle_offset: 0.0,
        inclination: 0.0,
        kind: AnimKind::Ring,
    });

    // Place headline Billboards along the Data Ring
    let headline_count = all_headlines.len().max(1);
    let angle_step = 2.0 * std::f32::consts::PI / headline_count as f32;

    for (hi, (planet_idx, text)) in all_headlines.iter().enumerate() {
        let base_angle = hi as f32 * angle_step;
        let hx = data_ring_radius * base_angle.cos();
        let hz = data_ring_radius * base_angle.sin();

        let hw = text.len().min(30) as f32 * 0.06;
        let ticker_prim_idx = primitives.len();

        primitives.push(SdfPrimitive::Billboard {
            position: [hx, 0.0, hz],
            size: [hw, 0.12],
            text: text.clone(),
            color: [0.9, 0.95, 1.0, 1.0], // Neon white
            opacity: 0.85,
        });
        anim.push(OzAnimMeta {
            depth: 0,
            orbit_radius: data_ring_radius,
            parent_center: [0.0, 0.0, 0.0],
            angle_offset: base_angle,
            inclination: 0.0,
            kind: AnimKind::Ticker { ring_radius: data_ring_radius },
        });

        // Record headline → planet mapping for Link Lines
        let planet_pi = planet_prim_indices.get(*planet_idx).copied().unwrap_or(0);
        headline_map.push(OzHeadlineEntry {
            prim_index: ticker_prim_idx,
            planet_prim_index: planet_pi,
        });
    }

    let scene = SdfScene {
        primitives,
        background_color: [0.04, 0.04, 0.12, 1.0], // Deep space blue
    };
    OzBuildResult {
        scene,
        anim,
        headline_map,
    }
}

/// Recursively emit satellite nodes — clean spheres only, no text.
fn emit_oz_children_clean(
    out: &mut Vec<SdfPrimitive>,
    anim: &mut OzAnimState,
    node: &LayoutNode,
    parent_center: [f32; 3],
    config: &OzConfig,
    color_index: usize,
    depth: u32,
    orbit_radius: f32,
) {
    if depth > config.max_depth {
        return;
    }

    let children: Vec<&LayoutNode> = node
        .children
        .iter()
        .filter(|c| !c.tag.is_empty() || !c.text.is_empty())
        .filter(|c| c.bounds.height > 0.0 || !c.text.is_empty())
        .collect();

    if children.is_empty() {
        return;
    }

    let count = children.len().max(1);
    let node_radius = if depth == 2 {
        config.satellite_radius
    } else {
        (config.micro_radius * 0.8_f32.powi(depth as i32 - 3)).max(0.015)
    };

    let sub_incl = oz_hash(color_index * 100 + depth as usize * 37) * std::f32::consts::PI * 0.8;

    // Sub-orbit ring (depth 2 only)
    if depth == 2 && count >= 2 {
        out.push(SdfPrimitive::Torus {
            center: parent_center,
            major_radius: orbit_radius,
            minor_radius: config.ring_thickness * 0.6,
            axis: [sub_incl.sin(), sub_incl.cos(), 0.0],
            color: OzPalette::ring(),
        });
        anim.push(OzAnimMeta {
            depth,
            orbit_radius,
            parent_center,
            angle_offset: 0.0,
            inclination: sub_incl,
            kind: AnimKind::Ring,
        });
    }

    for (i, child) in children.iter().enumerate() {
        let angle = 2.0 * std::f32::consts::PI * i as f32 / count as f32
            + oz_hash(i * 31 + depth as usize * 17) * 0.3;

        let lx = orbit_radius * angle.cos();
        let ly = orbit_radius * sub_incl.sin() * angle.sin();
        let lz = orbit_radius * sub_incl.cos() * angle.sin();

        let cx = parent_center[0] + lx;
        let cy = parent_center[1] + ly;
        let cz = parent_center[2] + lz;
        let center = [cx, cy, cz];

        let color = if depth == 2 {
            OzPalette::satellite(color_index)
        } else {
            OzPalette::micro(color_index)
        };

        // Sphere (clean — no text)
        let body_idx = out.len();
        out.push(SdfPrimitive::Sphere {
            center,
            radius: node_radius,
            color,
        });
        anim.push(OzAnimMeta {
            depth,
            orbit_radius,
            parent_center,
            angle_offset: angle,
            inclination: sub_incl,
            kind: AnimKind::Orbiter,
        });

        // Connector
        out.push(SdfPrimitive::Line {
            start: parent_center,
            end: center,
            thickness: config.connector_thickness * 0.5_f32.powi(depth as i32 - 1),
            color: OzPalette::connector(),
        });
        anim.push(OzAnimMeta {
            depth,
            orbit_radius,
            parent_center,
            angle_offset: angle,
            inclination: sub_incl,
            kind: AnimKind::Connector { child_index: body_idx },
        });

        // Recurse (no text)
        emit_oz_children_clean(
            out,
            anim,
            child,
            center,
            config,
            color_index,
            depth + 1,
            orbit_radius * 0.4,
        );
    }
}

/// Collect all headline texts from a node's subtree for the ticker ring.
fn collect_headlines_recursive(
    node: &LayoutNode,
    planet_index: usize,
    out: &mut Vec<(usize, String)>,
) {
    let headline = extract_headline(node);
    if !headline.is_empty() {
        out.push((planet_index, headline));
    }
    for child in &node.children {
        collect_headlines_recursive(child, planet_index, out);
    }
}

/// Extract a short label from a node.
fn extract_label(node: &LayoutNode) -> String {
    let text = collect_text(node);
    if !text.is_empty() {
        return text.chars().take(12).collect();
    }
    if !node.tag.is_empty() {
        return node.tag.clone();
    }
    String::new()
}

/// Extract a category name for OZ Orbital Labels.
/// Tries headings first, then tag name, then first few words of text.
fn extract_oz_category(node: &LayoutNode) -> String {
    // Check for heading children (h1-h6)
    for child in &node.children {
        if matches!(
            child.tag.as_str(),
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
        ) {
            let t = collect_text(child);
            if !t.is_empty() {
                return t.chars().take(16).collect();
            }
        }
    }
    // Fall back to semantic tag name
    match node.tag.as_str() {
        "nav" => "NAVIGATION".into(),
        "header" => "HEADER".into(),
        "footer" => "FOOTER".into(),
        "main" => "MAIN".into(),
        "aside" => "SIDEBAR".into(),
        "section" | "article" | "div" => {
            // Try first text content
            let t = collect_text(node);
            if !t.is_empty() {
                // Take first 2 words
                let short: String = t.split_whitespace().take(2).collect::<Vec<_>>().join(" ");
                if !short.is_empty() {
                    return short.chars().take(16).collect();
                }
            }
            node.tag.to_uppercase()
        }
        _ => {
            let t = collect_text(node);
            if !t.is_empty() {
                return t.chars().take(12).collect();
            }
            node.tag.clone()
        }
    }
}

/// Extract headline text from a child node for satellite labels.
/// Returns the first heading or first few words of text content.
fn extract_headline(node: &LayoutNode) -> String {
    // Heading nodes themselves
    if matches!(
        node.tag.as_str(),
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
    ) {
        let t = collect_text(node);
        if !t.is_empty() {
            return t.chars().take(24).collect();
        }
    }
    // Check for heading children
    for child in &node.children {
        if matches!(
            child.tag.as_str(),
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
        ) {
            let t = collect_text(child);
            if !t.is_empty() {
                return t.chars().take(24).collect();
            }
        }
    }
    // Links (often article titles)
    for child in &node.children {
        if child.tag == "a" {
            let t = collect_text(child);
            if !t.is_empty() {
                return t.chars().take(24).collect();
            }
        }
    }
    // First text content
    let t = collect_text(node);
    if !t.is_empty() {
        let short: String = t.split_whitespace().take(4).collect::<Vec<_>>().join(" ");
        return short.chars().take(24).collect();
    }
    String::new()
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  Helpers
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

fn collect_text(node: &LayoutNode) -> String {
    let mut text = String::new();
    if !node.text.is_empty() {
        text.push_str(node.text.trim());
    }
    for child in &node.children {
        let ct = collect_text(child);
        if !ct.is_empty() {
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(&ct);
        }
    }
    text
}
