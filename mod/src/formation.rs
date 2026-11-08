//! Where each spawned character goes. Pure code (unit-tested). Positions are
//! in Havok space: X/Z horizontal, Y up; a yaw θ faces (sin θ, cos θ).

use crate::config::Formation;

pub const SPACING: f32 = 1.5;

/// Horizontal unit forward vector (x, z) of an XYZW orientation quaternion
/// (+Z rotated by it). Confirmed in game in phase 0.
pub fn forward_of(q: [f32; 4]) -> [f32; 2] {
    let [x, y, z, w] = q;
    let f = [2.0 * (x * z + w * y), 1.0 - 2.0 * (x * x + y * y)];
    let len = (f[0] * f[0] + f[1] * f[1]).sqrt();
    if len < 1e-4 { [0.0, 1.0] } else { [f[0] / len, f[1] / len] }
}

/// Yaw that looks from `from` towards `to`.
pub fn yaw_towards(from: [f32; 3], to: [f32; 3]) -> f32 {
    (to[0] - from[0]).atan2(to[2] - from[2])
}

/// XYZW quaternion of a pure yaw.
pub fn yaw_quat(yaw: f32) -> [f32; 4] {
    [0.0, (yaw / 2.0).sin(), 0.0, (yaw / 2.0).cos()]
}

/// Small deterministic generator for the Random placement.
pub struct Lcg(pub u64);

impl Lcg {
    pub fn next_f32(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 40) as f32) / ((1u64 << 24) as f32)
    }
}

/// `count` positions around `center`, laid out in `formation`, oriented by
/// the player's horizontal `forward`.
pub fn positions(center: [f32; 3], forward: [f32; 2], count: u32, formation: Formation) -> Vec<[f32; 3]> {
    let n = count.max(1) as usize;
    let right = [forward[1], -forward[0]];
    let at = |side: f32, ahead: f32| {
        [
            center[0] + right[0] * side + forward[0] * ahead,
            center[1],
            center[2] + right[1] * side + forward[1] * ahead,
        ]
    };
    match formation {
        Formation::Line => (0..n)
            .map(|i| at((i as f32 - (n as f32 - 1.0) / 2.0) * SPACING, 0.0))
            .collect(),
        Formation::Circle => {
            if n == 1 {
                return vec![center];
            }
            let r = (n as f32 * SPACING / std::f32::consts::TAU).max(SPACING);
            (0..n)
                .map(|i| {
                    let a = i as f32 / n as f32 * std::f32::consts::TAU;
                    at(r * a.sin(), r * a.cos())
                })
                .collect()
        }
        Formation::Grid => {
            let cols = n.min(4);
            (0..n)
                .map(|i| {
                    let (row, col) = (i / cols, i % cols);
                    at((col as f32 - (cols as f32 - 1.0) / 2.0) * SPACING, row as f32 * SPACING)
                })
                .collect()
        }
    }
}

/// `count` random points within `radius` of `center` (at least 2 m away).
pub fn random_positions(center: [f32; 3], radius: f32, count: u32, rng: &mut Lcg) -> Vec<[f32; 3]> {
    let radius = radius.max(2.0);
    (0..count.max(1))
        .map(|_| {
            let a = rng.next_f32() * std::f32::consts::TAU;
            let d = 2.0 + rng.next_f32() * (radius - 2.0);
            [center[0] + d * a.sin(), center[1], center[2] + d * a.cos()]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn forward_matches_phase0_log() {
        // Phase 0: orientation (0, -0.8616291, 0, 0.5075384); a spawn 5 m ahead
        // landed where the player was looking.
        let f = forward_of([0.0, -0.8616291, 0.0, 0.5075384]);
        assert!(close(f[0] * f[0] + f[1] * f[1], 1.0));
        let yaw = yaw_quat(f[0].atan2(f[1]));
        assert!(close(forward_of(yaw)[0], f[0]) && close(forward_of(yaw)[1], f[1]));
    }

    #[test]
    fn yaw_towards_round_trip() {
        let from = [0.0, 0.0, 0.0];
        let to = [3.0, 7.0, 4.0];
        let f = forward_of(yaw_quat(yaw_towards(from, to)));
        assert!(close(f[0], 0.6) && close(f[1], 0.8));
    }

    #[test]
    fn line_is_centred_and_spaced() {
        let p = positions([10.0, 1.0, 10.0], [0.0, 1.0], 3, Formation::Line);
        assert_eq!(p.len(), 3);
        assert!(close(p[1][0], 10.0) && close(p[1][2], 10.0));
        assert!(close((p[0][0] - p[2][0]).abs(), 2.0 * SPACING));
        assert!(p.iter().all(|q| close(q[1], 1.0)));
    }

    #[test]
    fn circle_keeps_radius() {
        let c = [0.0, 0.0, 0.0];
        let p = positions(c, [1.0, 0.0], 8, Formation::Circle);
        let r0 = (p[0][0].powi(2) + p[0][2].powi(2)).sqrt();
        assert!(p.iter().all(|q| close((q[0].powi(2) + q[2].powi(2)).sqrt(), r0)));
        assert!(r0 >= SPACING);
        assert_eq!(positions(c, [1.0, 0.0], 1, Formation::Circle), vec![c]);
    }

    #[test]
    fn grid_rows_of_four() {
        let p = positions([0.0, 0.0, 0.0], [0.0, 1.0], 6, Formation::Grid);
        assert_eq!(p.len(), 6);
        assert!(close(p[4][2], SPACING) && close(p[0][2], 0.0));
    }

    #[test]
    fn random_within_radius() {
        let mut rng = Lcg(42);
        let p = random_positions([0.0, 0.0, 0.0], 10.0, 50, &mut rng);
        assert!(p.iter().all(|q| {
            let d = (q[0].powi(2) + q[2].powi(2)).sqrt();
            (2.0 - 1e-3..=10.0 + 1e-3).contains(&d)
        }));
    }
}
