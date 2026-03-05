//! Hyper-SDF IFS レンダリング — 万華鏡折り畳みによる超幾何構造生成
//!
//! Kaleidoscopic IFS (Iterated Function System) を用いて、ナノスケールの
//! ディテールを持つ数理的構造を SDF で描画する。

/// IFS 折り畳み設定。
#[derive(Debug, Clone)]
pub struct IfsConfig {
    /// 反復回数 (推奨 8–16)。
    pub iterations: u32,
    /// スケールファクター (> 1.0 で縮小方向)。
    pub scale: f32,
    /// オフセットベクトル。
    pub offset: [f32; 3],
    /// 折り畳み面の法線 (正規化済み)。
    pub fold_normals: Vec<[f32; 3]>,
    /// 折り畳み面の距離パラメータ。
    pub fold_distances: Vec<f32>,
}

impl Default for IfsConfig {
    fn default() -> Self {
        Self {
            iterations: 12,
            scale: 2.0,
            offset: [1.0, 1.0, 1.0],
            fold_normals: vec![
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
                // 正四面体折り畳み面
                [0.577_350_3, 0.577_350_3, 0.577_350_3],
            ],
            fold_distances: vec![0.0, 0.0, 0.0, 0.0],
        }
    }
}

/// 点に対して単一の折り畳みを適用。
///
/// 法線 `n` と距離 `d` で定義される面に対して反射する。
#[must_use]
fn fold_plane(p: [f32; 3], n: [f32; 3], d: f32) -> [f32; 3] {
    let dot = p[2].mul_add(n[2], p[0].mul_add(n[0], p[1] * n[1])) - d;
    if dot < 0.0 {
        [
            (2.0 * dot).mul_add(-n[0], p[0]),
            (2.0 * dot).mul_add(-n[1], p[1]),
            (2.0 * dot).mul_add(-n[2], p[2]),
        ]
    } else {
        p
    }
}

/// 万華鏡 IFS 変換を適用。
///
/// 入力点 `p` に対して、設定された折り畳み面を反復適用し、
/// スケーリングとオフセットを行う。最終的な点と累積スケールを返す。
#[must_use]
pub fn kaleidoscopic_fold(mut p: [f32; 3], config: &IfsConfig) -> ([f32; 3], f32) {
    let mut scale_acc = 1.0_f32;

    for _ in 0..config.iterations {
        // 全折り畳み面を順番に適用
        for (i, normal) in config.fold_normals.iter().enumerate() {
            let d = config.fold_distances.get(i).copied().unwrap_or(0.0);
            p = fold_plane(p, *normal, d);
        }

        // スケーリングとオフセット
        p = [
            p[0].mul_add(config.scale, -config.offset[0] * (config.scale - 1.0)),
            p[1].mul_add(config.scale, -config.offset[1] * (config.scale - 1.0)),
            p[2].mul_add(config.scale, -config.offset[2] * (config.scale - 1.0)),
        ];
        scale_acc *= config.scale;
    }

    (p, scale_acc)
}

/// Hyper-SDF 評価: IFS 変換後の点に対して基本形状の距離を計算。
///
/// 球をベースプリミティブとして使用し、IFS のスケールで割ることで
/// フラクタル構造の距離推定値を得る。
#[must_use]
pub fn hyper_sdf_eval(p: [f32; 3], config: &IfsConfig, base_radius: f32) -> f32 {
    let (folded, scale) = kaleidoscopic_fold(p, config);
    let len = folded[2]
        .mul_add(
            folded[2],
            folded[0].mul_add(folded[0], folded[1] * folded[1]),
        )
        .sqrt();
    (len - base_radius) / scale
}

/// Menger Sponge 用 IFS 設定を生成。
#[must_use]
pub fn menger_config(iterations: u32) -> IfsConfig {
    IfsConfig {
        iterations,
        scale: 3.0,
        offset: [1.0, 1.0, 1.0],
        fold_normals: vec![[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        fold_distances: vec![0.0, 0.0, 0.0],
    }
}

/// Sierpinski 四面体用 IFS 設定を生成。
#[must_use]
pub fn sierpinski_config(iterations: u32) -> IfsConfig {
    IfsConfig {
        iterations,
        scale: 2.0,
        offset: [1.0, 1.0, 1.0],
        fold_normals: vec![
            [0.577_350_3, 0.577_350_3, 0.577_350_3],
            [-0.577_350_3, 0.577_350_3, 0.577_350_3],
            [0.577_350_3, -0.577_350_3, 0.577_350_3],
        ],
        fold_distances: vec![0.0, 0.0, 0.0],
    }
}

/// Hyper-SDF の法線推定 (中央差分)。
#[must_use]
pub fn hyper_normal(p: [f32; 3], config: &IfsConfig, base_radius: f32, eps: f32) -> [f32; 3] {
    let dx = hyper_sdf_eval([p[0] + eps, p[1], p[2]], config, base_radius)
        - hyper_sdf_eval([p[0] - eps, p[1], p[2]], config, base_radius);
    let dy = hyper_sdf_eval([p[0], p[1] + eps, p[2]], config, base_radius)
        - hyper_sdf_eval([p[0], p[1] - eps, p[2]], config, base_radius);
    let dz = hyper_sdf_eval([p[0], p[1], p[2] + eps], config, base_radius)
        - hyper_sdf_eval([p[0], p[1], p[2] - eps], config, base_radius);
    let len = dz.mul_add(dz, dx.mul_add(dx, dy * dy)).sqrt();
    if len < 1e-10 {
        [0.0, 1.0, 0.0]
    } else {
        [dx / len, dy / len, dz / len]
    }
}

/// LOD (Level of Detail) に応じた反復回数を返す。
///
/// カメラからの距離が大きいほど反復回数を減らし、TDR リスクを低減する。
#[must_use]
pub fn lod_iterations(base_iterations: u32, camera_distance: f32, threshold: f32) -> u32 {
    if camera_distance <= threshold {
        return base_iterations;
    }
    let ratio = threshold / camera_distance;
    let reduced = (f64::from(base_iterations) * f64::from(ratio)).ceil() as u32;
    reduced.max(2).min(base_iterations)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_plane_reflects() {
        let p = [1.0, -1.0, 0.0];
        let n = [0.0, 1.0, 0.0];
        let folded = fold_plane(p, n, 0.0);
        assert!((folded[1] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn fold_plane_no_reflect() {
        let p = [1.0, 1.0, 0.0];
        let n = [0.0, 1.0, 0.0];
        let folded = fold_plane(p, n, 0.0);
        assert!((folded[1] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn kaleidoscopic_default_config() {
        let config = IfsConfig::default();
        assert_eq!(config.iterations, 12);
        assert!((config.scale - 2.0).abs() < 1e-6);
    }

    #[test]
    fn kaleidoscopic_fold_produces_result() {
        let config = IfsConfig::default();
        let (folded, scale) = kaleidoscopic_fold([0.5, 0.3, 0.2], &config);
        assert!(scale > 1.0, "Scale should accumulate");
        assert!(folded[0].is_finite());
        assert!(folded[1].is_finite());
        assert!(folded[2].is_finite());
    }

    #[test]
    fn hyper_sdf_finite() {
        let config = IfsConfig {
            iterations: 4,
            ..Default::default()
        };
        let d = hyper_sdf_eval([1.0, 0.0, 0.0], &config, 0.5);
        assert!(d.is_finite());
    }

    #[test]
    fn hyper_sdf_origin_inside() {
        let config = IfsConfig {
            iterations: 4,
            ..Default::default()
        };
        let d = hyper_sdf_eval([0.0, 0.0, 0.0], &config, 2.0);
        // 原点は大きな球の内部にあるはず
        assert!(d < 0.0, "Origin should be inside large sphere, got {d}");
    }

    #[test]
    fn hyper_sdf_far_point_positive() {
        let config = IfsConfig {
            iterations: 4,
            ..Default::default()
        };
        let d = hyper_sdf_eval([100.0, 100.0, 100.0], &config, 0.5);
        assert!(d > 0.0, "Far point should be outside, got {d}");
    }

    #[test]
    fn menger_config_valid() {
        let config = menger_config(5);
        assert_eq!(config.iterations, 5);
        assert!((config.scale - 3.0).abs() < 1e-6);
        assert_eq!(config.fold_normals.len(), 3);
    }

    #[test]
    fn sierpinski_config_valid() {
        let config = sierpinski_config(8);
        assert_eq!(config.iterations, 8);
        assert!((config.scale - 2.0).abs() < 1e-6);
    }

    #[test]
    fn hyper_normal_unit_length() {
        let config = IfsConfig {
            iterations: 4,
            ..Default::default()
        };
        let n = hyper_normal([1.0, 0.0, 0.0], &config, 0.5, 0.001);
        let len = n[2].mul_add(n[2], n[0].mul_add(n[0], n[1] * n[1])).sqrt();
        assert!(
            (len - 1.0).abs() < 0.01,
            "Normal should be unit length, got {len}"
        );
    }

    #[test]
    fn lod_iterations_close() {
        let iters = lod_iterations(12, 1.0, 5.0);
        assert_eq!(iters, 12);
    }

    #[test]
    fn lod_iterations_far() {
        let iters = lod_iterations(12, 20.0, 5.0);
        assert!(iters < 12);
        assert!(iters >= 2);
    }

    #[test]
    fn lod_iterations_minimum() {
        let iters = lod_iterations(12, 1000.0, 1.0);
        assert!(iters >= 2);
    }

    #[test]
    fn kaleidoscopic_scale_accumulates() {
        let config = IfsConfig {
            iterations: 3,
            scale: 2.0,
            ..Default::default()
        };
        let (_, scale) = kaleidoscopic_fold([0.1, 0.1, 0.1], &config);
        assert!((scale - 8.0).abs() < 1e-4, "2^3 = 8, got {scale}");
    }

    #[test]
    fn hyper_sdf_eval_menger() {
        let config = menger_config(3);
        let d = hyper_sdf_eval([0.0, 0.0, 0.0], &config, 1.0);
        assert!(d.is_finite());
    }
}
