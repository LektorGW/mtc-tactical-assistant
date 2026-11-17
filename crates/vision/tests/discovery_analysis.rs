use image::RgbaImage;
use mtc_vision::*;

fn proposal(id: &str, kind: OcrKind, text: &str) -> HudRegionProposal {
    HudRegionProposal {
        region: OcrRegionConfig {
            id: id.into(),
            kind,
            roi: Roi {
                x: 0.1,
                y: 0.1,
                width: 0.2,
                height: 0.05,
            },
            upscale: 2,
        },
        confidence: 0.9,
        text: text.into(),
        value: None,
        evidence: vec!["Syntetyczna jawna etykieta i dwa potwierdzenia położenia".into()],
    }
}

fn discovery(timestamp: u64) -> HudDiscovery {
    HudDiscovery {
        schema_version: 1,
        status: HudDiscoveryStatus::Ready,
        timestamp_ms: timestamp,
        reference_width: 320,
        reference_height: 200,
        regions: vec![
            proposal("auto_vehicleName", OcrKind::VehicleName, "BM-21 Grad"),
            proposal("auto_weaponName", OcrKind::WeaponName, "122 mm HE"),
            proposal("auto_timer", OcrKind::Timer, "27:42"),
        ],
        warnings: vec![],
        elapsed_ms: 20.0,
        source: "Syntetyczny pełnoklatkowy OCR, bez obrazu gry".into(),
    }
}

fn config(discovery: &HudDiscovery) -> AnalysisConfig {
    AnalysisConfig {
        schema_version: 1,
        reference_width: 320,
        reference_height: 200,
        aspect_tolerance: 0.03,
        max_age_ms: 2000,
        ocr_regions: discovery
            .regions
            .iter()
            .rev()
            .map(|p| p.region.clone())
            .collect(),
        marker_config: None,
    }
}

#[test]
fn reuses_clean_full_frame_text_by_region_id_without_second_crop_ocr() {
    let image = RgbaImage::new(320, 200);
    let mut d = discovery(1000);
    d.regions[2].value = Some(999.0); // Text must remain the authority for parsing.
    let a = analyze_frame_from_discovery(&image, 1000, 1100, &config(&d), &d).unwrap();
    assert_eq!(a.status, AnalysisStatus::Ok);
    assert_eq!(a.numeric_observations[0].value, 1662.0);
    assert!(a.numeric_observations[0].confidence < 0.85);
    assert_eq!(a.text_observations[0].text, "122 mm HE");
    assert_eq!(a.text_observations[1].text, "BM-21 Grad");
    assert!(a.text_observations.iter().all(|o| o.confidence < 0.85));
    assert!(a
        .ocr_regions
        .iter()
        .all(|r| r.status == RegionStatus::Observed));
    assert!(serde_json::to_string(&a)
        .unwrap()
        .contains("pewność heurystyczna"));
}

#[test]
fn blocks_unconfirmed_missing_and_moved_regions_without_losing_other_valid_text() {
    let image = RgbaImage::new(320, 200);
    for scenario in 0..4 {
        let mut d = discovery(1000);
        let c = config(&d);
        match scenario {
            0 => d.regions[0].confidence = 0.78,
            1 => {
                d.regions.remove(0);
            }
            2 => d.regions[0].region.roi.x += 0.01,
            _ => d.regions[0].evidence.clear(),
        }
        let a = analyze_frame_from_discovery(&image, 1000, 1100, &c, &d).unwrap();
        assert_eq!(a.status, AnalysisStatus::Partial);
        assert_eq!(a.numeric_observations.len(), 1);
        assert_eq!(a.text_observations.len(), 1);
        let blocked = a
            .ocr_regions
            .iter()
            .find(|r| r.kind == OcrKind::VehicleName)
            .unwrap();
        assert_eq!(blocked.status, RegionStatus::Blocked);
        assert!(blocked.text_observation.is_none());
        assert!(blocked.text.is_empty());
    }
}

#[test]
fn rejects_discovery_from_another_frame_or_invalid_ambiguous_metadata() {
    let image = RgbaImage::new(320, 200);
    for scenario in 0..8 {
        let mut d = discovery(1000);
        let c = config(&d);
        match scenario {
            0 => d.timestamp_ms = 999,
            1 => d.reference_width = 640,
            2 => d.schema_version = 2,
            3 => d.regions.push(d.regions[0].clone()),
            4 => {
                let mut duplicate_kind = d.regions[0].clone();
                duplicate_kind.region.id = "other_vehicle".into();
                d.regions.push(duplicate_kind);
            }
            5 => d.regions[0].confidence = f64::NAN,
            6 => d.regions[0].text = "x".repeat(4097),
            _ => d.regions[0].region.roi.x = 2.0,
        }
        assert!(analyze_frame_from_discovery(&image, 1000, 1100, &c, &d).is_err());
    }
}

#[test]
fn refuses_stale_future_changed_aspect_and_blocked_discoveries() {
    let image = RgbaImage::new(320, 200);
    let d = discovery(1000);
    for (now, status) in [(4000, AnalysisStatus::Stale), (900, AnalysisStatus::Future)] {
        let a = analyze_frame_from_discovery(&image, 1000, now, &config(&d), &d).unwrap();
        assert_eq!(a.status, status);
        assert!(a.numeric_observations.is_empty() && a.text_observations.is_empty());
        assert!(a
            .ocr_regions
            .iter()
            .all(|r| r.status == RegionStatus::Blocked));
    }
    let mut c = config(&d);
    c.reference_height = 100;
    let a = analyze_frame_from_discovery(&image, 1000, 1100, &c, &d).unwrap();
    assert_eq!(a.status, AnalysisStatus::Blocked);
    assert!(a.text_observations.is_empty());
    let mut d = d;
    d.status = HudDiscoveryStatus::Blocked;
    let a = analyze_frame_from_discovery(&image, 1000, 1100, &config(&d), &d).unwrap();
    assert!(a.numeric_observations.is_empty() && a.text_observations.is_empty());
    assert!(a
        .ocr_regions
        .iter()
        .all(|r| r.status == RegionStatus::Blocked));
}

#[test]
fn stabilizes_values_only_across_distinct_fresh_source_frames() {
    let image = RgbaImage::new(320, 200);
    let first_discovery = discovery(1000);
    let c = config(&first_discovery);
    let first = analyze_frame_from_discovery(&image, 1000, 1100, &c, &first_discovery).unwrap();
    let mut repeated =
        analyze_frame_from_discovery(&image, 1000, 1200, &c, &first_discovery).unwrap();
    stabilize_frame(&mut repeated, Some(&first));
    assert!(repeated.numeric_observations[0].confidence < 0.85);
    assert!(repeated
        .text_observations
        .iter()
        .all(|o| o.confidence < 0.85));

    let second_discovery = discovery(1500);
    let mut second =
        analyze_frame_from_discovery(&image, 1500, 1600, &c, &second_discovery).unwrap();
    stabilize_frame(&mut second, Some(&first));
    assert_eq!(second.numeric_observations[0].confidence, 0.9);
    assert!(second
        .text_observations
        .iter()
        .all(|o| o.confidence == 0.85));

    let mut changed_discovery = discovery(2000);
    changed_discovery.regions[0].text = "M1 Abrams".into();
    let mut changed =
        analyze_frame_from_discovery(&image, 2000, 2100, &c, &changed_discovery).unwrap();
    stabilize_frame(&mut changed, Some(&second));
    assert!(
        changed
            .text_observations
            .iter()
            .find(|o| o.kind == OcrKind::VehicleName)
            .unwrap()
            .confidence
            < 0.85
    );
}
