//! Shared recoil model. Only authority injects impulses; clients may advance a
//! received state for presentation. Angles are radians, velocities rad/s.
use rkyv::{Archive, Deserialize, Serialize};

use crate::{catalog::ResolvedAssembly, CameraInput};

/// Gameplay transfer from ammunition momentum to a controlled angular impulse.
/// This is not a rigid-body firearm simulation or a claim of measured recoil.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecoilProfile {
    pub pitch_impulse: f32,
    pub yaw_impulse: f32,
    pub return_rate: f32,
}

impl RecoilProfile {
    pub fn resolve(assembly: &ResolvedAssembly, impulse_ns: f32, control_gain: f32) -> Self {
        // A forward first moment resists rotation but slows recovery. Explicit
        // stock/grip/device modifiers multiply the transfer, not projectile spread.
        let leverage = (assembly.balance_kg_m / assembly.mass_kg).clamp(0.0, 0.6);
        let pitch =
            (impulse_ns * assembly.velocity_multiplier * control_gain * assembly.recoil_multiplier
                / (assembly.mass_kg * (1.0 + leverage * 2.0)))
                .clamp(0.15, 2.4);
        Self {
            pitch_impulse: pitch,
            yaw_impulse: pitch * 0.28,
            return_rate: (14.0 / (1.0 + leverage)).clamp(9.0, 14.0),
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Archive, Serialize, Deserialize)]
pub struct RecoilState {
    /// X yaw, Y pitch. Separate from voluntary CameraInput.
    pub angles: [f32; 2],
    pub velocity: [f32; 2],
    pub impulses: u32,
}

/// Eight-byte snapshot, independent of server-only impulse ordinal. Resolution:
/// angles 1/32768 rad, velocities 1/4096 rad/s; limits cover the bounded spring.
#[derive(Debug, Default, Clone, Copy, PartialEq, Archive, Serialize, Deserialize)]
pub struct PackedRecoil {
    angles: [i16; 2],
    velocity: [i16; 2],
}

impl PackedRecoil {
    pub fn pack(state: RecoilState) -> Self {
        Self {
            angles: state.angles.map(|v| (v * 32768.0).round() as i16),
            velocity: state.velocity.map(|v| (v * 4096.0).round() as i16),
        }
    }

    pub fn unpack(self) -> RecoilState {
        RecoilState {
            angles: self.angles.map(|v| f32::from(v) / 32768.0),
            velocity: self.velocity.map(|v| f32::from(v) / 4096.0),
            impulses: 0,
        }
    }
}

impl RecoilState {
    /// Closed-form critically damped spring. Partitioning an interval into
    /// 30/60/165 Hz steps does not change the trajectory between impulses.
    pub fn advance(&mut self, seconds: f32, rate: f32) {
        if !seconds.is_finite() || seconds <= 0.0 {
            return;
        }
        let decay = (-rate * seconds).exp();
        for (x, v) in self.angles.iter_mut().zip(&mut self.velocity) {
            let c = *v + rate * *x;
            *x = (*x + c * seconds) * decay;
            *v = (*v - rate * c * seconds) * decay;
        }
        for (axis, low, high) in [(0, -0.035, 0.035), (1, 0.0, 0.12)] {
            let bounded = self.angles[axis].clamp(low, high);
            if bounded != self.angles[axis] {
                self.angles[axis] = bounded;
                self.velocity[axis] = 0.0;
            }
        }
    }

    /// Invoke exactly once after an authorized projectile is created. Seed is
    /// actor/raid-stable; the per-actor ordinal avoids other shooters changing it.
    pub fn impulse(&mut self, profile: RecoilProfile, seed: u64) {
        const PATTERN: [f32; 8] = [-0.18, 0.12, 0.24, -0.30, -0.10, 0.32, 0.10, -0.22];
        let index = self.impulses;
        let hash = seed as u32 ^ (seed >> 32) as u32 ^ index.wrapping_mul(0x9e37_79b9);
        let side = (PATTERN[index as usize % PATTERN.len()] + jitter(hash) * 0.08).clamp(-0.4, 0.4);
        let vertical = 1.0 + jitter(hash ^ 0xa511_e9b3) * 0.08;
        self.velocity[0] = (self.velocity[0] + side * profile.yaw_impulse).clamp(-1.5, 1.5);
        self.velocity[1] = (self.velocity[1] + vertical * profile.pitch_impulse).clamp(0.0, 5.0);
        self.impulses = self.impulses.wrapping_add(1);
    }

    pub fn camera(&self, voluntary: CameraInput) -> CameraInput {
        CameraInput {
            yaw: voluntary.yaw + self.angles[0],
            pitch: (voluntary.pitch + self.angles[1]).clamp(-1.56, 1.56),
            roll: voluntary.roll,
        }
    }
}

fn jitter(mut bits: u32) -> f32 {
    bits ^= bits >> 16;
    bits = bits.wrapping_mul(0x7feb_352d);
    bits ^= bits >> 15;
    bits = bits.wrapping_mul(0x846c_a68b);
    bits ^= bits >> 16;
    bits as f32 / u32::MAX as f32 * 2.0 - 1.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        catalog::{Catalog, CatalogDocument, CATALOG_SOURCE},
        WeaponId,
    };

    #[test]
    fn recoil_snapshot_roundtrip_bounds_quantization_error_and_wire_cost() {
        for angles in [[-0.0349, 0.1599], [0.00237, 0.05372], [0.0, 0.0]] {
            let state = RecoilState {
                angles,
                velocity: [-1.4999, 4.9999],
                impulses: 30,
            };
            let bytes = crate::data::encode(&PackedRecoil::pack(state));
            assert_eq!(bytes.len(), 8);
            let restored: PackedRecoil = crate::data::decode(&bytes);
            let restored = restored.unpack();
            for axis in 0..2 {
                assert!((restored.angles[axis] - state.angles[axis]).abs() <= 1.0 / 65536.0);
                assert!((restored.velocity[axis] - state.velocity[axis]).abs() <= 1.0 / 8192.0);
            }
        }
    }

    #[test]
    fn recoil_spring_matches_closed_form_at_all_render_rates() {
        // Independent oracle: critically damped response to initial velocity.
        let profile = WeaponId::M4A1.spec().recoil;
        for hz in [30, 60, 165] {
            let mut state = RecoilState {
                velocity: [0.3, 1.2],
                ..Default::default()
            };
            for _ in 0..hz {
                state.advance(1.0 / hz as f32, profile.return_rate);
            }
            for (axis, impulse) in [0.3, 1.2].into_iter().enumerate() {
                let expected = impulse * (-profile.return_rate).exp();
                assert!((state.angles[axis] - expected).abs() < 1e-6);
                let velocity = impulse * (1.0 - profile.return_rate) * (-profile.return_rate).exp();
                assert!((state.velocity[axis] - velocity).abs() < 1e-6);
            }
        }
    }

    #[test]
    fn recoil_mass_balance_velocity_and_control_have_explicit_directions() {
        let base = WeaponId::M4A1.spec();
        let mut assembly = base.assembly.clone();
        let resolve = |a: &ResolvedAssembly| RecoilProfile::resolve(a, 3.6, 0.9);
        let ordinary = resolve(&assembly);
        assembly.mass_kg *= 2.0;
        assert!(resolve(&assembly).pitch_impulse < ordinary.pitch_impulse * 0.55);
        assembly = base.assembly.clone();
        assembly.balance_kg_m += assembly.mass_kg * 0.2;
        assert!(resolve(&assembly).pitch_impulse < ordinary.pitch_impulse);
        assert!(resolve(&assembly).return_rate < ordinary.return_rate);
        assembly = base.assembly.clone();
        assembly.recoil_multiplier = 0.8;
        assert!((resolve(&assembly).pitch_impulse / ordinary.pitch_impulse - 0.8).abs() < 1e-6);
        assembly = base.assembly.clone();
        assembly.velocity_multiplier = 1.1;
        assert!((resolve(&assembly).pitch_impulse / ordinary.pitch_impulse - 1.1).abs() < 1e-6);
    }

    #[test]
    fn recoil_burst_counts_each_impulse_and_repeats_seeded_trajectory() {
        let profile = WeaponId::M4A1.spec().recoil;
        let mut state = RecoilState::default();
        let mut replay = state;
        for _ in 0..30 {
            state.impulse(profile, 42);
            replay.impulse(profile, 42);
            for _ in 0..13 {
                state.advance(1.0 / 128.0, profile.return_rate);
                replay.advance(1.0 / 128.0, profile.return_rate);
                assert_eq!(state, replay);
                assert!((0.0..0.16).contains(&state.angles[1]));
                assert!(state.angles[0].abs() < 0.035);
            }
        }
        assert_eq!(state.impulses, 30);
        state.advance(1.5, profile.return_rate);
        assert!(state.angles.iter().all(|a| a.abs() < 1e-5));
        let mut burst = RecoilState::default();
        for _ in 0..3 {
            burst.impulse(profile, 42);
        }
        assert_eq!(burst.impulses, 3);
        assert!(burst.velocity[1] > profile.pitch_impulse * 2.7);
        let aim = CameraInput {
            pitch: 0.2,
            yaw: 0.4,
            roll: 0.1,
        };
        let state = RecoilState {
            angles: [0.01, 0.03],
            ..Default::default()
        };
        let camera = state.camera(aim);
        assert!((camera.pitch - 0.23).abs() < 1e-6);
        assert_eq!(aim.pitch, 0.2);
        assert_eq!(camera.roll, aim.roll);
    }

    #[test]
    fn recoil_extreme_cadence_stays_inside_gameplay_and_snapshot_limits() {
        let profile = RecoilProfile {
            pitch_impulse: 2.4,
            yaw_impulse: 0.672,
            return_rate: 9.0,
        };
        let mut state = RecoilState::default();
        for _ in 0..8192 {
            state.impulse(profile, 13);
            state.advance(1.0 / 128.0, profile.return_rate);
            assert!((0.0..=0.12).contains(&state.angles[1]));
            assert!(state.angles[0].abs() <= 0.035);
            let packed = PackedRecoil::pack(state).unpack();
            assert!((packed.angles[1] - state.angles[1]).abs() <= 1.0 / 65536.0);
        }
        assert_eq!(state.impulses, 8192);
        state.advance(2.0, profile.return_rate);
        assert!(state.angles.iter().all(|a| a.abs() < 1e-6));
    }

    #[test]
    fn recoil_catalog_rejects_invalid_momentum_and_transfer() {
        for value in [f32::NAN, 0.0, -1.0, 101.0] {
            let mut document: CatalogDocument = ron::from_str(CATALOG_SOURCE).unwrap();
            document.ammo[0].recoil_impulse_ns = value;
            assert!(Catalog::validate("fixture", document)
                .unwrap_err()
                .field
                .contains("recoil_impulse"));
        }
        for value in [f32::INFINITY, 0.0, 11.0] {
            let mut document: CatalogDocument = ron::from_str(CATALOG_SOURCE).unwrap();
            document.profiles[0].recoil_control = value;
            assert!(Catalog::validate("fixture", document)
                .unwrap_err()
                .field
                .contains("recoil_control"));
        }
    }
}
