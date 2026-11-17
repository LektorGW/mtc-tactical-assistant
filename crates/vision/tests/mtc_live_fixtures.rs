#![cfg(windows)]
//! Real MTC captures (2026-10-15, 1920x1080, DPI 125%) from tests/fixtures/live-2026-10-15.
use mtc_maps::Point;
use mtc_vision::*;

fn fixture(name: &str) -> image::RgbaImage {
    let path = format!(
        "{}/../../tests/fixtures/live-2026-10-15/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    image::open(path).unwrap().to_rgba8()
}

fn catalog() -> Vec<MapCatalogEntry> {
    let maps: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("../../../data/maps/community-maps.json")).unwrap();
    maps.iter()
        .map(|m| MapCatalogEntry {
            id: m["id"].as_str().unwrap().into(),
            name: m["name"].as_str().unwrap().into(),
            size_game_meters: m["sizeGameMeters"].as_f64().unwrap(),
            height_game_meters: None,
        })
        .collect()
}

#[test]
fn reads_zone_11_full_map_grid_label_and_sectors() {
    let image = fixture("full-map-zone11-frontlines.png");
    let reading = read_mtc_full_map(&image, 1000, &catalog());
    println!("{}", serde_json::to_string_pretty(&reading).unwrap());
    let grid = reading.grid.expect("grid");
    // Line centres measured on the fixture (crop origin 640,160 in the 1920x1080 client).
    for (got, expected) in [
        (grid.left, 32.0),
        (grid.top, 92.0),
        (grid.right, 609.0),
        (grid.bottom, 668.0),
    ] {
        assert!(
            (got - expected).abs() <= 3.0,
            "{got} vs {expected}: {grid:?}"
        );
    }
    assert_eq!(reading.grid_size_meters, Some(324.0));
    assert_eq!(reading.status, MtcMapStatus::Ready);
    assert_eq!(reading.map_id.as_deref(), Some("zone_11"));
    // Orange objective marker "A" sits in column A, row 5; "E" in column G, row 5.
    let a = map_grid_point(&grid, Some((2918.52, 2918.52)), Point { x: 86.0, y: 380.0 }).unwrap();
    assert_eq!(a.sector, "A5");
    let e = map_grid_point(
        &grid,
        Some((2918.52, 2918.52)),
        Point { x: 460.0, y: 380.0 },
    )
    .unwrap();
    assert_eq!(e.sector, "G5");
    let world = e.world.unwrap();
    assert!((world.x - 2918.52 * (460.0 - grid.left) / grid.width()).abs() < 1e-6);
}

/// Places a fixture crop back at its client position in a neutral 1920x1080 frame.
fn embed(name: &str, x: u32, y: u32) -> image::RgbaImage {
    let crop = fixture(name);
    let mut frame = image::RgbaImage::from_pixel(1920, 1080, image::Rgba([96, 104, 92, 255]));
    image::imageops::overlay(&mut frame, &crop, i64::from(x), i64::from(y));
    frame
}

fn known_names() -> Vec<String> {
    ["UB-32", "S-5K1", "BM-21", "9M22U HE-Frag"]
        .iter()
        .chain(OBSERVED_PANEL_NAMES.iter())
        .map(|s| s.to_string())
        .collect()
}

/// (file, weapon, ammo, elevation, azimuth)
type PanelCase = (
    &'static str,
    &'static str,
    &'static str,
    Option<f64>,
    Option<f64>,
);

#[test]
fn reads_weapon_panels_from_live_fixtures() {
    // (file, weapon, ammo, elevation, azimuth); templates never saw these frames.
    let cases: [PanelCase; 8] = [
        (
            "weapon-panel-rocketetet-a.png",
            "Rocketetet",
            "HE Rockets",
            Some(23.53),
            Some(268.53),
        ),
        (
            "weapon-panel-rocketetet-b.png",
            "Rocketetet",
            "HE Rockets",
            Some(23.29),
            Some(280.51),
        ),
        (
            "weapon-panel-rocketetet-c.png",
            "Rocketetet",
            "HE Rockets",
            Some(16.60),
            Some(274.72),
        ),
        (
            "weapon-panel-rocketetet-unloaded.png",
            "Rocketetet",
            "Unloaded",
            Some(23.29),
            Some(280.51),
        ),
        ("weapon-panel-spg9.png", "73 mm 2A28", "PG-9", None, None),
        (
            "weapon-panel-ub32-a.png",
            "UB-32",
            "S-5K1",
            Some(-0.89),
            Some(232.16),
        ),
        (
            "weapon-panel-ub32-b.png",
            "UB-32",
            "S-5K1",
            Some(13.71),
            Some(279.01),
        ),
        (
            "weapon-panel-ub32-c.png",
            "UB-32",
            "S-5K1",
            Some(32.61),
            Some(242.36),
        ),
    ];
    let mut failures = Vec::new();
    for (file, weapon, ammo, elevation, azimuth) in cases {
        let reading = read_weapon_panel(&embed(file, 1700, 340), 1000, &known_names());
        let got = (
            reading.weapon.as_ref().map(|t| t.text.clone()),
            reading.ammo.as_ref().map(|t| t.text.clone()),
            reading.elevation_deg.as_ref().map(|v| v.value),
            reading.azimuth_deg.as_ref().map(|v| v.value),
            reading.range_meters,
        );
        println!(
            "{file}: {got:?} range={:?} status={:?} weaponOcr={:?} ammoOcr={:?}",
            reading.range_text,
            reading.status,
            reading.weapon.as_ref().map(|t| &t.attempts),
            reading.ammo.as_ref().map(|t| &t.attempts)
        );
        let close = |a: Option<f64>, b: Option<f64>| match (a, b) {
            (Some(a), Some(b)) => (a - b).abs() < 0.001,
            (None, None) => true,
            _ => false,
        };
        if got.0.as_deref() != Some(weapon)
            || got.1.as_deref() != Some(ammo)
            || !close(got.2, elevation)
            || !close(got.3, azimuth)
            || got.4.is_some()
        {
            failures.push((file, got));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn reads_the_m109_panel_captured_on_2026_10_16() {
    let path = format!(
        "{}/../../tests/fixtures/live-2026-10-16/weapon-panel-m109.png",
        env!("CARGO_MANIFEST_DIR")
    );
    let crop = image::open(path).unwrap().to_rgba8();
    let mut frame = image::RgbaImage::from_pixel(1920, 1080, image::Rgba([96, 104, 92, 255]));
    image::imageops::overlay(&mut frame, &crop, 1700, 340);
    let mut names = known_names();
    names.extend(["155 mm M185", "M107 Medium Charge"].map(String::from));
    let reading = read_weapon_panel(&frame, 1000, &names);
    assert_eq!(
        reading.weapon.as_ref().map(|t| t.text.as_str()),
        Some("155 mm M185")
    );
    assert_eq!(
        reading.ammo.as_ref().map(|t| t.text.as_str()),
        Some("M107 Medium Charge")
    );
    assert!((reading.elevation_deg.unwrap().value - 3.12).abs() < 0.001);
    assert!((reading.azimuth_deg.unwrap().value - 180.44).abs() < 0.001);
    assert!(reading.range_meters.is_none());
}

#[test]
fn weapon_panel_is_absent_from_the_full_map_fixture() {
    let reading = read_weapon_panel(
        &embed("full-map-zone11-frontlines.png", 640, 160),
        1000,
        &known_names(),
    );
    assert!(reading.elevation_deg.is_none() && reading.azimuth_deg.is_none());
}

fn read_numeric(image: &image::RgbaImage, kind: OcrKind, roi: Roi) -> Option<f64> {
    let config = AnalysisConfig {
        schema_version: 1,
        reference_width: image.width(),
        reference_height: image.height(),
        aspect_tolerance: 0.03,
        max_age_ms: 2000,
        ocr_regions: vec![OcrRegionConfig {
            id: "r".into(),
            kind,
            roi,
            upscale: 2,
        }],
        marker_config: None,
    };
    let result = analyze_frame(image, 1000, 1000, &config).unwrap();
    println!(
        "{kind:?}: {:?} {:?}",
        result.ocr_regions[0].status, result.ocr_regions[0].text
    );
    result.numeric_observations.first().map(|o| o.value)
}

#[test]
fn reads_live_timer_and_central_compass_heading() {
    let timer = fixture("timer-5336.png");
    let full = Roi {
        x: 0.0,
        y: 0.0,
        width: 1.0,
        height: 1.0,
    };
    assert_eq!(
        read_numeric(&timer, OcrKind::Timer, full),
        Some(53.0 * 60.0 + 36.0)
    );
    // compass-269.png spans client x 520..1400, y 70..140; "269" is centred near x 960, y 105.
    let compass = fixture("compass-269.png");
    let centre = Roi {
        x: 410.0 / 880.0,
        y: 24.0 / 70.0,
        width: 60.0 / 880.0,
        height: 28.0 / 70.0,
    };
    assert_eq!(
        read_numeric(&compass, OcrKind::Heading, centre),
        Some(269.0)
    );
}

#[test]
fn reads_loadout_specifications_from_live_fixtures() {
    let primary = read_loadout_specs(&fixture("loadout-specs-primary.png"), 1000).unwrap();
    println!("{primary:#?}");
    assert_eq!(primary.status, LoadoutSpecsStatus::Ready);
    assert_eq!(primary.muzzle_velocity_mps.as_ref().unwrap().value, 825.0);
    assert_eq!(primary.damage.as_ref().unwrap().value, 66.0);
    assert_eq!(primary.projectile_type.as_deref(), Some("AP"));
    assert_eq!(primary.penetration_mm.as_ref().unwrap().value, 18.0);
    assert_eq!(primary.projectile_mass_kg.as_ref().unwrap().value, 0.0);
    assert!(primary.weapon_name.is_none());
    let tertiary = read_loadout_specs(&fixture("loadout-tertiary.png"), 1000).unwrap();
    println!("{tertiary:#?}");
    assert_eq!(tertiary.status, LoadoutSpecsStatus::Ready);
    assert_eq!(tertiary.slot.as_deref(), Some("Tertiary"));
    assert_eq!(tertiary.muzzle_velocity_mps.as_ref().unwrap().value, 350.0);
    assert_eq!(tertiary.damage.as_ref().unwrap().value, 29.0);
    assert_eq!(tertiary.penetration_mm.as_ref().unwrap().value, 1.0);
    // A weapon-panel frame is not a loadout screen.
    let panel = read_loadout_specs(&fixture("weapon-panel-ub32-a.png"), 1000).unwrap();
    assert_eq!(panel.status, LoadoutSpecsStatus::Blocked);
    assert!(panel.muzzle_velocity_mps.is_none());
}
