//! Compact, mode-independent readout of the accepted actor snapshot.
use std::fmt::Write;

use common::{
    ClientData,
    combat::{Anatomy, BLOOD_COLLAPSE, REGION_MAX, REGIONS, Region},
    humanoid::PhysicalMode,
};

fn region_name(region: Region) -> &'static str {
    match region {
        Region::Head => "Head",
        Region::Thorax => "Thorax",
        Region::Abdomen => "Abdomen",
        Region::LeftArm => "Left arm",
        Region::RightArm => "Right arm",
        Region::LeftLeg => "Left leg",
        Region::RightLeg => "Right leg",
    }
}

fn append_health(text: &mut String, anatomy: &Anatomy) {
    let current = std::array::from_fn::<_, 7, _>(|i| anatomy.regions[i].clamp(0.0, REGION_MAX[i]));
    let remaining: [f32; 7] = std::array::from_fn(|i| current[i] / REGION_MAX[i]);
    let mut order = REGIONS;
    // REGIONS order is the permanent tiebreaker, regardless of hit history.
    order.sort_unstable_by(|a, b| {
        remaining[*a as usize]
            .total_cmp(&remaining[*b as usize])
            .then_with(|| (*a as usize).cmp(&(*b as usize)))
    });
    let hp: f32 = current.iter().sum();
    let max: f32 = REGION_MAX.iter().sum();
    let blood = anatomy.blood;
    let bleed = anatomy.bleed_rate();
    let _ = write!(text, "HP {hp:.0}/{max:.0}  |  Blood {blood:.0}%");
    if blood <= BLOOD_COLLAPSE {
        text.push_str(" · LOW BLOOD");
    }
    if bleed > 0.0 && bleed < 0.01 {
        text.push_str("\nBLEEDING <0.01%/s");
    } else if bleed >= 0.01 {
        let _ = write!(text, "\nBLEEDING {bleed:.2}%/s");
    }
    for region in order
        .into_iter()
        .filter(|region| remaining[*region as usize] < 1.0)
        .take(3)
    {
        let fraction = remaining[region as usize];
        let severity = if fraction == 0.0 {
            "Destroyed"
        } else if fraction <= 0.25 {
            "Critical"
        } else if fraction <= 0.5 {
            "Severe"
        } else {
            "Wounded"
        };
        // Rounding must not make an injured region look healthy or destroyed.
        let percent = if fraction == 0.0 {
            0.0
        } else {
            (fraction * 100.0).clamp(1.0, 99.0)
        };
        let _ = write!(
            text,
            "\n{}: {severity} ({percent:.0}%)",
            region_name(region)
        );
    }
}

pub(super) fn append_readout(text: &mut String, local: &ClientData) {
    let anatomy = &local.body.anatomy;
    append_health(text, anatomy);
    let state = match local.pose.mode {
        PhysicalMode::Standing => None,
        PhysicalMode::Stumbling => Some("Stumbling"),
        PhysicalMode::Fallen => Some("Fallen"),
        PhysicalMode::Recovering => Some("Getting up"),
        PhysicalMode::Dead => Some("Dead"),
    };
    if let Some(state) = state {
        let _ = write!(text, "\nStatus: {state}");
    }
    if anatomy.capacity(Region::RightArm) <= 0.0 {
        text.push_str("\nTrigger arm disabled");
    }

    let mechanics = local.mechanics;
    let _ = write!(
        text,
        "\n{}  |  {:?}",
        local.weapon_id.spec().name,
        mechanics.mode
    );
    if mechanics.reloading() {
        text.push_str("  |  Reloading");
    }
    let chamber = if mechanics.chambered {
        "loaded"
    } else {
        "empty"
    };
    let _ = write!(text, "\nChamber {chamber} | Magazine ");
    if let Some(magazine) = mechanics.inserted_magazine() {
        let _ = write!(text, "{}", magazine.rounds());
    } else if mechanics.magazine_inserted {
        text.push_str("unknown");
    } else {
        text.push_str("absent");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combat_hud_reserves_percentage_endpoints_for_true_region_states() {
        let mut actor = ClientData::default();
        actor.body.anatomy.regions[Region::Head as usize] = 34.9;
        actor.body.anatomy.regions[Region::RightLeg as usize] = 0.1;
        actor.body.anatomy.regions[Region::LeftLeg as usize] = 0.0;
        let mut text = String::new();
        append_readout(&mut text, &actor);
        let wounds: Vec<_> = text.lines().skip(1).take(3).collect();
        assert_eq!(
            wounds,
            [
                "Left leg: Destroyed (0%)",
                "Right leg: Critical (1%)",
                "Head: Wounded (99%)",
            ]
        );
    }

    #[test]
    fn combat_hud_orders_top_three_by_fraction_and_uses_anatomical_ties_after_recovery() {
        let mut anatomy = Anatomy::default();
        let mut text = String::new();
        append_health(&mut text, &anatomy);
        assert_eq!(text, "HP 445/445  |  Blood 100%");

        anatomy.regions[Region::Head as usize] = 17.5;
        anatomy.regions[Region::Thorax as usize] = 42.5;
        anatomy.regions[Region::Abdomen as usize] = 39.0;
        anatomy.regions[Region::RightLeg as usize] = 7.0;
        anatomy.last_region = Some(Region::Thorax);
        text.clear();
        append_health(&mut text, &anatomy);
        assert_eq!(
            text,
            "HP 296/445  |  Blood 100%\nRight leg: Critical (10%)\nHead: Severe (50%)\nThorax: Severe (50%)"
        );
        anatomy.last_region = Some(Region::Head);
        let mut stable = String::new();
        append_health(&mut stable, &anatomy);
        assert_eq!(stable, text);

        anatomy.regions[Region::Head as usize] = 35.0;
        text.clear();
        append_health(&mut text, &anatomy);
        assert_eq!(
            text,
            "HP 314/445  |  Blood 100%\nRight leg: Critical (10%)\nThorax: Severe (50%)\nAbdomen: Wounded (60%)"
        );
    }

    #[test]
    fn combat_hud_labels_bleeding_and_vital_state_separately_from_regional_hp() {
        let mut actor = ClientData::default();
        actor.body.anatomy.blood = 30.0;
        actor.body.anatomy.bleeding = 2.0;
        actor.pose.mode = PhysicalMode::Fallen;
        let mut text = String::new();
        append_readout(&mut text, &actor);
        assert!(
            text.starts_with(
                "HP 445/445  |  Blood 30% · LOW BLOOD\nBLEEDING 2.00%/s\nStatus: Fallen"
            )
        );
        assert!(!text.contains("Critical"));
    }

    #[test]
    fn combat_hud_distinguishes_tiny_bleeding_from_no_bleeding_at_same_hp() {
        let mut actor = ClientData::default();
        actor.body.anatomy.blood = 30.0;
        actor.body.anatomy.bleeding = 0.003;
        actor.pose.mode = PhysicalMode::Recovering;
        let mut text = String::new();
        append_readout(&mut text, &actor);
        assert_eq!(
            text.lines().take(3).collect::<Vec<_>>(),
            [
                "HP 445/445  |  Blood 30% · LOW BLOOD",
                "BLEEDING <0.01%/s",
                "Status: Getting up",
            ]
        );

        actor.body.anatomy.bleeding = 0.01;
        text.clear();
        append_readout(&mut text, &actor);
        assert_eq!(text.lines().nth(1), Some("BLEEDING 0.01%/s"));

        actor.body.anatomy.bleeding = 0.0;
        actor.pose.mode = PhysicalMode::Standing;
        text.clear();
        append_readout(&mut text, &actor);
        assert!(text.starts_with("HP 445/445  |  Blood 30% · LOW BLOOD\n"));
        assert!(!text.contains("BLEEDING"));
        assert!(!text.contains("Status:"));
    }
}
