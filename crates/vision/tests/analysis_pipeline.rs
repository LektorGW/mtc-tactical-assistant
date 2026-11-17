use image::{Rgba, RgbaImage};
use mtc_maps::{MapCalibration, Point};
use mtc_vision::*;

fn signature(id: &str, role: MarkerRole, rgb: [u8; 3]) -> MarkerSignature {
    MarkerSignature {
        id: id.into(),
        role,
        rgb,
        tolerance: 12,
        min_component_pixels: 20,
        max_component_pixels: 120,
        min_aspect_ratio: 0.5,
        max_aspect_ratio: 2.0,
        min_fill_ratio: 0.2,
    }
}

fn marker_config() -> MarkerConfig {
    let map_roi = Roi {
        x: 0.25,
        y: 0.2,
        width: 0.5,
        height: 0.6,
    };
    MarkerConfig {
        enabled: true,
        map_roi,
        map_width: 160,
        map_height: 120,
        coordinate_calibration_confirmed: true,
        view_signature: Some(calibrate_view_signature(&synthetic_map(), map_roi).unwrap()),
        aspect_tolerance: 0.03,
        signatures: vec![
            signature("own-user-selection", MarkerRole::Own, [0, 240, 0]),
            signature("target-user-selection", MarkerRole::Target, [240, 0, 0]),
        ],
    }
}

fn fill_rect(image: &mut RgbaImage, x: u32, y: u32, width: u32, height: u32, color: [u8; 4]) {
    for py in y..y + height {
        for px in x..x + width {
            image.put_pixel(px, py, Rgba(color));
        }
    }
}

fn synthetic_map() -> RgbaImage {
    let mut image = RgbaImage::from_pixel(160, 100, Rgba([30, 40, 50, 255]));
    texture(&mut image, 40, 20, 80, 60);
    fill_rect(&mut image, 54, 39, 5, 5, [0, 240, 0, 255]);
    fill_rect(&mut image, 96, 60, 5, 5, [240, 0, 0, 255]);
    // Same color outside the map, tiny noise, oversized legend and thin line.
    fill_rect(&mut image, 5, 5, 5, 5, [240, 0, 0, 255]);
    fill_rect(&mut image, 85, 48, 1, 1, [240, 0, 0, 255]);
    fill_rect(&mut image, 65, 60, 15, 15, [240, 0, 0, 255]);
    fill_rect(&mut image, 70, 30, 10, 1, [240, 0, 0, 255]);
    image
}

fn texture(image: &mut RgbaImage, x: u32, y: u32, width: u32, height: u32) {
    for py in 0..height {
        for px in 0..width {
            let column = px * 16 / width;
            let row = py * 16 / height;
            let gray = 40 + ((column * 37 + row * 53 + column * row * 11) % 100) as u8;
            image.put_pixel(x + px, y + py, Rgba([gray, gray, gray, 255]));
        }
    }
}

#[test]
fn detects_only_visible_configured_components_and_maps_centroids_to_reference_pixels() {
    let result = detect_markers(&synthetic_map(), &marker_config(), 1000, 1100, 2000).unwrap();
    assert_eq!(result.markers.len(), 2);
    assert!(result.warning.is_none());
    let own = result
        .markers
        .iter()
        .find(|marker| marker.role == MarkerRole::Own)
        .unwrap();
    assert!((own.map_position.x - 33.0).abs() < 1e-9);
    assert!((own.map_position.y - 43.0).abs() < 1e-9);
    assert_eq!(own.pixel_count, 25);
    assert!((own.frame_position_normalized.x - 56.5 / 160.0).abs() < 1e-9);
    assert!(own.bounding_box.validate().is_ok());
}

#[test]
fn keeps_calibrated_positions_and_component_area_stable_when_frame_resolution_doubles() {
    let image = synthetic_map();
    let resized = image::imageops::resize(&image, 320, 200, image::imageops::FilterType::Nearest);
    let first = detect_markers(&image, &marker_config(), 1000, 1000, 2000).unwrap();
    let second = detect_markers(&resized, &marker_config(), 2000, 2000, 2000).unwrap();
    assert_eq!(second.markers.len(), first.markers.len());
    for (a, b) in first.markers.iter().zip(&second.markers) {
        assert!((a.map_position.x - b.map_position.x).abs() < 1e-9);
        assert!((a.map_position.y - b.map_position.y).abs() < 1e-9);
        assert_eq!(b.pixel_count, a.pixel_count * 4);
    }
}

#[test]
fn preserves_multiple_candidates_as_ambiguity_instead_of_selecting_one() {
    let mut image = synthetic_map();
    fill_rect(&mut image, 102, 35, 5, 5, [240, 0, 0, 255]);
    let result = detect_markers(&image, &marker_config(), 1000, 1000, 2000).unwrap();
    assert_eq!(
        result
            .markers
            .iter()
            .filter(|marker| marker.role == MarkerRole::Target)
            .count(),
        2
    );
    assert!(result.warning.unwrap().contains("Wiele"));
}

#[test]
fn requires_explicit_map_confirmation_opt_in_and_matching_reference_geometry() {
    let image = synthetic_map();
    let mut config = marker_config();
    config.coordinate_calibration_confirmed = false;
    assert!(detect_markers(&image, &config, 1000, 1000, 2000)
        .unwrap()
        .markers
        .is_empty());
    config.coordinate_calibration_confirmed = true;
    config.enabled = false;
    assert!(detect_markers(&image, &config, 1000, 1000, 2000)
        .unwrap()
        .markers
        .is_empty());
    config.enabled = true;
    config.map_width = 320;
    assert!(detect_markers(&image, &config, 1000, 1000, 2000)
        .unwrap()
        .markers
        .is_empty());
    config.map_roi.x = 0.9;
    assert!(detect_markers(&image, &config, 1000, 1000, 2000).is_err());
}

#[test]
fn rejects_overlapping_signatures_instead_of_assigning_the_same_icon_to_own_and_target() {
    let mut config = marker_config();
    config.signatures[1].rgb = config.signatures[0].rgb;
    assert!(config.validate().is_err());
    config.signatures[1].min_component_pixels = 121;
    config.signatures[1].max_component_pixels = 200;
    assert!(config.validate().is_ok());
}

#[test]
fn blocks_excessive_candidate_noise_without_returning_an_arbitrary_subset() {
    let mut image = RgbaImage::from_pixel(128, 128, Rgba([0, 0, 0, 255]));
    texture(&mut image, 0, 0, 128, 128);
    for x in (1..125).step_by(4) {
        for y in (1..125).step_by(4) {
            fill_rect(&mut image, x, y, 2, 2, [240, 0, 0, 255]);
        }
    }
    let mut target = signature("target", MarkerRole::Target, [240, 0, 0]);
    target.min_component_pixels = 4;
    let map_roi = Roi {
        x: 0.0,
        y: 0.0,
        width: 1.0,
        height: 1.0,
    };
    let config = MarkerConfig {
        enabled: true,
        map_roi,
        map_width: 128,
        map_height: 128,
        coordinate_calibration_confirmed: true,
        view_signature: Some(calibrate_view_signature(&image, map_roi).unwrap()),
        aspect_tolerance: 0.03,
        signatures: vec![target],
    };
    let result = detect_markers(&image, &config, 1000, 1000, 2000).unwrap();
    assert!(result.markers.is_empty());
    assert!(result.warning.unwrap().contains("Zbyt wiele"));
}

#[test]
fn refuses_stale_future_transparent_and_clipped_map_markers() {
    let mut image = synthetic_map();
    assert!(detect_markers(&image, &marker_config(), 1000, 4000, 2000)
        .unwrap()
        .markers
        .is_empty());
    assert!(detect_markers(&image, &marker_config(), 5000, 4000, 2000)
        .unwrap()
        .markers
        .is_empty());
    fill_rect(&mut image, 102, 35, 5, 5, [240, 0, 0, 0]);
    fill_rect(&mut image, 40, 30, 5, 5, [0, 240, 0, 255]);
    let result = detect_markers(&image, &marker_config(), 1000, 1000, 2000).unwrap();
    assert_eq!(
        result
            .markers
            .iter()
            .filter(|marker| marker.role == MarkerRole::Target)
            .count(),
        1
    );
    let clipped = result
        .markers
        .iter()
        .find(|marker| marker.map_position.x < 10.0)
        .unwrap();
    assert!(clipped.confidence < TrackerConfig::default().min_confidence);
}

fn batch_config() -> AnalysisConfig {
    AnalysisConfig {
        schema_version: 1,
        reference_width: 160,
        reference_height: 100,
        aspect_tolerance: 0.03,
        max_age_ms: 2000,
        ocr_regions: vec![
            OcrRegionConfig {
                id: "compass".into(),
                kind: OcrKind::Heading,
                roi: Roi {
                    x: 0.0,
                    y: 0.0,
                    width: 0.2,
                    height: 0.1,
                },
                upscale: 2,
            },
            OcrRegionConfig {
                id: "vehicle-label".into(),
                kind: OcrKind::VehicleName,
                roi: Roi {
                    x: 0.8,
                    y: 0.0,
                    width: 0.2,
                    height: 0.1,
                },
                upscale: 2,
            },
        ],
        marker_config: Some(marker_config()),
    }
}

#[test]
fn reads_multiple_rois_from_one_frame_and_preserves_explicit_vehicle_text() {
    let mut calls = 0;
    let result = analyze_frame_with_ocr(&synthetic_map(), 1000, 1000, &batch_config(), |crop| {
        assert_eq!((crop.width(), crop.height()), (64, 20));
        calls += 1;
        Ok(if calls == 1 {
            "138".into()
        } else {
            "  Vehicle:\n BM-21 Grad  ".into()
        })
    })
    .unwrap();
    assert_eq!(calls, 2);
    assert_eq!(result.status, AnalysisStatus::Ok);
    assert_eq!(result.numeric_observations[0].value, 138.0);
    assert_eq!(result.text_observations[0].text, "Vehicle: BM-21 Grad");
    assert_eq!(result.text_observations[0].kind, OcrKind::VehicleName);
    assert_eq!(result.markers.len(), 2);
}

#[test]
fn keeps_unrelated_text_and_markers_when_compass_ocr_is_ambiguous_or_failed() {
    for first in [
        Ok("090 138 180".to_string()),
        Err("OCR niedostępny".to_string()),
    ] {
        // Each region starts with the plain 64x20 enlargement; later inputs of the
        // same region are padded light-text fallbacks of the unaccepted compass read.
        let (mut region, mut compass_calls) = (0, 0);
        let result =
            analyze_frame_with_ocr(&synthetic_map(), 1000, 1000, &batch_config(), |crop| {
                if crop.dimensions() == (64, 20) {
                    region += 1;
                }
                if region == 1 {
                    compass_calls += 1;
                    first.clone()
                } else {
                    Ok("Unknown vehicle".to_string())
                }
            })
            .unwrap();
        assert_eq!(compass_calls, if first.is_ok() { 3 } else { 1 });
        assert_eq!(result.status, AnalysisStatus::Partial);
        assert!(result.numeric_observations.is_empty());
        assert_eq!(result.text_observations[0].text, "Unknown vehicle");
        assert_eq!(result.markers.len(), 2);
    }
}

#[test]
fn drops_stale_future_or_changed_aspect_frames_before_invoking_ocr() {
    for (timestamp, now, expected) in [
        (1000, 4000, AnalysisStatus::Stale),
        (4000, 1000, AnalysisStatus::Future),
    ] {
        let result =
            analyze_frame_with_ocr(&synthetic_map(), timestamp, now, &batch_config(), |_| {
                panic!("stale frame must not run OCR")
            })
            .unwrap();
        assert_eq!(result.status, expected);
        assert!(result.markers.is_empty());
    }
    let image = RgbaImage::new(160, 160);
    let result = analyze_frame_with_ocr(&image, 1000, 1000, &batch_config(), |_| {
        panic!("changed aspect must not run OCR")
    })
    .unwrap();
    assert_eq!(result.status, AnalysisStatus::Blocked);
}

#[test]
fn stabilizes_only_matching_fresh_region_ids_and_explicit_text() {
    let mut first_values = ["359.7", "Vehicle: BM-21 Grad"].into_iter();
    let first = analyze_frame_with_ocr(&synthetic_map(), 1000, 1000, &batch_config(), |_| {
        Ok(first_values.next().unwrap().into())
    })
    .unwrap();
    let mut second_values = ["0.1", "Vehicle: BM-21 Grad"].into_iter();
    let mut second = analyze_frame_with_ocr(&synthetic_map(), 1500, 1500, &batch_config(), |_| {
        Ok(second_values.next().unwrap().into())
    })
    .unwrap();
    stabilize_frame(&mut second, Some(&first));
    assert_eq!(second.numeric_observations[0].confidence, 0.9);
    assert_eq!(second.text_observations[0].confidence, 0.85);
    let mut changed = batch_config();
    changed.ocr_regions[0].id = "another-compass".into();
    let mut changed_values = ["0.1", "Different vehicle"].into_iter();
    let mut third = analyze_frame_with_ocr(&synthetic_map(), 1800, 1800, &changed, |_| {
        Ok(changed_values.next().unwrap().into())
    })
    .unwrap();
    stabilize_frame(&mut third, Some(&second));
    assert_eq!(third.numeric_observations[0].confidence, 0.6);
    assert_eq!(third.text_observations[0].confidence, 0.55);
}

#[test]
fn serializes_documented_configuration_and_defaults_confirmation_to_false() {
    let value = serde_json::to_value(batch_config()).unwrap();
    assert_eq!(value["ocrRegions"][1]["kind"], "vehicleName");
    assert_eq!(value["markerConfig"]["mapWidth"], 160);
    let mut value = value;
    value["markerConfig"]
        .as_object_mut()
        .unwrap()
        .remove("coordinateCalibrationConfirmed");
    let config: AnalysisConfig = serde_json::from_value(value).unwrap();
    assert!(
        !config
            .marker_config
            .unwrap()
            .coordinate_calibration_confirmed
    );
}

#[test]
fn invalidates_nested_and_flat_observations_after_slow_ocr() {
    let mut values = ["138", "Vehicle: BM-21 Grad"].into_iter();
    let mut result = analyze_frame_with_ocr(&synthetic_map(), 1000, 1000, &batch_config(), |_| {
        Ok(values.next().unwrap().into())
    })
    .unwrap();
    assert!(result.ocr_regions[0].observation.is_some());
    assert!(result.ocr_regions[1].text_observation.is_some());
    invalidate_frame(&mut result, AnalysisStatus::Stale, "OCR trwał zbyt długo");
    assert_eq!(result.status, AnalysisStatus::Stale);
    assert!(result.numeric_observations.is_empty());
    assert!(result.text_observations.is_empty());
    assert!(result.markers.is_empty());
    assert!(result
        .ocr_regions
        .iter()
        .all(|r| r.status == RegionStatus::Blocked
            && r.observation.is_none()
            && r.text_observation.is_none()));
    assert!(result
        .ocr_regions
        .iter()
        .all(|r| r.warning.as_deref() == Some("OCR trwał zbyt długo")));
}

#[test]
fn requires_a_selected_view_signature_even_when_map_geometry_is_confirmed() {
    let mut config = marker_config();
    config.view_signature = None;
    let result = detect_markers(&synthetic_map(), &config, 1000, 1000, 2000).unwrap();
    assert!(result.markers.is_empty());
    assert!(result.warning.unwrap().contains("deskryptora"));
    let mut serialized = serde_json::to_value(marker_config()).unwrap();
    serialized.as_object_mut().unwrap().remove("viewSignature");
    let restored: MarkerConfig = serde_json::from_value(serialized).unwrap();
    assert!(restored.view_signature.is_none());
}

#[test]
fn matches_reference_view_after_brightness_changes_and_small_marker_movement() {
    let reference = synthetic_map();
    let roi = marker_config().map_roi;
    let signature = calibrate_view_signature(&reference, roi).unwrap();
    assert_eq!(signature.gray_samples.len(), 256);
    let mut changed = reference.clone();
    for pixel in changed.pixels_mut() {
        for channel in 0..3 {
            pixel[channel] = (f64::from(pixel[channel]) * 1.1 + 12.0).min(255.0) as u8;
        }
    }
    fill_rect(&mut changed, 102, 35, 5, 5, [240, 0, 0, 255]);
    let result = match_view_signature(&changed, roi, &signature).unwrap();
    assert!(
        result.matched,
        "correlation={}, {:?}",
        result.correlation, result.warning
    );
    assert!(result.correlation >= 0.85);
}

#[test]
fn blocks_same_geometry_vehicle_view_with_coincidentally_matching_marker_colors() {
    let mut vehicle_view = synthetic_map();
    // Reverse the complete background layout while preserving candidate icon
    // colors and ROI geometry. This must not yield arbitrary world positions.
    for y in 20..80 {
        for x in 40..120 {
            let p = vehicle_view.get_pixel_mut(x, y);
            if p[0] == p[1] && p[1] == p[2] {
                let gray = 180 - p[0];
                *p = Rgba([gray, gray, gray, 255]);
            }
        }
    }
    let result = detect_markers(&vehicle_view, &marker_config(), 1000, 1000, 2000).unwrap();
    assert!(result.markers.is_empty());
    assert!(result.warning.unwrap().contains("nie pasuje"));
}

#[test]
fn refuses_uniform_weak_texture_and_flat_brightness_gradient_templates() {
    let roi = Roi {
        x: 0.0,
        y: 0.0,
        width: 1.0,
        height: 1.0,
    };
    let mut uniform = RgbaImage::from_pixel(128, 128, Rgba([80, 80, 80, 255]));
    assert!(calibrate_view_signature(&uniform, roi).is_err());
    fill_rect(&mut uniform, 50, 50, 5, 5, [0, 240, 0, 255]);
    assert!(calibrate_view_signature(&uniform, roi).is_err());
    let mut gradient = RgbaImage::new(128, 128);
    for (x, y, p) in gradient.enumerate_pixels_mut() {
        let gray = 40 + ((x + y) / 2) as u8;
        *p = Rgba([gray, gray, gray, 255]);
    }
    assert!(calibrate_view_signature(&gradient, roi).is_err());
    for (x, y, p) in gradient.enumerate_pixels_mut() {
        let gray = 80 + ((x * 11 + y * 37) % 3) as u8;
        *p = Rgba([gray, gray, gray, 255]);
    }
    assert!(calibrate_view_signature(&gradient, roi).is_err());
}

#[test]
fn rejects_wrong_geometry_rotation_and_tampered_descriptor() {
    let mut square = RgbaImage::new(128, 128);
    texture(&mut square, 0, 0, 128, 128);
    let roi = Roi {
        x: 0.0,
        y: 0.0,
        width: 1.0,
        height: 1.0,
    };
    let signature = calibrate_view_signature(&square, roi).unwrap();
    let rotated = image::imageops::rotate90(&square);
    assert!(
        !match_view_signature(&rotated, roi, &signature)
            .unwrap()
            .matched
    );
    let wider = image::imageops::resize(&square, 256, 128, image::imageops::FilterType::Nearest);
    assert!(
        !match_view_signature(&wider, roi, &signature)
            .unwrap()
            .matched
    );
    let mut invalid = signature.clone();
    invalid.threshold = 0.8;
    assert!(invalid.validate().is_err());
    invalid = signature.clone();
    invalid.gray_samples.clear();
    assert!(invalid.validate().is_err());
    invalid = signature;
    invalid.gray_samples.fill(100);
    assert!(invalid.validate().is_err());
}

#[test]
fn keeps_valid_hud_when_reference_map_view_is_not_visible() {
    let mut config = batch_config();
    config.marker_config.as_mut().unwrap().view_signature = None;
    let mut values = ["138", "Vehicle: BM-21 Grad"].into_iter();
    let result = analyze_frame_with_ocr(&synthetic_map(), 1000, 1000, &config, |_| {
        Ok(values.next().unwrap().into())
    })
    .unwrap();
    assert_eq!(result.status, AnalysisStatus::Partial);
    assert_eq!(result.numeric_observations[0].value, 138.0);
    assert!(result.markers.is_empty());
}

fn calibration() -> MapCalibration {
    MapCalibration {
        units_per_pixel: 2.0,
        north_rotation_deg: 0.0,
        origin_pixel: Point { x: 0.0, y: 0.0 },
        origin_world: Point { x: 100.0, y: 200.0 },
        uncertainty_units: 1.0,
    }
}

fn sample(x: f64, y: f64, timestamp_ms: u64) -> PositionSample {
    PositionSample {
        source_id: "manually-selected-target".into(),
        map_position: Point { x, y },
        timestamp_ms,
        confidence: 0.8,
    }
}

#[test]
fn estimates_scaled_east_and_north_motion_from_observed_positions_and_limits_prediction() {
    let samples = [
        sample(10.0, 20.0, 1000),
        sample(15.0, 18.0, 2000),
        sample(20.0, 16.0, 3000),
    ];
    let result =
        estimate_motion(&samples, &calibration(), 3100, &TrackerConfig::default()).unwrap();
    assert_eq!(result.status, MotionStatus::Ready);
    assert_eq!(result.count, 3);
    let velocity = result.velocity.unwrap();
    assert!((velocity[0] - 10.0).abs() < 1e-9);
    assert!((velocity[1] - 4.0).abs() < 1e-9);
    let predicted = predict_position(&result, 2.0).unwrap();
    assert!((predicted.x - 160.0).abs() < 1e-9);
    assert!((predicted.y - 176.0).abs() < 1e-9);
    assert!(predict_position(&result, 11.0).is_err());
    assert_eq!(result.timestamp_ms, 3000);
}

#[test]
fn rotates_tracker_velocity_according_to_map_calibration() {
    let mut calibration = calibration();
    calibration.north_rotation_deg = 90.0;
    let result = estimate_motion(
        &[sample(10.0, 20.0, 1000), sample(10.0, 15.0, 2000)],
        &calibration,
        2000,
        &TrackerConfig::default(),
    )
    .unwrap();
    let velocity = result.velocity.unwrap();
    assert!((velocity[0] - 10.0).abs() < 1e-9);
    assert!(velocity[1].abs() < 1e-9);
}

#[test]
fn requires_two_fresh_positions_and_rejects_stale_future_or_out_of_bounds_samples() {
    let config = TrackerConfig::default();
    assert_eq!(
        estimate_motion(&[sample(10.0, 20.0, 1000)], &calibration(), 1100, &config)
            .unwrap()
            .status,
        MotionStatus::Collecting
    );
    assert_eq!(
        estimate_motion(
            &[sample(10.0, 20.0, 1000), sample(11.0, 20.0, 2000)],
            &calibration(),
            6000,
            &config
        )
        .unwrap()
        .status,
        MotionStatus::Stale
    );
    assert_eq!(
        estimate_motion(
            &[sample(10.0, 20.0, 1000), sample(11.0, 20.0, 2000)],
            &calibration(),
            1500,
            &config
        )
        .unwrap()
        .status,
        MotionStatus::Rejected
    );
    for position in [(-1.0, 10.0), (800.0, 10.0), (10.0, 600.0), (f64::NAN, 10.0)] {
        let result = estimate_motion(
            &[
                sample(10.0, 20.0, 1000),
                sample(position.0, position.1, 2000),
            ],
            &calibration(),
            2000,
            &config,
        )
        .unwrap();
        assert_eq!(result.status, MotionStatus::Rejected);
        assert!(result.velocity.is_none());
    }
}

#[test]
fn rejects_marker_switches_jumps_duplicate_timestamps_and_low_confidence() {
    let config = TrackerConfig {
        max_speed_units_per_second: 10.0,
        ..TrackerConfig::default()
    };
    for second in [
        sample(100.0, 20.0, 2000),
        PositionSample {
            source_id: "other-target".into(),
            ..sample(11.0, 20.0, 2000)
        },
        sample(11.0, 20.0, 1000),
        PositionSample {
            confidence: 0.3,
            ..sample(11.0, 20.0, 2000)
        },
    ] {
        let result = estimate_motion(
            &[sample(10.0, 20.0, 1000), second],
            &calibration(),
            2000,
            &config,
        )
        .unwrap();
        assert_eq!(result.status, MotionStatus::Rejected);
        assert!(result.velocity.is_none());
    }
}

#[test]
fn excludes_expired_history_and_rejects_observed_maneuvers_that_do_not_fit_constant_velocity() {
    let samples = [
        sample(1.0, 1.0, 0),
        sample(10.0, 20.0, 4000),
        sample(15.0, 20.0, 5000),
    ];
    let result =
        estimate_motion(&samples, &calibration(), 5000, &TrackerConfig::default()).unwrap();
    assert_eq!(result.status, MotionStatus::Ready);
    assert_eq!(result.count, 2);
    let maneuver = [
        sample(10.0, 20.0, 3000),
        sample(50.0, 20.0, 4000),
        sample(10.0, 20.0, 5000),
    ];
    let result =
        estimate_motion(&maneuver, &calibration(), 5000, &TrackerConfig::default()).unwrap();
    assert_eq!(result.status, MotionStatus::Rejected);
}
