//! ALICE-Browser × ALICE-CDN bridge
//!
//! Vivaldi coordinate-based CDN routing for browser resource fetching.
//!
//! Author: Moroya Sakamoto

use alice_cdn::VivaldiCoord;

/// CDN routing decision
#[derive(Debug, Clone)]
pub struct CdnRouteDecision {
    pub edge_node_id: u32,
    pub estimated_rtt_ms: f32,
    pub cache_hit: bool,
}

/// Browser CDN router using Vivaldi coordinates
pub struct BrowserCdnRouter {
    local_coord: VivaldiCoord,
    edge_coords: Vec<(u32, VivaldiCoord)>,
    pub requests_routed: u64,
}

impl BrowserCdnRouter {
    pub fn new(local_coord: VivaldiCoord) -> Self {
        Self {
            local_coord,
            edge_coords: Vec::new(),
            requests_routed: 0,
        }
    }

    /// Register an edge node
    pub fn add_edge_node(&mut self, node_id: u32, coord: VivaldiCoord) {
        self.edge_coords.push((node_id, coord));
    }

    /// Route to nearest edge node by Vivaldi distance
    pub fn route(&mut self, _resource_hash: u64) -> Option<CdnRouteDecision> {
        if self.edge_coords.is_empty() { return None; }
        let (best_id, best_rtt) = self.edge_coords.iter()
            .map(|(id, coord)| (*id, self.local_coord.distance(coord)))
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))?;
        self.requests_routed += 1;
        Some(CdnRouteDecision {
            edge_node_id: best_id,
            estimated_rtt_ms: best_rtt,
            cache_hit: false,
        })
    }

    pub fn edge_count(&self) -> usize {
        self.edge_coords.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coord(x: f32, y: f32) -> VivaldiCoord {
        VivaldiCoord::new(x, y)
    }

    #[test]
    fn test_route_nearest() {
        let mut router = BrowserCdnRouter::new(coord(0.0, 0.0));
        router.add_edge_node(1, coord(1.0, 0.0));
        router.add_edge_node(2, coord(10.0, 0.0));
        let decision = router.route(0xABCD).unwrap();
        assert_eq!(decision.edge_node_id, 1);
    }

    #[test]
    fn test_route_empty() {
        let mut router = BrowserCdnRouter::new(coord(0.0, 0.0));
        assert!(router.route(0).is_none());
    }

    #[test]
    fn test_request_counter() {
        let mut router = BrowserCdnRouter::new(coord(0.0, 0.0));
        router.add_edge_node(1, coord(1.0, 1.0));
        router.route(1);
        router.route(2);
        assert_eq!(router.requests_routed, 2);
    }
}
