//! Own-arrow detection on real minimap crops (2026-10-16, Rohkstov, 1080p).
//! Labels were marked by eye; compass values come from the HUD of the same frames.
use mtc_vision::*;

const MAP: &str = "rohkstov";
const SIZE: f64 = 5398.56;

fn dir() -> String {
    format!(
        "{}/../../tests/fixtures/live-2026-10-16",
        env!("CARGO_MANIFEST_DIR")
    )
}

#[test]
fn finds_the_labelled_player_arrow_and_checks_it_against_the_compass() {
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(format!("{}/manifest.json", dir())).unwrap())
            .unwrap();
    let map = OwnMapContext {
        map_id: MAP.into(),
        width_meters: SIZE,
        height_meters: SIZE,
    };
    let (mut hits, mut false_hits, mut misses, mut expected_misses) = (0, 0, Vec::new(), 0);
    for fixture in manifest["fixtures"].as_array().unwrap() {
        let file = fixture["file"].as_str().unwrap();
        // The manifest also lists non-minimap captures (e.g. the M109 weapon panel).
        if !file.starts_with("minimap-own-") {
            continue;
        }
        let expected = &fixture["expected"];
        let image = image::open(format!("{}/{file}", dir())).unwrap().to_rgba8();
        let grid = detect_map_grid(&image);
        let candidates = grid
            .map(|g| own_arrow_candidates(&image, &g, 1.0))
            .unwrap_or_default();
        let compass = expected["compassDeg"].as_f64();
        let reading =
            decide_own_position(1, image.dimensions(), grid, candidates, compass, Some(&map));
        let status = expected["status"].as_str().unwrap();
        println!(
            "{file}: {:?} {:?}",
            reading.status,
            reading
                .marker
                .as_ref()
                .map(|m| (m.pixel, m.sector.clone(), m.heading_deg))
        );
        match (&reading.marker, status) {
            (None, "none") => expected_misses += 1,
            (None, _) => misses.push(file),
            (Some(marker), _) => {
                let label = &expected["arrowInCrop"];
                let d = (marker.pixel.x - label["x"].as_f64().unwrap())
                    .hypot(marker.pixel.y - label["y"].as_f64().unwrap());
                if d > 3.0 || status == "none" {
                    false_hits += 1;
                    continue;
                }
                hits += 1;
                let wanted = if status == "ready" {
                    OwnPositionStatus::Ready
                } else {
                    OwnPositionStatus::Unconfirmed
                };
                assert_eq!(reading.status, wanted, "{file}");
                let world = marker.world.unwrap();
                assert!((world.x - marker.fraction.x * SIZE).abs() < 1e-6);
                assert!((world.y - (1.0 - marker.fraction.y) * SIZE).abs() < 1e-6);
            }
        }
    }
    println!("hits={hits} false={false_hits} misses={misses:?} expected_misses={expected_misses}");
    assert_eq!(false_hits, 0, "a wrong position is never reported");
    assert!(misses.is_empty(), "{misses:?}");
    assert_eq!(hits, 12);
}

#[test]
fn frames_without_a_map_never_report_an_own_position() {
    for name in [
        "weapon-panel-ub32-a.png",
        "timer-5336.png",
        "loadout-tertiary.png",
    ] {
        let path = format!(
            "{}/../../tests/fixtures/live-2026-10-15/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        let image = image::open(path).unwrap().to_rgba8();
        let reading = read_own_position_with(&image, 1, None, |_| {
            panic!("no compass OCR without an arrow")
        });
        assert!(reading.marker.is_none(), "{name}");
    }
}
