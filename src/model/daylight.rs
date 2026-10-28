//! How much daylight reaches a room, from the sun's position.

use serde::Deserialize;

use crate::sun::SunPosition;

/// How much daylight a room gets. See DESIGN.md, "Room daylight".
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub(crate) struct Daylight {
    /// 0–1: window area and shading.
    pub amount: f64,
    /// Window orientations as azimuth, degrees clockwise from north.
    pub facing: Vec<f64>,
}

/// The sky part is zero from the end of civil twilight and full from 30°.
const SKY_DARK_ELEVATION: f64 = -6.0;
const SKY_FULL_ELEVATION: f64 = 30.0;
/// The direct part fades in over the first degrees above the horizon instead
/// of jumping in at sunrise.
const DIRECT_FADE_IN_ELEVATION: f64 = 5.0;

impl Daylight {
    /// Daylight reaching the room, 0–1.
    pub(crate) fn at(&self, sun: SunPosition) -> f64 {
        let sky = sky_light(sun.elevation);
        let direct = direct_light(sun.elevation) * self.windows_facing(sun.azimuth);
        (self.amount * (sky + direct)).min(1.0)
    }

    /// How squarely the windows face the sun's azimuth, averaged over the
    /// windows: 1 head-on, 0 side-on or behind.
    fn windows_facing(&self, azimuth: f64) -> f64 {
        let facing_sun = |facing: &f64| (azimuth - facing).to_radians().cos().max(0.0);
        let total: f64 = self.facing.iter().map(facing_sun).sum();
        total / self.facing.len() as f64
    }
}

/// Light from the sky, 0–1, whatever the windows face.
fn sky_light(elevation: f64) -> f64 {
    let progress = (elevation - SKY_DARK_ELEVATION) / (SKY_FULL_ELEVATION - SKY_DARK_ELEVATION);
    ease(progress)
}

/// Direct sunlight on a window facing the sun, 0–1: strongest low over
/// the horizon, where it shines in flat.
fn direct_light(elevation: f64) -> f64 {
    let fade_in = ease(elevation / DIRECT_FADE_IN_ELEVATION);
    fade_in * elevation.to_radians().cos()
}

/// Cosine ease from 0 to 1 while `x` goes from 0 to 1; flat outside.
fn ease(x: f64) -> f64 {
    (1.0 - (std::f64::consts::PI * x.clamp(0.0, 1.0)).cos()) / 2.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sun(elevation: f64, azimuth: f64) -> SunPosition {
        SunPosition { elevation, azimuth }
    }

    #[test]
    fn daylight_follows_the_sun_and_the_windows() {
        let east = Daylight {
            amount: 0.4,
            facing: vec![90.0],
        };
        let west = Daylight {
            amount: 0.4,
            facing: vec![270.0],
        };
        let north = Daylight {
            amount: 0.4,
            facing: vec![0.0],
        };

        assert_eq!(east.at(sun(-10.0, 30.0)), 0.0, "night");
        let morning = sun(15.0, 100.0);
        assert!(
            east.at(morning) > west.at(morning) + 0.2,
            "{} vs {}",
            east.at(morning),
            west.at(morning)
        );
        let evening = sun(15.0, 260.0);
        assert!(
            west.at(evening) > east.at(evening) + 0.2,
            "{} vs {}",
            west.at(evening),
            east.at(evening)
        );

        let noon = sun(30.0, 180.0);
        assert_eq!(north.at(noon), 0.4, "sun behind the window: sky part only");
        assert!(
            (east.at(noon) - north.at(noon)).abs() < 1e-9,
            "sun exactly side-on"
        );
    }

    #[test]
    fn daylight_has_no_jump_at_sunrise_and_is_capped() {
        let east = Daylight {
            amount: 1.0,
            facing: vec![90.0],
        };
        let step = east.at(sun(0.1, 90.0)) - east.at(sun(-0.1, 90.0));
        assert!(step < 0.01, "jump of {step} at sunrise");
        assert_eq!(east.at(sun(20.0, 90.0)), 1.0);
    }

    #[test]
    fn several_windows_average_their_direct_light() {
        let south_west = Daylight {
            amount: 0.4,
            facing: vec![180.0, 270.0],
        };
        let south = Daylight {
            amount: 0.4,
            facing: vec![180.0],
        };
        let afternoon = sun(20.0, 225.0);
        assert!(
            (south_west.at(afternoon) - south.at(afternoon)).abs() < 1e-9,
            "both windows see the sun at 45°"
        );
    }
}
