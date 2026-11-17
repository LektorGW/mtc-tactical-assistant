use serde::{Deserialize, Serialize};

mod catalog;
mod registration;
mod terrain;
pub use catalog::*;
pub use registration::*;
pub use terrain::*;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub fn valid(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

/// Image y points down; world y points north. Rotation is clockwise from north.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MapCalibration {
    pub units_per_pixel: f64,
    pub north_rotation_deg: f64,
    pub origin_pixel: Point,
    pub origin_world: Point,
    pub uncertainty_units: f64,
}

impl MapCalibration {
    pub fn validate(&self) -> Result<(), String> {
        if !self.units_per_pixel.is_finite() || self.units_per_pixel <= 0.0 {
            return Err("Brak poprawnej skali mapy".into());
        }
        if !self.north_rotation_deg.is_finite()
            || !self.origin_pixel.valid()
            || !self.origin_world.valid()
            || !self.uncertainty_units.is_finite()
            || self.uncertainty_units < 0.0
        {
            return Err("Niepoprawna kalibracja mapy".into());
        }
        Ok(())
    }

    pub fn transform(&self, p: Point) -> Result<Point, String> {
        self.validate()?;
        if !p.valid() {
            return Err("Niepoprawny punkt mapy".into());
        }
        let a = self.north_rotation_deg.to_radians();
        let east = (p.x - self.origin_pixel.x) * self.units_per_pixel;
        let north = -(p.y - self.origin_pixel.y) * self.units_per_pixel;
        let result = Point {
            x: self.origin_world.x + east * a.cos() + north * a.sin(),
            y: self.origin_world.y - east * a.sin() + north * a.cos(),
        };
        if !result.valid() {
            return Err("Kalibracja prowadzi do przepełnienia współrzędnych".into());
        }
        Ok(result)
    }

    /// Same pixel→world mapping. Uncertainty is presentation metadata and does
    /// not move positions, so it must not invalidate a terrain or map binding.
    pub fn same_geometry(&self, other: &MapCalibration) -> bool {
        let close = |a: f64, b: f64| (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0);
        close(self.units_per_pixel, other.units_per_pixel)
            && close(self.north_rotation_deg, other.north_rotation_deg)
            && close(self.origin_pixel.x, other.origin_pixel.x)
            && close(self.origin_pixel.y, other.origin_pixel.y)
            && close(self.origin_world.x, other.origin_world.x)
            && close(self.origin_world.y, other.origin_world.y)
    }
}

/// Scale of a complete, square community map texture.
///
/// The source calculator stores positions as one normalized XY pair for both
/// axes and multiplies the delta by the full map size. A raster with a
/// different aspect ratio would need two scales, so it is rejected instead of
/// silently distorting distances.
pub fn full_square_map_scale(size_units: f64, width: u32, height: u32) -> Result<f64, String> {
    if !size_units.is_finite() || size_units <= 0.0 {
        return Err("Brak dodatniego wymiaru pełnej mapy".into());
    }
    if width < 2 || height < 2 {
        return Err("Obraz mapy jest zbyt mały".into());
    }
    if (width as f64 / height as f64 - 1.0).abs() > 0.01 {
        return Err(format!(
            "Obraz {width}×{height} nie jest kwadratowy — wymiar katalogu dotyczy pełnej kwadratowej mapy"
        ));
    }
    Ok(size_units / width as f64)
}

pub fn calibrate_two_points(a: Point, b: Point, known_distance: f64) -> Result<f64, String> {
    let pixels = (b.x - a.x).hypot(b.y - a.y);
    if !a.valid()
        || !b.valid()
        || !known_distance.is_finite()
        || known_distance <= 0.0
        || pixels < 1.0
    {
        return Err("Wskaż dwa różne punkty i dodatni dystans referencyjny".into());
    }
    Ok(known_distance / pixels)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bearing {
    pub distance: f64,
    pub azimuth_deg: f64,
    pub uncertainty_units: f64,
}

pub fn bearing(calibration: &MapCalibration, own: Point, target: Point) -> Result<Bearing, String> {
    let a = calibration.transform(own)?;
    let b = calibration.transform(target)?;
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let distance = dx.hypot(dy);
    if !distance.is_finite() {
        return Err("Dystans poza zakresem liczb".into());
    }
    if distance < 1e-9 {
        return Err("Pozycja i cel są identyczne — azymut nieokreślony".into());
    }
    Ok(Bearing {
        distance,
        azimuth_deg: dx.atan2(dy).to_degrees().rem_euclid(360.0),
        uncertainty_units: calibration.uncertainty_units * 2.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn calibration() -> MapCalibration {
        MapCalibration {
            units_per_pixel: 2.0,
            north_rotation_deg: 0.0,
            origin_pixel: Point { x: 0.0, y: 0.0 },
            origin_world: Point { x: 0.0, y: 0.0 },
            uncertainty_units: 1.0,
        }
    }
    #[test]
    fn rejects_missing_map_scale() {
        let mut c = calibration();
        c.units_per_pixel = 0.0;
        assert!(c.validate().is_err());
    }
    #[test]
    fn computes_cardinal_azimuths_and_distance() {
        let c = calibration();
        let own = Point { x: 10.0, y: 10.0 };
        for (x, y, az) in [
            (10.0, 0.0, 0.0),
            (20.0, 10.0, 90.0),
            (10.0, 20.0, 180.0),
            (0.0, 10.0, 270.0),
        ] {
            let b = bearing(&c, own, Point { x, y }).unwrap();
            assert!((b.distance - 20.0).abs() < 1e-8);
            assert!((b.azimuth_deg - az).abs() < 1e-8);
        }
    }
    #[test]
    fn rotates_axes_and_translates_origin() {
        let mut c = calibration();
        c.north_rotation_deg = 90.0;
        c.origin_world = Point { x: 100.0, y: 200.0 };
        let p = c.transform(Point { x: 0.0, y: -10.0 }).unwrap();
        assert!((p.x - 120.0).abs() < 1e-8);
        assert!((p.y - 200.0).abs() < 1e-8);
    }
    #[test]
    fn calibrates_two_points_without_assuming_world_units() {
        assert_eq!(
            calibrate_two_points(Point { x: 0.0, y: 0.0 }, Point { x: 3.0, y: 4.0 }, 100.0)
                .unwrap(),
            20.0
        );
        assert!(
            calibrate_two_points(Point { x: 0.0, y: 0.0 }, Point { x: 0.0, y: 0.0 }, 100.0)
                .is_err()
        );
    }
    #[test]
    fn rejects_nan_and_identical_points() {
        assert!(calibration()
            .transform(Point {
                x: f64::NAN,
                y: 0.0
            })
            .is_err());
        assert!(bearing(
            &calibration(),
            Point { x: 1.0, y: 1.0 },
            Point { x: 1.0, y: 1.0 }
        )
        .is_err());
    }
    #[test]
    fn full_square_map_scale_matches_source_normalized_distance() {
        // Normandy Bocage: 664 × 9 studs × 0.36 = 2151.36 game meters on a 1024 px texture.
        let scale = full_square_map_scale(2151.36, 1024, 1024).unwrap();
        assert!((scale - 2.1009375).abs() < 1e-12);
        // Source: distance = |Δ normalized| × size; here Δ = 512 px = 0.5 of the map.
        let b = bearing(
            &MapCalibration {
                units_per_pixel: scale,
                ..calibration()
            },
            Point { x: 0.0, y: 512.0 },
            Point { x: 512.0, y: 512.0 },
        )
        .unwrap();
        assert!((b.distance - 2151.36 / 2.0).abs() < 1e-9);
        assert!((b.azimuth_deg - 90.0).abs() < 1e-9);
    }
    #[test]
    fn rejects_non_square_or_invalid_full_map_texture() {
        assert!(full_square_map_scale(1000.0, 1024, 512).is_err());
        assert!(full_square_map_scale(0.0, 1024, 1024).is_err());
        assert!(full_square_map_scale(f64::NAN, 1024, 1024).is_err());
        assert!(full_square_map_scale(1000.0, 1, 1).is_err());
        assert!(full_square_map_scale(1000.0, 1024, 1020).is_ok());
    }
    #[test]
    fn same_geometry_ignores_uncertainty_but_not_scale_or_rotation() {
        let a = calibration();
        let mut b = a;
        b.uncertainty_units = 25.0;
        assert!(a.same_geometry(&b));
        b.units_per_pixel *= 1.0 + 1e-6;
        assert!(!a.same_geometry(&b));
        let mut c = a;
        c.north_rotation_deg = 0.5;
        assert!(!a.same_geometry(&c));
    }
    #[test]
    fn rejects_coordinate_overflow() {
        let mut c = calibration();
        c.units_per_pixel = f64::MAX;
        assert!(c.transform(Point { x: 2.0, y: 1.0 }).is_err());
    }
}
