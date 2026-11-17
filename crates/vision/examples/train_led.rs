//! Trains the MTC weapon-panel digit templates from labelled local captures.
//!
//! cargo run -p mtc-vision --example train_led -- <frames-dir> <led-labels.json> [out.json] [--cv]
//!
//! Frames `live-NNN-*.png` are full 1920x1080 client captures that stay outside
//! the repository. Frames used as committed fixtures are excluded, so fixture
//! tests measure accuracy on frames the templates have not seen.
use mtc_vision::{
    angle_value_bands, decode_led_value, locate_weapon_panel, train_led_model, LedBand,
};
use std::path::Path;

const INITIAL_WIDTHS: [(char, usize); 13] = [
    ('0', 7),
    ('1', 4),
    ('2', 7),
    ('3', 7),
    ('4', 7),
    ('5', 7),
    ('6', 7),
    ('7', 7),
    ('8', 7),
    ('9', 7),
    ('.', 2),
    ('-', 6),
    ('°', 4),
];

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = Path::new(&args[1]);
    let labels: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&args[2]).unwrap()).unwrap();
    let excluded: Vec<String> = labels["fixtureFrames"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    let mut samples: Vec<(String, LedBand, String)> = Vec::new();
    for (frame, value) in labels["frames"].as_object().unwrap() {
        let path = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .find(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(&format!("live-{frame}-"))
            })
            .unwrap_or_else(|| panic!("frame {frame}"));
        let image = image::open(&path).unwrap().to_rgba8();
        let layout = locate_weapon_panel(&image).unwrap_or_else(|| panic!("panel {frame}"));
        let (elevation, azimuth) =
            angle_value_bands(&image, &layout).unwrap_or_else(|| panic!("bands {frame}"));
        samples.push((
            frame.clone(),
            elevation,
            value["elevation"].as_str().unwrap().into(),
        ));
        samples.push((
            frame.clone(),
            azimuth,
            value["azimuth"].as_str().unwrap().into(),
        ));
    }
    let training: Vec<(LedBand, String)> = samples
        .iter()
        .filter(|(frame, _, _)| !excluded.contains(frame))
        .map(|(_, b, l)| (b.clone(), l.clone()))
        .collect();
    if args.iter().any(|a| a == "--cv") {
        let names: Vec<&String> = {
            let mut n: Vec<&String> = samples
                .iter()
                .map(|s| &s.0)
                .filter(|f| !excluded.contains(f))
                .collect();
            n.dedup();
            n
        };
        let (mut ok, mut total) = (0, 0);
        for fold in 0..2 {
            let train: Vec<(LedBand, String)> = samples
                .iter()
                .filter(|(f, _, _)| {
                    !excluded.contains(f) && names.iter().position(|n| *n == f).unwrap() % 2 != fold
                })
                .map(|(_, b, l)| (b.clone(), l.clone()))
                .collect();
            let model = train_led_model(&train, &INITIAL_WIDTHS);
            for (frame, band, label) in samples.iter().filter(|(f, _, _)| {
                !excluded.contains(f) && names.iter().position(|n| *n == f).unwrap() % 2 == fold
            }) {
                let got = decode_led_value(band, &model).map(|r| r.text);
                total += 1;
                if got.as_deref() == Some(label.as_str()) {
                    ok += 1;
                } else {
                    println!("CV miss {frame}: {label} -> {got:?}");
                }
            }
        }
        println!("2-fold CV: {ok}/{total}");
    }
    let model = train_led_model(&training, &INITIAL_WIDTHS);
    let (mut ok, mut total) = (0, 0);
    for (frame, band, label) in &samples {
        let got = decode_led_value(band, &model).map(|r| (r.text, r.mean_cost));
        total += 1;
        let hit = got.as_ref().is_some_and(|g| g.0 == *label);
        ok += usize::from(hit);
        let tag = if excluded.contains(frame) {
            "holdout"
        } else {
            "train"
        };
        if !hit || tag == "holdout" {
            println!("{tag} {frame}: {label} -> {got:?}");
        }
    }
    println!(
        "all frames: {ok}/{total}; max mean cost {}",
        model.max_mean_cost
    );
    if let Some(out) = args.get(3).filter(|a| !a.starts_with("--")) {
        std::fs::write(out, serde_json::to_string(&model).unwrap()).unwrap();
        println!("wrote {out}");
    }
}
