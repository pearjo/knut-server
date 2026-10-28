//! Solar position and sunrise/sunset, after NOAA's solar calculator.
//! Accurate to about a minute for the next decades, which is plenty for
//! lighting.

use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SunPosition {
    /// Degrees above the horizon, negative below it. No refraction correction.
    pub elevation: f64,
    /// Degrees clockwise from north.
    pub azimuth: f64,
}

/// Sunrise and sunset of one day; `None` during polar day or night.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SunTimes {
    pub sunrise: DateTime<Utc>,
    pub sunset: DateTime<Utc>,
}

/// The sun's apparent elevation at sunrise/sunset: its radius plus
/// atmospheric refraction.
const HORIZON_ZENITH: f64 = 90.833;

struct SolarParameters {
    declination: f64,
    /// Equation of time in minutes.
    equation_of_time: f64,
}

fn julian_century(time: DateTime<Utc>) -> f64 {
    let julian_day = time.timestamp() as f64 / 86_400.0 + 2_440_587.5;
    (julian_day - 2_451_545.0) / 36_525.0
}

fn solar_parameters(time: DateTime<Utc>) -> SolarParameters {
    let t = julian_century(time);
    let mean_longitude = (280.46646 + t * (36000.76983 + t * 0.0003032)).rem_euclid(360.0);
    let mean_anomaly = 357.52911 + t * (35999.05029 - 0.0001537 * t);
    let eccentricity = 0.016708634 - t * (0.000042037 + 0.0000001267 * t);
    let m = mean_anomaly.to_radians();
    let equation_of_centre = m.sin() * (1.914602 - t * (0.004817 + 0.000014 * t))
        + (2.0 * m).sin() * (0.019993 - 0.000101 * t)
        + (3.0 * m).sin() * 0.000289;
    let true_longitude = mean_longitude + equation_of_centre;
    let omega = (125.04 - 1934.136 * t).to_radians();
    let apparent_longitude = true_longitude - 0.00569 - 0.00478 * omega.sin();
    let mean_obliquity =
        23.0 + (26.0 + (21.448 - t * (46.815 + t * (0.00059 - t * 0.001813))) / 60.0) / 60.0;
    let obliquity = (mean_obliquity + 0.00256 * omega.cos()).to_radians();
    let declination = (obliquity.sin() * apparent_longitude.to_radians().sin()).asin();

    let y = (obliquity / 2.0).tan().powi(2);
    let l = mean_longitude.to_radians();
    let equation_of_time = 4.0
        * (y * (2.0 * l).sin() - 2.0 * eccentricity * m.sin()
            + 4.0 * eccentricity * y * m.sin() * (2.0 * l).cos()
            - 0.5 * y * y * (4.0 * l).sin()
            - 1.25 * eccentricity * eccentricity * (2.0 * m).sin())
        .to_degrees();
    SolarParameters {
        declination,
        equation_of_time,
    }
}

/// Where the sun is at `time`, seen from `lat`, `lon` (degrees).
pub(crate) fn position(lat: f64, lon: f64, time: DateTime<Utc>) -> SunPosition {
    let params = solar_parameters(time);
    let minutes = time.timestamp().rem_euclid(86_400) as f64 / 60.0;
    let true_solar_time = (minutes + params.equation_of_time + 4.0 * lon).rem_euclid(1440.0);
    let hour_angle = true_solar_time / 4.0 - 180.0;

    let lat_r = lat.to_radians();
    let cos_zenith = lat_r.sin() * params.declination.sin()
        + lat_r.cos() * params.declination.cos() * hour_angle.to_radians().cos();
    let zenith = cos_zenith.clamp(-1.0, 1.0).acos();

    SunPosition {
        elevation: 90.0 - zenith.to_degrees(),
        azimuth: azimuth(lat_r, zenith, params.declination, hour_angle),
    }
}

/// Degrees clockwise from north, from the latitude, zenith and declination
/// in radians and the hour angle in degrees (positive in the afternoon).
fn azimuth(lat_r: f64, zenith: f64, declination: f64, hour_angle: f64) -> f64 {
    let denominator = lat_r.cos() * zenith.sin();
    // At the poles or with the sun straight overhead, every direction is
    // the same; pick south.
    if denominator.abs() < 1e-9 {
        return 180.0;
    }

    let cos_azimuth = (lat_r.sin() * zenith.cos() - declination.sin()) / denominator;
    let angle = cos_azimuth.clamp(-1.0, 1.0).acos().to_degrees();
    let afternoon = hour_angle > 0.0;
    if afternoon {
        (angle + 180.0).rem_euclid(360.0)
    } else {
        (540.0 - angle).rem_euclid(360.0)
    }
}

/// Sunrise and sunset on `date` (a calendar day in UTC terms; for local
/// days near Europe this is the same day).
pub(crate) fn times(lat: f64, lon: f64, date: NaiveDate) -> Option<SunTimes> {
    let midnight = Utc.from_utc_datetime(&date.and_hms_opt(0, 0, 0)?);
    let approximate_noon = midnight + Duration::minutes((720.0 - 4.0 * lon) as i64);
    let params = solar_parameters(approximate_noon);

    let lat_r = lat.to_radians();
    let cos_hour_angle = HORIZON_ZENITH.to_radians().cos()
        / (lat_r.cos() * params.declination.cos())
        - lat_r.tan() * params.declination.tan();
    if !(-1.0..=1.0).contains(&cos_hour_angle) {
        return None;
    }
    let hour_angle = cos_hour_angle.acos().to_degrees();
    let solar_noon = 720.0 - 4.0 * lon - params.equation_of_time;
    let at = |minutes: f64| midnight + Duration::seconds((minutes * 60.0).round() as i64);
    Some(SunTimes {
        sunrise: at(solar_noon - 4.0 * hour_angle),
        sunset: at(solar_noon + 4.0 * hour_angle),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAT: f64 = 52.99;
    const LON: f64 = 10.58;

    #[test]
    fn midsummer_in_lower_saxony() {
        let date = NaiveDate::from_ymd_opt(2026, 6, 21).unwrap();
        let times = times(LAT, LON, date).unwrap();
        let sunrise = times.sunrise.format("%H:%M").to_string();
        let sunset = times.sunset.format("%H:%M").to_string();
        assert!(
            ("02:45".."03:00").contains(&sunrise.as_str()),
            "sunrise {sunrise} UTC"
        );
        assert!(
            ("19:40".."19:55").contains(&sunset.as_str()),
            "sunset {sunset} UTC"
        );

        let noon = times.sunrise + (times.sunset - times.sunrise) / 2;
        let sun = position(LAT, LON, noon);
        assert!((sun.elevation - 60.45).abs() < 0.3, "{sun:?}");
        assert!((sun.azimuth - 180.0).abs() < 1.0, "{sun:?}");
    }

    #[test]
    fn positions_at_equinoxes_and_solstices() {
        // Reference values from almanac declination and equation of time with
        // the closed-form spherical triangle, independent of the NOAA series.
        let cases = [
            ("2026-03-20T05:10:00Z", -2.41, 87.06),
            ("2026-03-20T11:25:00Z", 36.95, 179.94),
            ("2026-03-20T14:00:00Z", 28.01, 225.09),
            ("2026-06-21T03:00:00Z", 0.13, 48.87),
            ("2026-06-21T11:44:00Z", 60.08, 191.42),
            ("2026-06-21T15:00:00Z", 39.27, 256.64),
            ("2026-09-23T05:00:00Z", -1.58, 88.04),
            ("2026-09-23T11:10:00Z", 36.83, 179.98),
            ("2026-09-23T14:00:00Z", 26.15, 228.79),
            ("2026-12-21T07:30:00Z", -0.71, 130.12),
            ("2026-12-21T11:16:00Z", 13.57, 180.05),
            ("2026-12-21T14:00:00Z", 5.67, 217.27),
        ];
        for (time, elevation, azimuth) in cases {
            let sun = position(LAT, LON, time.parse().unwrap());
            assert!((sun.elevation - elevation).abs() < 0.5, "{time}: {sun:?}");
            assert!((sun.azimuth - azimuth).abs() < 0.5, "{time}: {sun:?}");
        }
    }

    #[test]
    fn elevation_is_near_zero_at_sunrise() {
        let date = NaiveDate::from_ymd_opt(2026, 12, 21).unwrap();
        let times = times(LAT, LON, date).unwrap();
        let sun = position(LAT, LON, times.sunrise);
        assert!((sun.elevation + 0.833).abs() < 0.2, "{sun:?}");
        assert!(sun.azimuth > 90.0 && sun.azimuth < 180.0, "{sun:?}");
    }

    #[test]
    fn polar_night_has_no_sunrise() {
        let date = NaiveDate::from_ymd_opt(2026, 12, 21).unwrap();
        assert_eq!(times(78.2, 15.6, date), None);
    }
}
