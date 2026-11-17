use image::{Rgba, RgbaImage};
use mtc_maps::Point;
use mtc_vision::*;

fn roi() -> Roi {
    Roi {
        x: 0.2,
        y: 0.25,
        width: 0.6,
        height: 0.5,
    }
}
fn rectangle(image: &mut RgbaImage, x: u32, y: u32, width: u32, height: u32, color: [u8; 4]) {
    for py in y..y + height {
        for px in x..x + width {
            image.put_pixel(px, py, Rgba(color));
        }
    }
}
fn map_image() -> RgbaImage {
    let mut image = RgbaImage::from_pixel(160, 120, Rgba([80, 80, 80, 255]));
    rectangle(&mut image, 45, 45, 7, 7, [240, 30, 30, 255]);
    rectangle(&mut image, 100, 65, 7, 7, [240, 30, 30, 255]);
    rectangle(&mut image, 75, 40, 5, 5, [0, 220, 250, 255]);
    rectangle(&mut image, 5, 5, 7, 7, [240, 30, 30, 255]); // Outside user map ROI.
    rectangle(&mut image, 90, 60, 1, 2, [0, 220, 250, 255]); // Tiny alias/noise.
    rectangle(&mut image, 80, 75, 5, 5, [240, 30, 30, 20]); // Invisible/transparent fringe.
    image
}
fn click(x: f64, y: f64) -> Point {
    Point {
        x: x / 160.0,
        y: y / 120.0,
    }
}

#[test]
fn proposes_visible_components_without_roles_or_world_coordinates() {
    let report = propose_marker_signatures(&map_image(), roi(), 1000, 1100).unwrap();
    assert_eq!(report.status, MarkerProposalStatus::Proposed);
    assert_eq!(report.proposals.len(), 3);
    assert_eq!(
        (report.observed_map_width, report.observed_map_height),
        (96, 60)
    );
    for p in &report.proposals {
        assert_eq!(p.signature.role, MarkerRole::Unassigned);
        assert!(p.confidence < 0.85 && !p.clipped);
        p.signature.validate().unwrap();
        p.bounding_box.validate().unwrap();
        assert!(p.frame_position_normalized.x >= roi().x);
    }
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["schemaVersion"], 1);
    assert_eq!(json["proposals"][0]["signature"]["role"], "unassigned");
    assert!(!json.to_string().contains("mapPosition"));
    assert!(!json.to_string().contains("worldPosition"));
}

#[test]
fn calibrates_clicked_component_among_identical_colors_using_full_frame_point() {
    let image = map_image();
    let left = calibrate_marker_signature(&image, roi(), click(48.5, 48.5), 1000, 1100).unwrap();
    let right = calibrate_marker_signature(&image, roi(), click(103.5, 68.5), 1000, 1100).unwrap();
    assert_eq!(left.status, MarkerCalibrationStatus::Calibrated);
    assert_eq!(right.status, MarkerCalibrationStatus::Calibrated);
    let a = left.proposal.unwrap();
    let b = right.proposal.unwrap();
    assert_eq!(a.signature.rgb, [240, 30, 30]);
    assert_eq!(a.signature.role, MarkerRole::Unassigned);
    assert_eq!(a.signature.id, b.signature.id);
    assert_ne!(a.id, b.id);
    assert_eq!(a.pixel_count, 49);
    assert!((a.frame_position_normalized.x - 48.5 / 160.0).abs() < 1e-10);
    assert!(a.signature.min_component_pixels <= a.pixel_count);
    assert!(a.signature.max_component_pixels >= a.pixel_count);
}

#[test]
fn handles_hollow_transparent_centers_white_icons_and_minor_color_aliasing() {
    let mut image = RgbaImage::from_pixel(160, 120, Rgba([80, 80, 80, 255]));
    rectangle(&mut image, 50, 50, 9, 9, [0, 220, 250, 255]);
    rectangle(&mut image, 51, 51, 7, 7, [0, 0, 0, 0]);
    image.put_pixel(50, 50, Rgba([8, 210, 240, 255]));
    rectangle(&mut image, 80, 50, 5, 5, [235, 235, 235, 255]);
    let hollow = calibrate_marker_signature(&image, roi(), click(54.5, 54.5), 1000, 1100).unwrap();
    assert_eq!(hollow.status, MarkerCalibrationStatus::Calibrated);
    let p = hollow.proposal.unwrap();
    assert_eq!(p.pixel_count, 32);
    assert!(p.fill_ratio < 0.5 && p.signature.min_fill_ratio < p.fill_ratio);
    assert!(p.color_spread <= 20);
    let white = calibrate_marker_signature(&image, roi(), click(82.5, 52.5), 1000, 1100).unwrap();
    assert_eq!(white.status, MarkerCalibrationStatus::Calibrated);
    assert_eq!(white.proposal.unwrap().signature.rgb, [235, 235, 235]);
    // Capture is often opaque: the empty center shows the same dark map color.
    let mut opaque = RgbaImage::from_pixel(160, 120, Rgba([20, 20, 20, 255]));
    rectangle(&mut opaque, 50, 50, 9, 9, [0, 220, 250, 255]);
    rectangle(&mut opaque, 51, 51, 7, 7, [20, 20, 20, 255]);
    let ring = calibrate_marker_signature(&opaque, roi(), click(54.5, 54.5), 1000, 1100).unwrap();
    assert_eq!(ring.status, MarkerCalibrationStatus::Calibrated);
    assert_eq!(ring.proposal.unwrap().signature.rgb, [0, 220, 250]);
}

#[test]
fn never_calibrates_equal_distance_icons_or_clipped_and_outside_points() {
    let mut image = RgbaImage::from_pixel(160, 120, Rgba([80, 80, 80, 255]));
    rectangle(&mut image, 55, 50, 3, 3, [240, 30, 30, 255]);
    rectangle(&mut image, 61, 50, 3, 3, [240, 30, 30, 255]);
    let tie = calibrate_marker_signature(&image, roi(), click(59.5, 51.5), 1000, 1100).unwrap();
    assert_eq!(tie.status, MarkerCalibrationStatus::Ambiguous);
    assert!(tie.proposal.is_none());
    rectangle(&mut image, 32, 40, 5, 5, [0, 220, 250, 255]);
    let clipped = calibrate_marker_signature(&image, roi(), click(34.5, 42.5), 1000, 1100).unwrap();
    assert_eq!(clipped.status, MarkerCalibrationStatus::Blocked);
    assert!(clipped.proposal.is_none());
    let outside = calibrate_marker_signature(&image, roi(), click(5.0, 5.0), 1000, 1100).unwrap();
    assert_eq!(outside.status, MarkerCalibrationStatus::Blocked);
    assert!(calibrate_marker_signature(
        &image,
        roi(),
        Point {
            x: f64::NAN,
            y: 0.5
        },
        1000,
        1100
    )
    .is_err());
}

#[test]
fn blocks_stale_future_noise_and_oversized_proposals_without_arbitrary_subset() {
    let image = map_image();
    for now in [900, 4000] {
        assert_eq!(
            propose_marker_signatures(&image, roi(), 1000, now)
                .unwrap()
                .status,
            MarkerProposalStatus::Stale
        );
        let c = calibrate_marker_signature(&image, roi(), click(48.5, 48.5), 1000, now).unwrap();
        assert_eq!(c.status, MarkerCalibrationStatus::Stale);
        assert!(c.proposal.is_none());
    }
    for config in [
        MarkerProposalConfig {
            max_proposals: 1,
            ..Default::default()
        },
        MarkerProposalConfig {
            max_components: 1,
            ..Default::default()
        },
        MarkerProposalConfig {
            max_analysis_pixels: 10,
            ..Default::default()
        },
    ] {
        let p = propose_marker_signatures_with_config(&image, roi(), 1000, 1100, &config).unwrap();
        assert_eq!(p.status, MarkerProposalStatus::Blocked);
        assert!(p.proposals.is_empty());
    }
}

fn policy() -> MarkerSelectionConfig {
    MarkerSelectionConfig {
        scope: "epoch-7/map-a/calibration-1".into(),
        frame_width: 160,
        frame_height: 120,
        map_width: 800,
        map_height: 600,
        max_age_ms: 2000,
        initial_radius_pixels: 8.0,
        base_gate_pixels: 2.0,
        max_association_radius_pixels: 30.0,
        max_speed_pixels_per_second: 40.0,
        ambiguity_margin_pixels: 1.0,
        min_confidence: 0.6,
    }
}
fn hint() -> MarkerSelectionHint {
    MarkerSelectionHint {
        selection_id: "explicit-target-1".into(),
        signature_id: "common-red".into(),
        role: MarkerRole::Target,
        confirmed_point: Point { x: 100.0, y: 100.0 },
        timestamp_ms: 1000,
        scope: policy().scope,
        frame_width: 160,
        frame_height: 120,
        map_width: 800,
        map_height: 600,
    }
}
fn candidate(x: f64, y: f64, timestamp: u64) -> MarkerObservation {
    MarkerObservation {
        signature_id: "common-red".into(),
        role: MarkerRole::Unassigned,
        map_position: Point { x, y },
        frame_position_normalized: Point {
            x: x / 800.0,
            y: y / 600.0,
        },
        bounding_box: Roi {
            x: 0.1,
            y: 0.1,
            width: 0.02,
            height: 0.02,
        },
        pixel_count: 49,
        confidence: 0.8,
        timestamp_ms: timestamp,
        source: "Synthetic visible component".into(),
    }
}
fn choose(
    candidates: &[MarkerObservation],
    previous: Option<&MarkerSelectionState>,
    timestamp: u64,
    now: u64,
) -> MarkerSelectionResult {
    select_confirmed_marker(candidates, &hint(), previous, timestamp, now, &policy()).unwrap()
}

#[test]
fn chooses_explicit_point_independent_of_candidate_order_and_needs_two_fresh_frames() {
    let first = choose(
        &[candidate(150.0, 100.0, 1000), candidate(100.0, 100.0, 1000)],
        None,
        1000,
        1100,
    );
    assert_eq!(first.status, MarkerSelectionStatus::Collecting);
    assert_eq!(first.track_id.as_deref(), Some("explicit-target-1"));
    assert_eq!(first.selected.as_ref().unwrap().role, MarkerRole::Target);
    assert_eq!(first.selected.as_ref().unwrap().map_position.x, 100.0);
    assert!(first.confidence < 0.85);
    let repeated = choose(
        &[candidate(100.0, 100.0, 1000)],
        first.state.as_ref(),
        1000,
        1200,
    );
    assert_eq!(repeated.status, MarkerSelectionStatus::Blocked);
    assert_eq!(repeated.count, 1);
    assert!(repeated.selected.is_none());
    let second = choose(
        &[candidate(150.0, 100.0, 1500), candidate(110.0, 100.0, 1500)],
        first.state.as_ref(),
        1500,
        1600,
    );
    assert_eq!(second.status, MarkerSelectionStatus::Ready);
    assert_eq!(second.count, 2);
    assert_eq!(second.confidence, 0.9);
    // Follow observed motion rather than a different icon closer to the old position.
    let third = choose(
        &[candidate(112.0, 100.0, 2000), candidate(120.0, 100.0, 2000)],
        second.state.as_ref(),
        2000,
        2100,
    );
    assert_eq!(third.status, MarkerSelectionStatus::Ready);
    assert_eq!(third.selected.unwrap().map_position.x, 120.0);
}

#[test]
fn refuses_ties_far_candidates_teleports_and_low_confidence_without_first_candidate_fallback() {
    let tie = choose(
        &[candidate(99.0, 100.0, 1000), candidate(101.0, 100.0, 1000)],
        None,
        1000,
        1100,
    );
    assert_eq!(tie.status, MarkerSelectionStatus::Ambiguous);
    assert!(tie.selected.is_none() && tie.state.is_none());
    let far = choose(&[candidate(120.0, 100.0, 1000)], None, 1000, 1100);
    assert_eq!(far.status, MarkerSelectionStatus::Rejected);
    let first = choose(&[candidate(100.0, 100.0, 1000)], None, 1000, 1100);
    let jump = choose(
        &[candidate(400.0, 100.0, 1500)],
        first.state.as_ref(),
        1500,
        1600,
    );
    assert_eq!(jump.status, MarkerSelectionStatus::Rejected);
    assert!(jump.selected.is_none() && jump.state.is_none());
    let mut low = candidate(100.0, 100.0, 1000);
    low.confidence = 0.35;
    assert!(choose(&[low], None, 1000, 1100).selected.is_none());
}

#[test]
fn rejects_stale_future_wrong_frame_and_changed_map_source_or_geometry() {
    let first = choose(&[candidate(100.0, 100.0, 1000)], None, 1000, 1100);
    for (timestamp, now) in [(1000, 4000), (2000, 3300), (1500, 1400)] {
        let result = choose(
            &[candidate(100.0, 100.0, timestamp)],
            first.state.as_ref(),
            timestamp,
            now,
        );
        assert_eq!(result.status, MarkerSelectionStatus::Stale);
        assert!(result.selected.is_none() && result.state.is_none());
    }
    let mixed = choose(
        &[candidate(100.0, 100.0, 1000), candidate(150.0, 100.0, 900)],
        None,
        1000,
        1100,
    );
    assert_eq!(mixed.status, MarkerSelectionStatus::Rejected);
    for scenario in 0..3 {
        let mut config = policy();
        match scenario {
            0 => config.scope = "epoch-8/map-a/calibration-1".into(),
            1 => config.map_width = 801,
            _ => config.frame_width = 320,
        }
        let result = select_confirmed_marker(
            &[candidate(100.0, 100.0, 1500)],
            &hint(),
            first.state.as_ref(),
            1500,
            1600,
            &config,
        )
        .unwrap();
        assert_eq!(result.status, MarkerSelectionStatus::Blocked);
        assert!(result.selected.is_none() && result.state.is_none());
    }
}

#[test]
fn manually_assigns_two_distinct_tracks_from_one_unassigned_color_class() {
    let candidates = vec![candidate(100.0, 100.0, 1000), candidate(150.0, 100.0, 1000)];
    let target = choose(&candidates, None, 1000, 1100);
    let mut own_hint = hint();
    own_hint.selection_id = "explicit-own-1".into();
    own_hint.role = MarkerRole::Own;
    own_hint.confirmed_point.x = 150.0;
    let own = select_confirmed_marker(&candidates, &own_hint, None, 1000, 1100, &policy()).unwrap();
    assert_eq!(own.selected.unwrap().map_position.x, 150.0);
    assert_eq!(target.selected.unwrap().map_position.x, 100.0);
    assert_ne!(own.track_id, target.track_id);
    // A new explicit click resets continuity even if the color class stays the same.
    own_hint.timestamp_ms = 1500;
    let reset = select_confirmed_marker(
        &[candidate(150.0, 100.0, 1500)],
        &own_hint,
        target.state.as_ref(),
        1500,
        1600,
        &policy(),
    )
    .unwrap();
    assert_eq!(reset.status, MarkerSelectionStatus::Collecting);
    assert_eq!(reset.count, 1);
}

#[test]
fn maintains_explicit_track_with_fresh_observations_but_never_reacquires_old_click_after_a_gap() {
    let mut result = choose(&[candidate(100.0, 100.0, 1000)], None, 1000, 1100);
    for (timestamp, x) in [(1500, 110.0), (2000, 120.0), (2500, 130.0), (3500, 150.0)] {
        result = choose(
            &[candidate(x, 100.0, timestamp)],
            result.state.as_ref(),
            timestamp,
            timestamp + 100,
        );
        assert_eq!(result.status, MarkerSelectionStatus::Ready);
        assert_eq!(result.selected.as_ref().unwrap().map_position.x, x);
    }
    assert_eq!(
        choose(&[candidate(100.0, 100.0, 3500)], None, 3500, 3600).status,
        MarkerSelectionStatus::Stale
    );
    let gap = choose(
        &[candidate(150.0, 100.0, 7000)],
        result.state.as_ref(),
        7000,
        7100,
    );
    assert_eq!(gap.status, MarkerSelectionStatus::Stale);
    assert!(gap.selected.is_none() && gap.state.is_none());
}

#[test]
fn rejects_out_of_bounds_and_corrupted_history_without_predicting_invisible_positions() {
    let first = choose(&[candidate(100.0, 100.0, 1000)], None, 1000, 1100);
    assert!(choose(
        &[candidate(900.0, 100.0, 1500)],
        first.state.as_ref(),
        1500,
        1600
    )
    .selected
    .is_none());
    let mut state = first.state.unwrap();
    state.previous = Some(candidate(100.0, 100.0, 1000)); // Duplicate source time.
    let invalid = choose(&[candidate(110.0, 100.0, 1500)], Some(&state), 1500, 1600);
    assert_eq!(invalid.status, MarkerSelectionStatus::Rejected);
    assert!(invalid.selected.is_none() && invalid.state.is_none());
    let mut outside = candidate(100.0, 100.0, 1000);
    outside.frame_position_normalized.x = 1.1;
    assert_eq!(
        choose(&[outside], None, 1000, 1100).status,
        MarkerSelectionStatus::Rejected
    );
}
