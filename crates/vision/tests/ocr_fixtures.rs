#![cfg(windows)]
use mtc_vision::{
    analyze_frame, ocr, parse_numeric, AnalysisConfig, AnalysisStatus, OcrKind, OcrRegionConfig,
    Roi,
};

#[test]
fn reads_bm21_match_timer_from_provided_photograph() {
    check(
        include_bytes!("../../../tests/fixtures/bm21_grad_rozgrywka.jpg"),
        Roi {
            x: 0.465,
            y: 0.123,
            width: 0.095,
            height: 0.043,
        },
        1662.0,
    );
}
#[test]
fn reads_helicopter_match_timer_from_provided_photograph() {
    check(
        include_bytes!("../../../tests/fixtures/smiglowiec_rozgrywka.jpg"),
        Roi {
            x: 0.455,
            y: 0.188,
            width: 0.096,
            height: 0.047,
        },
        1773.0,
    );
}
fn check(bytes: &[u8], roi: Roi, expected: f64) {
    let img = image::load_from_memory(bytes).unwrap().to_rgba8();
    let crop = roi.crop(&img).unwrap();
    let doubled = image::imageops::resize(
        &crop,
        crop.width() * 2,
        crop.height() * 2,
        image::imageops::FilterType::CatmullRom,
    );
    let text = ocr(&doubled).expect("Systemowy OCR Windows musi być dostępny dla tego testu");
    println!("OCR: {text}");
    assert_eq!(
        parse_numeric("timer", &text, 0)
            .expect("Nie rozpoznano poprawnego czasu meczu")
            .value,
        expected
    );
    let config = AnalysisConfig {
        schema_version: 1,
        reference_width: img.width(),
        reference_height: img.height(),
        aspect_tolerance: 0.03,
        max_age_ms: 2000,
        ocr_regions: vec![OcrRegionConfig {
            id: "match-timer".into(),
            kind: OcrKind::Timer,
            roi,
            upscale: 2,
        }],
        marker_config: None,
    };
    let result = analyze_frame(&img, 1000, 1000, &config).unwrap();
    assert_eq!(result.status, AnalysisStatus::Ok);
    assert_eq!(result.numeric_observations[0].value, expected);
}

fn analyse_region(img: &image::RgbaImage, kind: OcrKind, roi: Roi, upscale: u32) -> Option<f64> {
    let config = AnalysisConfig {
        schema_version: 1,
        reference_width: img.width(),
        reference_height: img.height(),
        aspect_tolerance: 0.03,
        max_age_ms: 2000,
        ocr_regions: vec![OcrRegionConfig {
            id: "region".into(),
            kind,
            roi,
            upscale,
        }],
        marker_config: None,
    };
    let result = analyze_frame(img, 1000, 1000, &config).unwrap();
    println!(
        "{kind:?} x{upscale} {:?}: {:?} {:?}",
        roi, result.ocr_regions[0].status, result.ocr_regions[0].text
    );
    result.numeric_observations.first().map(|o| o.value)
}

#[test]
fn reads_central_compass_heading_from_bm21_photograph_with_light_text_fallback() {
    // White "138" over a bright blue sky, under the CONQUEST label. Several
    // hand-drawn ROI variants and every supported upscale must give 138.
    let img = image::load_from_memory(include_bytes!(
        "../../../tests/fixtures/bm21_grad_rozgrywka.jpg"
    ))
    .unwrap()
    .to_rgba8();
    let mut failures = Vec::new();
    for roi in [
        Roi {
            x: 0.500,
            y: 0.183,
            width: 0.050,
            height: 0.022,
        },
        Roi {
            x: 0.495,
            y: 0.180,
            width: 0.060,
            height: 0.028,
        },
        Roi {
            x: 0.505,
            y: 0.184,
            width: 0.040,
            height: 0.018,
        },
    ] {
        for upscale in 2..=4 {
            if analyse_region(&img, OcrKind::Heading, roi, upscale) != Some(138.0) {
                failures.push((roi, upscale));
            }
        }
    }
    assert!(failures.is_empty(), "Compass 138 not read for {failures:?}");
    // Plain sky and clouds next to the compass never produce a heading.
    for upscale in 2..=4 {
        let sky = Roi {
            x: 0.30,
            y: 0.33,
            width: 0.05,
            height: 0.022,
        };
        assert_eq!(analyse_region(&img, OcrKind::Heading, sky, upscale), None);
    }
}

#[test]
fn reads_both_match_timers_at_every_supported_upscale() {
    for (bytes, roi, expected) in [
        (
            include_bytes!("../../../tests/fixtures/bm21_grad_rozgrywka.jpg").as_slice(),
            Roi {
                x: 0.465,
                y: 0.123,
                width: 0.095,
                height: 0.043,
            },
            1662.0,
        ),
        (
            include_bytes!("../../../tests/fixtures/smiglowiec_rozgrywka.jpg").as_slice(),
            Roi {
                x: 0.455,
                y: 0.188,
                width: 0.096,
                height: 0.047,
            },
            1773.0,
        ),
    ] {
        let img = image::load_from_memory(bytes).unwrap().to_rgba8();
        for upscale in 1..=4 {
            assert_eq!(
                analyse_region(&img, OcrKind::Timer, roi, upscale),
                Some(expected),
                "timer x{upscale}"
            );
        }
    }
}
