//! Tunable gameplay anatomy, not a medical model. All live damage is server applied.
use super::{ArmorState, BodyHealth, BodyPart};
use crate::humanoid::{ray_box, Segment, SegmentPose, SEGMENTS, SEGMENT_COUNT};
use bevy::prelude::*;
use rkyv::{Archive, Deserialize, Serialize};

pub const REGION_MAX: [f32; 7] = [35.0, 85.0, 65.0, 60.0, 60.0, 70.0, 70.0];
pub const SEPARATION_DAMAGE: f32 = 78.0;
pub const BLOOD_COLLAPSE: f32 = 38.0;
pub const BLOOD_DEATH: f32 = 12.0;
pub const TISSUE_PENETRATION_PER_METRE: f32 = 80.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Archive, Serialize, Deserialize)]
#[repr(usize)]
pub enum Region {
    Head,
    Thorax,
    Abdomen,
    LeftArm,
    RightArm,
    LeftLeg,
    RightLeg,
}
pub const REGIONS: [Region; 7] = [
    Region::Head,
    Region::Thorax,
    Region::Abdomen,
    Region::LeftArm,
    Region::RightArm,
    Region::LeftLeg,
    Region::RightLeg,
];
impl Region {
    pub fn of(segment: Segment) -> Self {
        use Segment::*;
        match segment {
            Head | Neck => Self::Head,
            Thorax => Self::Thorax,
            Pelvis | Abdomen => Self::Abdomen,
            LeftUpperArm | LeftForearm | LeftHand => Self::LeftArm,
            RightUpperArm | RightForearm | RightHand => Self::RightArm,
            LeftThigh | LeftShin | LeftFoot => Self::LeftLeg,
            RightThigh | RightShin | RightFoot => Self::RightLeg,
        }
    }
    pub fn armor_part(self) -> BodyPart {
        match self {
            Self::Head => BodyPart::Head,
            Self::Thorax => BodyPart::Thorax,
            Self::LeftArm | Self::RightArm => BodyPart::Arms,
            Self::LeftLeg | Self::RightLeg => BodyPart::Legs,
            Self::Abdomen => BodyPart::Arms,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Archive, Serialize, Deserialize)]
#[repr(usize)]
pub enum Organ {
    Brain,
    Heart,
    Liver,
    Stomach,
    Muscle,
}
pub const ORGANS: [Organ; 4] = [Organ::Brain, Organ::Heart, Organ::Liver, Organ::Stomach];
impl Organ {
    pub fn volume(self) -> (Segment, Vec3, Vec3) {
        match self {
            Self::Brain => (
                Segment::Head,
                Vec3::new(0.0, 0.035, 0.0),
                Vec3::new(0.105, 0.105, 0.10),
            ),
            Self::Heart => (
                Segment::Thorax,
                Vec3::new(-0.065, 0.01, -0.025),
                Vec3::new(0.052, 0.065, 0.06),
            ),
            Self::Liver => (
                Segment::Abdomen,
                Vec3::new(0.08, 0.055, 0.0),
                Vec3::new(0.065, 0.045, 0.065),
            ),
            Self::Stomach => (
                Segment::Abdomen,
                Vec3::new(-0.07, -0.025, 0.0),
                Vec3::new(0.06, 0.055, 0.065),
            ),
            Self::Muscle => (Segment::Pelvis, Vec3::ZERO, Vec3::splat(0.1)),
        }
    }
}

#[derive(Debug, Clone, Archive, Serialize, Deserialize)]
pub struct Anatomy {
    pub regions: [f32; 7],
    pub muscle: [f32; 7],
    pub organs: [f32; 4],
    pub trauma: [f32; SEGMENT_COUNT],
    pub missing: u32,
    pub wounds: u32,
    pub blood: f32,
    pub bleeding: f32,
    pub vigor: f32,
    pub last_region: Option<Region>,
    pub last_organ: Option<Organ>,
}
impl Default for Anatomy {
    fn default() -> Self {
        Self {
            regions: REGION_MAX,
            muscle: [100.0; 7],
            organs: [100.0; 4],
            trauma: [0.0; SEGMENT_COUNT],
            missing: 0,
            wounds: 0,
            blood: 100.0,
            bleeding: 0.0,
            vigor: 100.0,
            last_region: None,
            last_organ: None,
        }
    }
}
impl Anatomy {
    pub fn capacity(&self, region: Region) -> f32 {
        (self.regions[region as usize] / REGION_MAX[region as usize])
            .min(self.muscle[region as usize] / 100.0)
            .clamp(0.0, 1.0)
    }
    pub fn leg_support(&self) -> (f32, f32) {
        (
            self.capacity(Region::LeftLeg),
            self.capacity(Region::RightLeg),
        )
    }
    pub fn handling(&self) -> f32 {
        (self.capacity(Region::LeftArm) + self.capacity(Region::RightArm)) * 0.5
    }
    pub fn speed(&self) -> f32 {
        let (l, r) = self.leg_support();
        (0.2 + 0.8 * l.min(r)).clamp(0.2, 1.0)
    }
    pub fn alive(&self) -> bool {
        self.organs[0] > 0.0 && self.blood > BLOOD_DEATH
    }
    pub fn bleed_rate(&self) -> f32 {
        self.bleeding + (100.0 - self.organs[1]) * 0.045 + (100.0 - self.organs[2]) * 0.025
    }
    pub fn tick(&mut self, dt: f32, exerting: bool) {
        self.blood = (self.blood - self.bleed_rate() * dt).max(0.0);
        let stomach = self.organs[3] / 100.0;
        self.vigor = (self.vigor
            + if exerting {
                -dt * (8.0 + (1.0 - stomach) * 16.0)
            } else {
                dt * (2.0 + stomach * 10.0)
            })
        .clamp(0.0, 100.0);
    }
    /// Returns every newly detached segment exactly once, including descendants.
    pub fn separate(&mut self, segment: Segment) -> u32 {
        if !segment.limb() || self.missing & (1 << segment.index()) != 0 {
            return 0;
        }
        let mut mask = 1 << segment.index();
        for s in SEGMENTS {
            let mut parent = s.parent();
            while let Some(p) = parent {
                if p == segment {
                    mask |= 1 << s.index();
                    break;
                }
                parent = p.parent();
            }
        }
        let new = mask & !self.missing;
        self.missing |= new;
        self.wounds |= 1 << segment.index();
        if let Some(parent) = segment.parent() {
            self.wounds |= 1 << parent.index();
        }
        let region = Region::of(segment) as usize;
        self.regions[region] = 0.0;
        self.muscle[region] = 0.0;
        self.bleeding = (self.bleeding + 0.8).min(5.0);
        new
    }
}

impl BodyHealth {
    /// One impact, one armor evaluation; internal intersections are ordered along the actual ray.
    pub fn hit_anatomy(
        &mut self,
        segment: Segment,
        damage: f32,
        penetration: f32,
        armor: &mut ArmorState,
        origin: Vec3,
        direction: Vec3,
        range: f32,
        pose: &[SegmentPose; SEGMENT_COUNT],
    ) -> u32 {
        if !self.alive()
            || !damage.is_finite()
            || damage <= 0.0
            || self.anatomy.missing & (1 << segment.index()) != 0
        {
            return 0;
        }
        let region = Region::of(segment);
        let part = region.armor_part();
        let stopped = match region {
            Region::Head => armor.helmet > 0.0 && penetration < 22.0 * (armor.helmet / 40.0),
            Region::Thorax => {
                armor.vest > 0.0
                    && armor.kind.durability() > 0.0
                    && penetration
                        < armor.kind.protection() * (armor.vest / armor.kind.durability())
            }
            _ => false,
        };
        let applied = armor.absorb(part, damage, penetration);
        let a = &mut self.anatomy;
        a.regions[region as usize] = (a.regions[region as usize] - applied).max(0.0);
        a.muscle[region as usize] = (a.muscle[region as usize] - applied * 1.2).max(0.0);
        a.last_region = Some(region);
        a.last_organ = Some(Organ::Muscle);
        if !stopped {
            a.wounds |= 1 << segment.index();
            a.trauma[segment.index()] += applied;
            a.bleeding = (a.bleeding + applied * 0.003).min(5.0);
            let entry = ray_box(
                origin,
                direction,
                pose[segment.index()],
                segment.half_size(),
                range,
            )
            .map_or(0.0, |hit| hit.0);
            let reach = (penetration.max(0.0) / TISSUE_PENETRATION_PER_METRE).min(1.2);
            let mut internal: Vec<_> = ORGANS
                .iter()
                .filter_map(|organ| {
                    let (s, offset, half) = organ.volume();
                    if a.missing & (1 << s.index()) != 0 {
                        return None;
                    }
                    let parent = pose[s.index()];
                    let organ_pose = SegmentPose {
                        position: (Vec3::from(parent.position)
                            + Quat::from_array(parent.rotation) * offset)
                            .into(),
                        rotation: parent.rotation,
                    };
                    ray_box(origin, direction, organ_pose, half, range)
                        .filter(|hit| hit.0 + 0.001 >= entry && hit.0 - entry <= reach)
                        .map(|hit| (*organ, hit.0))
                })
                .collect();
            internal.sort_by(|a, b| a.1.total_cmp(&b.1));
            for (organ, _) in internal {
                let multiplier = match organ {
                    Organ::Brain => 3.2,
                    Organ::Heart => 2.4,
                    _ => 1.8,
                };
                a.organs[organ as usize] =
                    (a.organs[organ as usize] - applied * multiplier).max(0.0);
                a.last_organ = Some(organ);
                a.wounds |= 1 << organ.volume().0.index();
            }
        }
        // Compatibility telemetry retains aggregate values; live regional damage does not spill limbs into vitals.
        self.parts = [
            a.regions[0],
            a.regions[1],
            (a.regions[3] + a.regions[4]) * 0.5,
            (a.regions[5] + a.regions[6]) * 0.5,
        ];
        self.last_hit = Some(part);
        if a.trauma[segment.index()] >= SEPARATION_DAMAGE {
            a.separate(segment)
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{combat::*, humanoid::*};
    fn shoot(
        body: &mut BodyHealth,
        segment: Segment,
        x: f32,
        y: f32,
        damage: f32,
        armor: &mut ArmorState,
        penetration: f32,
    ) -> u32 {
        let pose = evaluate_pose(&PoseState::default(), 1.0, 1.0);
        body.hit_anatomy(
            segment,
            damage,
            penetration,
            armor,
            Vec3::new(x, y, -2.0),
            Vec3::Z,
            4.0,
            &pose,
        )
    }
    #[test]
    fn left_and_right_leg_and_arm_impair_independent_capacity() {
        let mut body = BodyHealth::default();
        let mut armor = ArmorState::default();
        shoot(
            &mut body,
            Segment::LeftShin,
            -0.13,
            -0.65,
            35.0,
            &mut armor,
            100.0,
        );
        assert_eq!(body.anatomy.capacity(Region::LeftLeg), 0.5);
        assert_eq!(body.anatomy.capacity(Region::RightLeg), 1.0);
        assert!(body.speed_factor() < 1.0);
        shoot(
            &mut body,
            Segment::RightForearm,
            0.25,
            0.0,
            30.0,
            &mut armor,
            100.0,
        );
        assert_eq!(body.anatomy.handling(), 0.75);
        assert_eq!(body.anatomy.capacity(Region::LeftArm), 1.0);
    }
    #[test]
    fn stopped_armor_never_damages_heart_and_wears_only_once() {
        let mut body = BodyHealth::default();
        let mut armor = ArmorState::from_loadout(Loadout {
            armor: ArmorKind::Heavy,
            ..default()
        });
        shoot(
            &mut body,
            Segment::Thorax,
            -0.065,
            0.21,
            50.0,
            &mut armor,
            10.0,
        );
        assert_eq!(body.anatomy.organs, [100.0; 4]);
        assert_eq!(armor.vest, 80.0);
        assert_eq!(body.anatomy.regions[1], 75.0);
        assert_eq!(body.anatomy.wounds, 0);
    }
    #[test]
    fn brain_ends_control_heart_and_liver_bleed_stomach_changes_vigor() {
        let mut armor = ArmorState::default();
        let mut brain = BodyHealth::default();
        shoot(
            &mut brain,
            Segment::Head,
            0.0,
            0.68,
            34.0,
            &mut armor,
            100.0,
        );
        assert_eq!(brain.anatomy.organs[0], 0.0);
        assert!(!brain.alive());
        let mut heart = BodyHealth::default();
        shoot(
            &mut heart,
            Segment::Thorax,
            -0.065,
            0.21,
            30.0,
            &mut armor,
            100.0,
        );
        assert_eq!(heart.anatomy.organs[1], 28.0);
        assert!(heart.alive());
        let mut liver = BodyHealth::default();
        shoot(
            &mut liver,
            Segment::Abdomen,
            0.08,
            -0.045,
            20.0,
            &mut armor,
            100.0,
        );
        assert_eq!(liver.anatomy.organs[2], 64.0);
        assert!(liver.anatomy.bleed_rate() > 0.9);
        let mut stomach = BodyHealth::default();
        shoot(
            &mut stomach,
            Segment::Abdomen,
            -0.07,
            -0.125,
            20.0,
            &mut armor,
            100.0,
        );
        assert_eq!(stomach.anatomy.organs[3], 64.0);
        stomach.anatomy.tick(1.0, true);
        let mut healthy = Anatomy::default();
        healthy.tick(1.0, true);
        assert!(stomach.anatomy.vigor < healthy.vigor);
        heart.anatomy.tick(20.0, false);
        assert!(heart.anatomy.blood < BLOOD_COLLAPSE);
    }
    #[test]
    fn separation_invalidates_descendants_and_is_idempotent() {
        let mut a = Anatomy::default();
        let mask = a.separate(Segment::LeftUpperArm);
        assert_eq!(mask, (1 << 5) | (1 << 6) | (1 << 7));
        assert_eq!(a.separate(Segment::LeftUpperArm), 0);
        assert_eq!(a.capacity(Region::LeftArm), 0.0);
        assert_eq!(a.capacity(Region::RightArm), 1.0);
    }
    #[test]
    fn damaging_the_second_leg_never_restores_movement_capacity() {
        let mut body = BodyHealth::default();
        let mut armor = ArmorState::default();
        shoot(
            &mut body,
            Segment::LeftShin,
            -0.13,
            -0.65,
            80.0,
            &mut armor,
            100.0,
        );
        let before = body.speed_factor();
        assert_eq!(before, 0.2);
        shoot(
            &mut body,
            Segment::RightShin,
            0.13,
            -0.65,
            35.0,
            &mut armor,
            100.0,
        );
        assert_eq!(body.speed_factor(), 0.2);
        assert!(body.speed_factor() <= before);
    }
}
