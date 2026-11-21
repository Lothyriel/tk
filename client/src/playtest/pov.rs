//! Short POV route driven by confirmed raid state, not animation timers.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) enum Ads {
    #[default]
    Rest,
    Aim,
    Release,
    Finish,
}

/// Resolved ECS state can precede GPU mesh/material preparation. Give the
/// ready assembly a short, bounded warm-up before sampling the first pose.
pub(super) const VIEW_WARMUP_SECONDS: f64 = 1.0;

impl Ads {
    pub(super) fn advance(
        &mut self,
        elapsed: f64,
        grounded: bool,
        ads: f32,
        world_fov: f32,
    ) -> Result<Option<&'static str>, &'static str> {
        if elapsed >= 8.0 {
            return Err("POV ADS did not reach its confirmed pose before timeout");
        }
        if !grounded {
            return Ok(None);
        }
        let (next, capture) = match self {
            Self::Rest if ads < 0.02 && (world_fov - 70.0).abs() < 2.0 => (Self::Aim, "pov-rest"),
            Self::Aim if ads > 0.9 && (world_fov - 44.0).abs() < 2.0 => (Self::Release, "pov-ads"),
            Self::Release if ads < 0.02 && (world_fov - 70.0).abs() < 2.0 => {
                (Self::Finish, "pov-return")
            }
            _ => return Ok(None),
        };
        *self = next;
        Ok(Some(capture))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pov_ads_requires_confirmed_state_and_matching_render_lens() {
        let mut route = Ads::default();
        assert_eq!(route.advance(0.1, false, 0.0, 70.0), Ok(None));
        assert_eq!(route.advance(0.2, true, 0.0, 70.0), Ok(Some("pov-rest")));
        assert_eq!(route.advance(0.3, true, 0.95, 70.0), Ok(None));
        assert_eq!(route.advance(0.4, true, 0.0, 44.0), Ok(None));
        assert_eq!(route.advance(0.45, false, 0.95, 44.0), Ok(None));
        assert_eq!(route.advance(0.5, true, 0.95, 44.0), Ok(Some("pov-ads")));
        assert_eq!(route.advance(0.6, true, 0.0, 44.0), Ok(None));
        assert_eq!(route.advance(0.7, true, 0.0, 70.0), Ok(Some("pov-return")));
    }

    #[test]
    fn pov_ads_times_out_without_advancing_a_missing_state() {
        let mut route = Ads::default();
        assert!(route.advance(8.0, false, 0.0, 70.0).is_err());
        assert_eq!(route, Ads::Rest);
    }
}
