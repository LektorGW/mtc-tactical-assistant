use mtc_vision::*;

fn word(text: &str, x: f64, y: f64, width: f64, height: f64) -> OcrWordBox {
    OcrWordBox {
        text: text.into(),
        roi: Roi {
            x,
            y,
            width,
            height,
        },
    }
}
fn line(words: Vec<OcrWordBox>) -> OcrLineBox {
    let x = words.iter().map(|w| w.roi.x).fold(1.0, f64::min);
    let y = words.iter().map(|w| w.roi.y).fold(1.0, f64::min);
    let right = words
        .iter()
        .map(|w| w.roi.x + w.roi.width)
        .fold(0.0, f64::max);
    let bottom = words
        .iter()
        .map(|w| w.roi.y + w.roi.height)
        .fold(0.0, f64::max);
    OcrLineBox {
        text: words
            .iter()
            .map(|w| w.text.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        roi: Roi {
            x,
            y,
            width: right - x,
            height: bottom - y,
        },
        words,
    }
}
fn layout(lines: Vec<OcrLineBox>) -> OcrLayout {
    OcrLayout {
        schema_version: 1,
        source_width: 1920,
        source_height: 1080,
        recognized_width: 1920,
        recognized_height: 1080,
        text: lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        text_angle_deg: None,
        lines,
        source: "Syntetyczny układ OCR do regresji; nie obraz gry".into(),
        elapsed_ms: 10.0,
    }
}
fn timer() -> OcrLineBox {
    line(vec![word("27:42", 0.465, 0.03, 0.07, 0.035)])
}
fn heading_scale(value: &str, left: &str, right: &str, prominent: bool) -> OcrLineBox {
    line(vec![
        word(left, 0.30, 0.135, 0.03, 0.016),
        word(
            value,
            0.48,
            0.128,
            0.04,
            if prominent { 0.027 } else { 0.016 },
        ),
        word(right, 0.67, 0.135, 0.03, 0.016),
    ])
}
fn find(discovery: &HudDiscovery, kind: OcrKind) -> Option<&HudRegionProposal> {
    discovery
        .regions
        .iter()
        .find(|proposal| proposal.region.kind == kind)
}

#[test]
fn proposes_timer_and_highlighted_central_heading_with_scale_evidence() {
    let d = discovery_from_layout(
        &layout(vec![timer(), heading_scale("138", "090", "180", true)]),
        1000,
        1100,
    );
    assert_eq!(d.status, HudDiscoveryStatus::Ready);
    assert_eq!(find(&d, OcrKind::Timer).unwrap().value, Some(1662.0));
    let heading = find(&d, OcrKind::Heading).unwrap();
    assert_eq!(heading.value, Some(138.0));
    assert!(heading.evidence.iter().any(|text| text.contains("skali")));
    assert!(d
        .regions
        .iter()
        .all(|r| r.confidence < 0.85 && r.region.roi.validate().is_ok()));
    let config = AnalysisConfig {
        schema_version: 1,
        reference_width: d.reference_width,
        reference_height: d.reference_height,
        aspect_tolerance: 0.03,
        max_age_ms: 2000,
        ocr_regions: d.regions.iter().map(|p| p.region.clone()).collect(),
        marker_config: None,
    };
    assert!(config.validate().is_ok());
}

#[test]
fn refuses_equal_size_compass_ticks_and_an_unlabeled_single_number() {
    for extra in [
        heading_scale("135", "090", "180", false),
        line(vec![word("138", 0.48, 0.13, 0.04, 0.027)]),
    ] {
        let d = discovery_from_layout(&layout(vec![timer(), extra]), 1000, 1000);
        assert!(find(&d, OcrKind::Heading).is_none());
        assert_eq!(d.status, HudDiscoveryStatus::Partial);
    }
}

#[test]
fn handles_compass_wraparound_and_rejects_values_outside_visible_scale_arc() {
    let d = discovery_from_layout(
        &layout(vec![heading_scale("359", "350", "000", true)]),
        1000,
        1000,
    );
    assert_eq!(find(&d, OcrKind::Heading).unwrap().value, Some(359.0));
    let d = discovery_from_layout(
        &layout(vec![heading_scale("200", "090", "180", true)]),
        1000,
        1000,
    );
    assert!(find(&d, OcrKind::Heading).is_none());
}

#[test]
fn only_accepts_speed_and_altitude_with_explicit_labels() {
    let explicit = layout(vec![
        line(vec![
            word("Speed:", 0.75, 0.5, 0.06, 0.022),
            word("40", 0.82, 0.5, 0.025, 0.022),
            word("km/h", 0.85, 0.5, 0.045, 0.022),
        ]),
        line(vec![
            word("Altitude:", 0.72, 0.55, 0.085, 0.022),
            word("1200m", 0.82, 0.55, 0.06, 0.022),
        ]),
    ]);
    let d = discovery_from_layout(&explicit, 1000, 1000);
    assert_eq!(find(&d, OcrKind::Speed).unwrap().value, Some(40.0));
    assert_eq!(find(&d, OcrKind::Altitude).unwrap().value, Some(1200.0));
    let bare = layout(vec![
        line(vec![word("40", 0.82, 0.5, 0.025, 0.022)]),
        line(vec![word("1200m", 0.82, 0.55, 0.06, 0.022)]),
    ]);
    assert!(discovery_from_layout(&bare, 1000, 1000).regions.is_empty());
}

#[test]
fn supports_polish_labels_and_preserves_name_only_roi() {
    let l = layout(vec![
        line(vec![
            word("Pojazd:", 0.02, 0.8, 0.06, 0.025),
            word("BM-21", 0.1, 0.8, 0.07, 0.025),
            word("Grad", 0.18, 0.8, 0.05, 0.025),
        ]),
        line(vec![
            word("Amunicja:", 0.02, 0.84, 0.085, 0.025),
            word("APFSDS", 0.12, 0.84, 0.1, 0.025),
        ]),
        line(vec![
            word("Mapa:", 0.02, 0.88, 0.05, 0.025),
            word("Desert", 0.1, 0.88, 0.08, 0.025),
        ]),
        line(vec![
            word("Prędkość:", 0.74, 0.5, 0.08, 0.025),
            word("40,5", 0.84, 0.5, 0.045, 0.025),
        ]),
        line(vec![
            word("Wysokość:", 0.73, 0.55, 0.09, 0.025),
            word("-10", 0.84, 0.55, 0.035, 0.025),
        ]),
    ]);
    let d = discovery_from_layout(&l, 1000, 1000);
    assert_eq!(find(&d, OcrKind::VehicleName).unwrap().text, "BM-21 Grad");
    assert!(find(&d, OcrKind::VehicleName).unwrap().region.roi.x > 0.09);
    assert_eq!(find(&d, OcrKind::WeaponName).unwrap().text, "APFSDS");
    assert_eq!(find(&d, OcrKind::MapName).unwrap().text, "Desert");
    assert_eq!(find(&d, OcrKind::Speed).unwrap().value, Some(40.5));
    assert_eq!(find(&d, OcrKind::Altitude).unwrap().value, Some(-10.0));
    assert!(d.regions.len() <= 16);
}

#[test]
fn refuses_merged_name_and_label_boxes_and_ammunition_counters() {
    let l = layout(vec![
        timer(),
        line(vec![word("Vehicle:BM21", 0.05, 0.8, 0.2, 0.025)]),
        line(vec![
            word("Ammo:", 0.05, 0.84, 0.06, 0.025),
            word("12/40", 0.13, 0.84, 0.07, 0.025),
        ]),
    ]);
    let d = discovery_from_layout(&l, 1000, 1000);
    assert!(find(&d, OcrKind::VehicleName).is_none());
    assert!(find(&d, OcrKind::WeaponName).is_none());
    assert!(d.warnings.iter().any(|text| text.contains("BoundingRect")));
}

#[test]
fn never_infers_vehicle_from_an_unlabeled_model_name_or_selection_menu() {
    for words in [
        vec![
            word("BM-21", 0.1, 0.8, 0.07, 0.025),
            word("Grad", 0.18, 0.8, 0.05, 0.025),
        ],
        vec![
            word("Choose", 0.01, 0.8, 0.06, 0.025),
            word("Vehicle:", 0.08, 0.8, 0.08, 0.025),
            word("BM-21", 0.17, 0.8, 0.07, 0.025),
        ],
    ] {
        let d = discovery_from_layout(&layout(vec![line(words)]), 1000, 1000);
        assert!(find(&d, OcrKind::VehicleName).is_none());
    }
}

#[test]
fn rejects_chat_timestamps_bottom_clocks_and_invalid_timer_seconds() {
    for words in [
        vec![
            word("Player:", 0.35, 0.1, 0.08, 0.03),
            word("27:42", 0.45, 0.1, 0.07, 0.03),
        ],
        vec![word("27:42", 0.45, 0.9, 0.07, 0.03)],
        vec![word("27:72", 0.45, 0.1, 0.07, 0.03)],
    ] {
        assert!(find(
            &discovery_from_layout(&layout(vec![line(words)]), 1000, 1000),
            OcrKind::Timer
        )
        .is_none());
    }
}

#[test]
fn labels_and_timer_words_split_by_ocr_remain_geometrically_supported() {
    let d = discovery_from_layout(
        &layout(vec![
            line(vec![
                word("27:", 0.465, 0.03, 0.035, 0.03),
                word("42", 0.5, 0.03, 0.035, 0.03),
            ]),
            line(vec![
                word("Heading:", 0.02, 0.2, 0.07, 0.02),
                word("270", 0.1, 0.2, 0.045, 0.02),
            ]),
        ]),
        1000,
        1000,
    );
    assert_eq!(find(&d, OcrKind::Timer).unwrap().value, Some(1662.0));
    assert_eq!(find(&d, OcrKind::Heading).unwrap().value, Some(270.0));
}

#[test]
fn pairs_an_explicit_label_with_one_nearby_value_line() {
    let d = discovery_from_layout(
        &layout(vec![
            line(vec![word("Altitude:", 0.75, 0.55, 0.08, 0.02)]),
            line(vec![word("250m", 0.75, 0.578, 0.06, 0.02)]),
        ]),
        1000,
        1000,
    );
    assert_eq!(find(&d, OcrKind::Altitude).unwrap().value, Some(250.0));
}

#[test]
fn blocks_multiple_timers_and_multiple_values_for_one_label() {
    let d = discovery_from_layout(
        &layout(vec![
            timer(),
            line(vec![word("29:33", 0.465, 0.2, 0.07, 0.035)]),
        ]),
        1000,
        1000,
    );
    assert_eq!(d.status, HudDiscoveryStatus::Ambiguous);
    assert!(find(&d, OcrKind::Timer).is_none());
    let d = discovery_from_layout(
        &layout(vec![line(vec![
            word("Speed:", 0.75, 0.5, 0.06, 0.022),
            word("40", 0.82, 0.5, 0.025, 0.022),
            word("50", 0.85, 0.5, 0.025, 0.022),
        ])]),
        1000,
        1000,
    );
    assert_eq!(d.status, HudDiscoveryStatus::Ambiguous);
    assert!(find(&d, OcrKind::Speed).is_none());
}

#[test]
fn refuses_equally_near_value_lines_and_photographed_keyboard_alt_keys() {
    let d = discovery_from_layout(
        &layout(vec![
            line(vec![word("Altitude:", 0.75, 0.55, 0.08, 0.02)]),
            line(vec![word("250m", 0.75, 0.578, 0.06, 0.02)]),
            line(vec![word("450m", 0.752, 0.578, 0.06, 0.02)]),
        ]),
        1000,
        1000,
    );
    assert_eq!(d.status, HudDiscoveryStatus::Ambiguous);
    assert!(find(&d, OcrKind::Altitude).is_none());
    let d = discovery_from_layout(
        &layout(vec![line(vec![
            word("Alt", 0.2, 0.91, 0.03, 0.02),
            word("5", 0.24, 0.91, 0.02, 0.02),
        ])]),
        1000,
        1000,
    );
    assert!(find(&d, OcrKind::Altitude).is_none());
}

#[test]
fn does_not_reinterpret_labeled_altitude_as_heading_amid_other_numbers() {
    let d = discovery_from_layout(
        &layout(vec![line(vec![
            word("090", 0.3, 0.135, 0.03, 0.016),
            word("Altitude:", 0.39, 0.128, 0.08, 0.027),
            word("138", 0.48, 0.128, 0.04, 0.027),
            word("180", 0.67, 0.135, 0.03, 0.016),
        ])]),
        1000,
        1000,
    );
    assert!(find(&d, OcrKind::Heading).is_none());
}

#[test]
fn rejects_stale_future_rotated_and_inconsistent_geometry() {
    for (timestamp, now) in [(1000, 4000), (2000, 1000)] {
        let d = discovery_from_layout(&layout(vec![timer()]), timestamp, now);
        assert_eq!(d.status, HudDiscoveryStatus::Blocked);
        assert!(d.regions.is_empty());
    }
    let mut l = layout(vec![timer()]);
    l.text_angle_deg = Some(20.0);
    assert!(discovery_from_layout(&l, 1000, 1000).regions.is_empty());
    l = layout(vec![timer()]);
    l.lines[0].words[0].roi.x = 0.99;
    assert!(discovery_from_layout(&l, 1000, 1000).regions.is_empty());
    l = layout(vec![timer()]);
    l.recognized_width = 960;
    assert!(discovery_from_layout(&l, 1000, 1000).regions.is_empty());
}

#[test]
fn maps_resize_and_clockwise_ocr_rotation_back_to_source_normalized_boxes() {
    let normal = normalize_ocr_box(100.0, 50.0, 20.0, 10.0, 200, 100, 0.0).unwrap();
    let resized = normalize_ocr_box(50.0, 25.0, 10.0, 5.0, 100, 50, 0.0).unwrap();
    assert_eq!(normal, resized);
    let rotated = normalize_ocr_box(60.0, 40.0, 20.0, 10.0, 100, 100, 90.0).unwrap();
    assert!((rotated.x - 0.5).abs() < 1e-9);
    assert!((rotated.y - 0.6).abs() < 1e-9);
    assert!((rotated.width - 0.1).abs() < 1e-9);
    assert!((rotated.height - 0.2).abs() < 1e-9);
    assert!(normalize_ocr_box(500.0, 500.0, 20.0, 10.0, 100, 100, 0.0).is_err());
}

#[test]
fn public_discovery_serializes_the_documented_region_contract() {
    let d = discovery_from_layout(&layout(vec![timer()]), 1000, 1000);
    let json = serde_json::to_value(d).unwrap();
    assert_eq!(json["status"], "partial");
    assert_eq!(json["regions"][0]["region"]["kind"], "timer");
    assert_eq!(json["referenceWidth"], 1920);
}

#[cfg(windows)]
#[test]
fn windows_layout_finds_actual_timer_words_with_source_boxes_in_both_photographs() {
    for (bytes, expected) in [
        (
            include_bytes!("../../../tests/fixtures/bm21_grad_rozgrywka.jpg").as_slice(),
            "27:42",
        ),
        (
            include_bytes!("../../../tests/fixtures/smiglowiec_rozgrywka.jpg").as_slice(),
            "29:33",
        ),
    ] {
        let image = image::load_from_memory(bytes).unwrap().to_rgba8();
        let l = ocr_layout(&image).unwrap();
        assert_eq!(
            (l.source_width, l.source_height),
            (image.width(), image.height())
        );
        assert!(l.validate().is_ok());
        println!(
            "Actual photograph angle {:?}, OCR {}",
            l.text_angle_deg, l.text
        );
        let timer = l
            .lines
            .iter()
            .find(|line| {
                line.words
                    .iter()
                    .map(|word| word.text.as_str())
                    .collect::<String>()
                    == expected
            })
            .expect("Windows OCR should locate the real visible match timer");
        assert!(timer.roi.x < 0.6 && timer.roi.x + timer.roi.width > 0.45 && timer.roi.y < 0.35);
        let crop = timer.roi.crop(&image).unwrap();
        assert!(crop.width() > 10 && crop.height() > 8);
        let discovery = discovery_from_layout(&l, 1000, 1000);
        assert!(find(&discovery, OcrKind::VehicleName).is_none());
        assert!(find(&discovery, OcrKind::WeaponName).is_none());
        assert!(find(&discovery, OcrKind::MapName).is_none());
        assert!(find(&discovery, OcrKind::Heading).is_none());
        let proposal = find(&discovery, OcrKind::Timer).unwrap();
        let selected = proposal.region.roi.crop(&image).unwrap();
        let enlarged = image::imageops::resize(
            &selected,
            selected.width() * 2,
            selected.height() * 2,
            image::imageops::FilterType::CatmullRom,
        );
        let read = ocr(&enlarged).unwrap();
        assert_eq!(
            parse_numeric("timer", &read, 1000).unwrap().value,
            if expected == "27:42" { 1662.0 } else { 1773.0 }
        );
    }
}
