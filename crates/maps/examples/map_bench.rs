//! Benchmark dopasowania widoku mapy na lokalnych teksturach społeczności.
//!
//! Rastry NIE są częścią repozytorium (licencja redystrybucji niepotwierdzona).
//! Wskaż katalog z plikami `<id>.png` argumentem lub zmienną `MTC_MAP_BENCH_DIR`:
//!
//! ```text
//! cargo run -p mtc-maps --release --example map_bench -- C:\...\mtc-map-bench\maps
//! cargo run -p mtc-maps --example map_bench -- C:\...\maps --quick
//! ```
//!
//! Opcje: `--quick` (mniej map i scenariuszy), `--maps a,b,c`, `--dump <katalog>`
//! (zapis kilku wygenerowanych klatek do podglądu), `--csv <plik>`, `--seed <n>`,
//! `--budget <ms>` (domyślnie 500).
//!
//! Każda klatka jest syntetycznym „zrzutem z gry” zbudowanym z tekstury referencyjnej:
//! ciemne tło lub scena, HUD, nałożona siatka z literami/cyframi, kolorowe markery,
//! zmiana jasności/kontrastu/gammy, rozmycie i kompresja JPEG. Znana transformacja
//! pozwala zmierzyć błąd punktu w pikselach referencji.

use image::{codecs::jpeg::JpegEncoder, imageops, imageops::FilterType, Rgba, RgbaImage};
use mtc_maps::{
    match_map_view, MapCalibration, MapMatchConfig, MapReference, MapReferenceMetadata,
    MapViewStatus, Point,
};
use std::{
    collections::BTreeMap,
    env, fs,
    io::Write as _,
    path::{Path, PathBuf},
    time::Instant,
};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn range(&mut self, a: f64, b: f64) -> f64 {
        a + (b - a) * self.unit()
    }
    fn int(&mut self, a: u32, b: u32) -> u32 {
        a + (self.next() % (b - a + 1) as u64) as u32
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Full1920,
    Full1366,
    Zoom,
    Minimap,
    Holdout,
    NoMap,
}
impl Kind {
    fn label(self) -> &'static str {
        match self {
            Kind::Full1920 => "pełna mapa 1920×1080",
            Kind::Full1366 => "pełna mapa 1366×768",
            Kind::Zoom => "kadr zoom 1.5–4× (1920×1080)",
            Kind::Minimap => "minimapa 200–300 px w rogu",
            Kind::Holdout => "negatyw: mapa spoza katalogu",
            Kind::NoMap => "negatyw: brak mapy (scena+HUD)",
        }
    }
    fn positive(self) -> bool {
        !matches!(self, Kind::Holdout | Kind::NoMap)
    }
}

/// reference = ref_origin + (q - screen_origin + 0.5) * scale - 0.5
#[derive(Clone, Copy)]
struct Truth {
    screen: (f64, f64, f64, f64),
    ref_origin: (f64, f64),
    scale: f64,
}
impl Truth {
    fn reference(&self, q: Point) -> Point {
        Point {
            x: self.ref_origin.0 + (q.x - self.screen.0 + 0.5) * self.scale - 0.5,
            y: self.ref_origin.1 + (q.y - self.screen.1 + 0.5) * self.scale - 0.5,
        }
    }
}

struct Scenario {
    kind: Kind,
    map: usize,
    frame: RgbaImage,
    truth: Option<Truth>,
    description: String,
}

fn blend(img: &mut RgbaImage, x: i64, y: i64, color: [u8; 3], alpha: f64) {
    if x < 0 || y < 0 || x >= img.width() as i64 || y >= img.height() as i64 {
        return;
    }
    let p = img.get_pixel_mut(x as u32, y as u32);
    for c in 0..3 {
        p[c] = (p[c] as f64 * (1.0 - alpha) + color[c] as f64 * alpha).round() as u8;
    }
    p[3] = 255;
}
fn fill_rect(img: &mut RgbaImage, x: i64, y: i64, w: i64, h: i64, color: [u8; 3], alpha: f64) {
    for yy in y..y + h {
        for xx in x..x + w {
            blend(img, xx, yy, color, alpha);
        }
    }
}
fn fill_circle(img: &mut RgbaImage, cx: f64, cy: f64, r: f64, color: [u8; 3], alpha: f64) {
    for yy in (cy - r).floor() as i64..=(cy + r).ceil() as i64 {
        for xx in (cx - r).floor() as i64..=(cx + r).ceil() as i64 {
            if (xx as f64 - cx).hypot(yy as f64 - cy) <= r {
                blend(img, xx, yy, color, alpha);
            }
        }
    }
}
fn fill_triangle(img: &mut RgbaImage, cx: f64, cy: f64, r: f64, color: [u8; 3]) {
    for yy in (cy - r) as i64..=(cy + r) as i64 {
        let t = (yy as f64 - (cy - r)) / (2.0 * r);
        let half = r * t;
        for xx in (cx - half) as i64..=(cx + half) as i64 {
            blend(img, xx, yy, color, 1.0);
        }
    }
}

/// Deterministic 5×7 pseudo-glyph per character: font-like strokes for the corner detector.
fn glyph(c: char) -> [u8; 7] {
    let mut h = (c as u32).wrapping_mul(2_654_435_761) ^ 0x5bd1_e995;
    let mut rows = [0u8; 7];
    for (i, row) in rows.iter_mut().enumerate() {
        h ^= h >> 13;
        h = h.wrapping_mul(0x5bd1_e995);
        h ^= h >> 15;
        *row = (h & 0x1f) as u8;
        if i == 0 || i == 6 || i == 3 {
            *row |= 0b01110;
        }
        *row |= 0b10000;
    }
    rows
}
fn draw_text(img: &mut RgbaImage, x: i64, y: i64, text: &str, px: i64, color: [u8; 3]) {
    let mut cx = x;
    for c in text.chars() {
        if c != ' ' {
            let g = glyph(c);
            for (row, bits) in g.iter().enumerate() {
                for col in 0..5 {
                    if bits & (1 << (4 - col)) != 0 {
                        fill_rect(img, cx + col * px, y + row as i64 * px, px, px, color, 1.0);
                    }
                }
            }
        }
        cx += 6 * px;
    }
}

fn dark_background(w: u32, h: u32, rng: &mut Rng) -> RgbaImage {
    let base = rng.range(12.0, 40.0);
    let tint = [
        rng.range(0.9, 1.1),
        rng.range(0.9, 1.1),
        rng.range(0.95, 1.25),
    ];
    let mut img = RgbaImage::from_fn(w, h, |x, y| {
        let g = base + 10.0 * ((x as f64 / 97.0).sin() * (y as f64 / 61.0).cos());
        Rgba([
            (g * tint[0]).clamp(0.0, 255.0) as u8,
            (g * tint[1]).clamp(0.0, 255.0) as u8,
            (g * tint[2]).clamp(0.0, 255.0) as u8,
            255,
        ])
    });
    // Dimmed 3D scene silhouettes behind the map overlay.
    for _ in 0..30 {
        let cx = rng.range(0.0, w as f64);
        let cy = rng.range(h as f64 * 0.4, h as f64);
        fill_triangle(&mut img, cx, cy, rng.range(10.0, 40.0), [20, 34, 22]);
    }
    img
}

fn scene_background(w: u32, h: u32, rng: &mut Rng) -> RgbaImage {
    let horizon = h as f64 * rng.range(0.35, 0.55);
    let mut noise = Rng(rng.next());
    let mut img = RgbaImage::from_fn(w, h, |_, y| {
        if (y as f64) < horizon {
            let t = y as f64 / horizon;
            Rgba([
                (120.0 + 80.0 * t) as u8,
                (170.0 + 50.0 * t) as u8,
                (230.0 + 15.0 * t) as u8,
                255,
            ])
        } else {
            Rgba([70, 120, 40, 255])
        }
    });
    for y in horizon as u32..h {
        for x in 0..w {
            let n = (noise.next() % 40) as i32 - 20;
            let p = img.get_pixel_mut(x, y);
            p[0] = (p[0] as i32 + n / 2).clamp(0, 255) as u8;
            p[1] = (p[1] as i32 + n).clamp(0, 255) as u8;
        }
    }
    for _ in 0..80 {
        let cx = rng.range(0.0, w as f64);
        let cy = rng.range(horizon - 20.0, h as f64);
        fill_triangle(&mut img, cx, cy, rng.range(8.0, 45.0), [25, 60, 30]);
    }
    for _ in 0..6 {
        fill_circle(
            &mut img,
            rng.range(0.0, w as f64),
            rng.range(0.0, horizon),
            rng.range(30.0, 90.0),
            [245, 245, 250],
            0.8,
        );
    }
    img
}

fn draw_hud(img: &mut RgbaImage, rng: &mut Rng) {
    let w = img.width() as i64;
    let h = img.height() as i64;
    let k = (w as f64 / 1920.0).max(0.5);
    let s = |v: f64| (v * k).round() as i64;
    // Timer pill and compass strip.
    fill_rect(
        img,
        w / 2 - s(60.0),
        s(20.0),
        s(120.0),
        s(50.0),
        [35, 45, 75],
        0.95,
    );
    let timer = format!("{:02}:{:02}", rng.int(0, 29), rng.int(0, 59));
    draw_text(
        img,
        w / 2 - s(45.0),
        s(30.0),
        &timer,
        s(4.0).max(2),
        [240, 240, 240],
    );
    for i in 0..24 {
        let x = w / 2 - s(450.0) + i * s(38.0);
        let tall = if i % 3 == 0 { s(18.0) } else { s(9.0) };
        fill_rect(img, x, s(90.0), s(2.0).max(1), tall, [230, 230, 230], 0.8);
        if i % 3 == 0 {
            let label = format!("{:03}", (i * 15 + rng.int(0, 2) as i64 * 5) % 360);
            draw_text(
                img,
                x - s(10.0),
                s(112.0),
                &label,
                s(2.0).max(1),
                [220, 220, 220],
            );
        }
    }
    // Menu button, team icon, bottom-left vehicle label, ammo counter.
    fill_rect(img, s(10.0), s(30.0), s(120.0), s(45.0), [35, 45, 75], 0.95);
    draw_text(
        img,
        s(25.0),
        s(40.0),
        "MENU",
        s(4.0).max(2),
        [240, 240, 240],
    );
    fill_circle(
        img,
        s(170.0) as f64,
        s(52.0) as f64,
        s(25.0) as f64,
        [35, 45, 75],
        0.95,
    );
    draw_text(
        img,
        s(10.0),
        h - s(40.0),
        "MANAGE VEHICLE",
        s(3.0).max(2),
        [240, 240, 240],
    );
    fill_rect(
        img,
        w - s(260.0),
        h - s(90.0),
        s(240.0),
        s(70.0),
        [20, 20, 20],
        0.7,
    );
    draw_text(
        img,
        w - s(250.0),
        h - s(75.0),
        &format!("{} {}", rng.int(1, 40), rng.int(100, 999)),
        s(5.0).max(2),
        [250, 210, 60],
    );
}

const COLUMNS: [char; 12] = ['E', 'F', 'G', 'H', 'J', 'K', 'L', 'M', 'N', 'O', 'P', 'Q'];

/// MTC-like grid: letter columns (without I), numbered rows, thin translucent lines.
fn draw_grid(img: &mut RgbaImage, x: i64, y: i64, w: i64, h: i64, rng: &mut Rng) {
    let cells = rng.int(6, 10) as i64;
    let alpha = rng.range(0.25, 0.55);
    let color = if rng.unit() < 0.5 {
        [230, 230, 230]
    } else {
        [15, 15, 15]
    };
    let px = (w / 300).clamp(1, 3);
    for i in 0..=cells {
        let gx = x + i * w / cells;
        let gy = y + i * h / cells;
        fill_rect(img, gx, y, 1, h, color, alpha);
        fill_rect(img, x, gy, w, 1, color, alpha);
        if i < cells {
            let col = COLUMNS[(i as usize) % COLUMNS.len()].to_string();
            draw_text(
                img,
                gx + w / cells / 2 - 3 * px,
                y - 9 * px,
                &col,
                px,
                [235, 235, 235],
            );
            draw_text(
                img,
                x - 13 * px,
                gy + h / cells / 2 - 4 * px,
                &format!("{}", i + 1),
                px,
                [235, 235, 235],
            );
            // Some games label cells inside the map too.
            draw_text(img, gx + 2, gy + 2, &format!("{col}{}", i + 1), px, color);
        }
    }
}

fn draw_markers(img: &mut RgbaImage, x: i64, y: i64, w: i64, h: i64, rng: &mut Rng, scale: f64) {
    let count = rng.int(8, 25);
    let colors = [
        [220, 40, 40],
        [40, 90, 230],
        [40, 210, 230],
        [240, 200, 40],
        [60, 200, 60],
        [250, 140, 30],
        [255, 255, 255],
    ];
    for _ in 0..count {
        let cx = x as f64 + rng.range(0.03, 0.97) * w as f64;
        let cy = y as f64 + rng.range(0.03, 0.97) * h as f64;
        let r = rng.range(4.0, 9.0) * scale;
        let color = colors[rng.int(0, colors.len() as u32 - 1) as usize];
        match rng.int(0, 3) {
            0 => {
                fill_circle(img, cx, cy, r + 1.5, [0, 0, 0], 1.0);
                fill_circle(img, cx, cy, r, color, 1.0);
            }
            1 => {
                fill_rect(
                    img,
                    (cx - r) as i64 - 1,
                    (cy - r) as i64 - 1,
                    (2.0 * r) as i64 + 2,
                    (2.0 * r) as i64 + 2,
                    [0, 0, 0],
                    1.0,
                );
                fill_rect(
                    img,
                    (cx - r) as i64,
                    (cy - r) as i64,
                    (2.0 * r) as i64,
                    (2.0 * r) as i64,
                    color,
                    1.0,
                );
            }
            2 => fill_triangle(img, cx, cy, r, color),
            _ => {
                // Capture-point style label: yellow box with a dark letter.
                let side = (2.2 * r) as i64;
                fill_rect(img, cx as i64, cy as i64, side, side, [240, 190, 40], 1.0);
                let letter = ['A', 'B', 'C', 'D', 'E', 'F'][rng.int(0, 5) as usize].to_string();
                draw_text(
                    img,
                    cx as i64 + side / 4,
                    cy as i64 + side / 6,
                    &letter,
                    (side / 10).max(1),
                    [30, 30, 30],
                );
            }
        }
    }
}

fn photometric(img: &mut RgbaImage, rng: &mut Rng) -> String {
    let gain = rng.range(0.55, 1.1);
    let contrast = rng.range(0.7, 1.2);
    let gamma = rng.range(0.8, 1.4);
    let offset = rng.range(-10.0, 15.0);
    let mut lut = [0u8; 256];
    for (i, v) in lut.iter_mut().enumerate() {
        let x = (i as f64 / 255.0).powf(gamma) * 255.0;
        let y = ((x - 128.0) * contrast + 128.0) * gain + offset;
        *v = y.round().clamp(0.0, 255.0) as u8;
    }
    for p in img.pixels_mut() {
        for c in 0..3 {
            p[c] = lut[p[c] as usize];
        }
    }
    format!("gain {gain:.2} kontrast {contrast:.2} gamma {gamma:.2}")
}

fn degrade(img: RgbaImage, rng: &mut Rng) -> (RgbaImage, String) {
    let sigma = if rng.unit() < 0.5 {
        rng.range(0.4, 1.3)
    } else {
        0.0
    };
    let img = if sigma > 0.0 {
        imageops::blur(&img, sigma as f32)
    } else {
        img
    };
    let quality = rng.int(55, 90) as u8;
    let rgb = image::DynamicImage::ImageRgba8(img).to_rgb8();
    let mut bytes = Vec::new();
    JpegEncoder::new_with_quality(&mut bytes, quality)
        .encode_image(&rgb)
        .expect("jpeg encode");
    let decoded = image::load_from_memory(&bytes)
        .expect("jpeg decode")
        .to_rgba8();
    (decoded, format!("blur σ {sigma:.2} jpeg q{quality}"))
}

/// Draws reference rect (rx,ry,rw,rh) scaled to dst (x,y,dw,dh) and returns the ground truth.
fn place_map(
    frame: &mut RgbaImage,
    map: &RgbaImage,
    src: (u32, u32, u32, u32),
    dst: (i64, i64, u32, u32),
) -> Truth {
    let crop = imageops::crop_imm(map, src.0, src.1, src.2, src.3).to_image();
    let filter = if dst.2 < src.2 {
        FilterType::Triangle
    } else {
        FilterType::CatmullRom
    };
    let resized = imageops::resize(&crop, dst.2, dst.3, filter);
    imageops::overlay(frame, &resized, dst.0, dst.1);
    Truth {
        screen: (dst.0 as f64, dst.1 as f64, dst.2 as f64, dst.3 as f64),
        ref_origin: (src.0 as f64, src.1 as f64),
        scale: src.2 as f64 / dst.2 as f64,
    }
}

fn make_scenario(kind: Kind, map_index: usize, map: &RgbaImage, rng: &mut Rng) -> Scenario {
    let (w, h) = match kind {
        Kind::Full1366 => (1366, 768),
        _ => (1920, 1080),
    };
    let mut description = String::new();
    let (mut frame, truth) = match kind {
        Kind::Full1920 | Kind::Full1366 | Kind::Holdout => {
            let mut frame = dark_background(w, h, rng);
            let max = (h as f64 * 0.92).min(1400.0);
            let min = if w == 1366 { 600.0 } else { 600.0f64.min(max) };
            let size = rng.range(min, max).round() as u32;
            let x = rng.int(20, w - size - 20) as i64;
            let y = rng.int(((h - size) / 2).min(60), h - size - 10) as i64;
            description.push_str(&format!("mapa {size}px @({x},{y}) "));
            let truth = place_map(
                &mut frame,
                map,
                (0, 0, map.width(), map.height()),
                (x, y, size, size),
            );
            draw_grid(&mut frame, x, y, size as i64, size as i64, rng);
            draw_markers(
                &mut frame,
                x,
                y,
                size as i64,
                size as i64,
                rng,
                w as f64 / 1920.0 * 1.4,
            );
            (frame, Some(truth))
        }
        Kind::Zoom => {
            let mut frame = dark_background(w, h, rng);
            let zoom = rng.range(1.5, 4.0);
            let view = rng.int(700, 1000);
            let src_side = ((map.width().min(map.height()) as f64) / zoom).round() as u32;
            let rx = rng.int(0, map.width() - src_side);
            let ry = rng.int(0, map.height() - src_side);
            let x = rng.int(20, w - view - 20) as i64;
            let y = rng.int(20, h - view - 20) as i64;
            description.push_str(&format!(
                "zoom {zoom:.2} okno {view}px src ({rx},{ry},{src_side}) "
            ));
            let truth = place_map(
                &mut frame,
                map,
                (rx, ry, src_side, src_side),
                (x, y, view, view),
            );
            draw_grid(&mut frame, x, y, view as i64, view as i64, rng);
            draw_markers(&mut frame, x, y, view as i64, view as i64, rng, 1.4);
            (frame, Some(truth))
        }
        Kind::Minimap => {
            let mut frame = scene_background(w, h, rng);
            let size = rng.int(200, 300);
            let x = rng.int(5, 30) as i64;
            let y = (h - size) as i64 - rng.int(60, 120) as i64;
            description.push_str(&format!("minimapa {size}px "));
            fill_rect(
                &mut frame,
                x - 4,
                y - 24,
                size as i64 + 8,
                size as i64 + 28,
                [20, 20, 25],
                0.85,
            );
            let truth = place_map(
                &mut frame,
                map,
                (0, 0, map.width(), map.height()),
                (x, y, size, size),
            );
            draw_grid(&mut frame, x, y, size as i64, size as i64, rng);
            draw_markers(&mut frame, x, y, size as i64, size as i64, rng, 0.6);
            (frame, Some(truth))
        }
        Kind::NoMap => {
            let frame = if rng.unit() < 0.5 {
                scene_background(w, h, rng)
            } else {
                let mut f = dark_background(w, h, rng);
                // An empty map-like panel with a grid but no texture.
                draw_grid(&mut f, 500, 100, 880, 880, rng);
                draw_markers(&mut f, 500, 100, 880, 880, rng, 1.4);
                f
            };
            (frame, None)
        }
    };
    draw_hud(&mut frame, rng);
    description.push_str(&photometric(&mut frame, rng));
    let (frame, degraded) = degrade(frame, rng);
    description.push(' ');
    description.push_str(&degraded);
    Scenario {
        kind,
        map: map_index,
        frame,
        truth,
        description,
    }
}

fn metadata(id: &str) -> MapReferenceMetadata {
    MapReferenceMetadata {
        schema_version: 1,
        id: id.into(),
        name: id.into(),
        unit: "game_units".into(),
        source: "Lokalny cache benchmarku (poza repozytorium)".into(),
        confirmed: true,
        calibration: MapCalibration {
            units_per_pixel: 1.0,
            north_rotation_deg: 0.0,
            origin_pixel: Point { x: 0.0, y: 0.0 },
            origin_world: Point { x: 0.0, y: 0.0 },
            uncertainty_units: 1.0,
        },
    }
}

#[derive(Default, Clone)]
struct Tally {
    total: usize,
    correct: usize,
    inaccurate: usize,
    false_match: usize,
    no_match: usize,
    ambiguous: usize,
    timed_out: usize,
    times: Vec<f64>,
    errors: Vec<f64>,
}
impl Tally {
    fn percentile(values: &[f64], p: f64) -> f64 {
        if values.is_empty() {
            return f64::NAN;
        }
        let mut v = values.to_vec();
        v.sort_by(|a, b| a.total_cmp(b));
        v[((v.len() - 1) as f64 * p).round() as usize]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Mode {
    Catalog,
    Preferred,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    let value = |name: &str| args.windows(2).find(|a| a[0] == name).map(|a| a[1].clone());
    let dir = args
        .iter()
        .find(|a| !a.starts_with("--") && Path::new(a).is_dir())
        .cloned()
        .or_else(|| env::var("MTC_MAP_BENCH_DIR").ok())
        .ok_or("Podaj katalog z teksturami <id>.png lub ustaw MTC_MAP_BENCH_DIR")?;
    let quick = args.iter().any(|a| a == "--quick");
    let seed: u64 = value("--seed")
        .map(|s| s.parse())
        .transpose()?
        .unwrap_or(2026);
    let budget: u64 = value("--budget")
        .map(|s| s.parse())
        .transpose()?
        .unwrap_or(500);
    let filter: Option<Vec<String>> =
        value("--maps").map(|s| s.split(',').map(str::to_owned).collect());
    let dump = value("--dump").map(PathBuf::from);
    let csv_path = value("--csv").map(PathBuf::from);

    let mut paths: Vec<_> = fs::read_dir(&dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "png"))
        .collect();
    paths.sort();
    let mut ids = Vec::new();
    let mut images = Vec::new();
    let mut references = Vec::new();
    let mut registration_ms = Vec::new();
    for path in &paths {
        let id = path.file_stem().unwrap().to_string_lossy().to_string();
        if filter.as_ref().is_some_and(|f| !f.contains(&id)) {
            continue;
        }
        let image = image::open(path)?.to_rgba8();
        let started = Instant::now();
        let reference = MapReference::new(metadata(&id), &image)?;
        registration_ms.push(started.elapsed().as_secs_f64() * 1000.0);
        ids.push(id);
        images.push(image);
        references.push(reference);
        if quick && ids.len() >= 6 {
            break;
        }
    }
    eprintln!(
        "Załadowano {} map, rejestracja: mediana {:.0} ms, max {:.0} ms",
        ids.len(),
        Tally::percentile(&registration_ms, 0.5),
        Tally::percentile(&registration_ms, 1.0)
    );
    let mut config = MapMatchConfig {
        max_processing_ms: budget,
        ..Default::default()
    };
    if let Some(v) = value("--dim") {
        config.max_image_dimension = v.parse()?;
    }
    if let Some(v) = value("--features") {
        config.max_features_per_level = v.parse()?;
    }
    if let Some(v) = value("--levels") {
        config.pyramid_scales = v.split(',').map(str::parse).collect::<Result<_, _>>()?;
    }
    if let Some(v) = value("--rotations") {
        config.rotations_deg = v.split(',').map(str::parse).collect::<Result<_, _>>()?;
    }
    if let Some(v) = value("--ncc") {
        config.min_verification_correlation = v.parse()?;
    }
    if let Some(v) = value("--min-ratio") {
        config.min_inlier_ratio = v.parse()?;
    }
    if let Some(v) = value("--min-inliers") {
        config.min_inliers = v.parse()?;
    }
    if let Some(v) = value("--hamming") {
        config.max_descriptor_hamming = v.parse()?;
    }
    if let Some(v) = value("--ratio") {
        config.descriptor_ratio = v.parse()?;
    }
    config.validate()?;
    if let Some(frame) = value("--frame") {
        // Real capture: report the decision for the whole catalog, nothing synthetic.
        let frame = image::open(frame)?.to_rgba8();
        for budget_mode in ["katalog", "preferowana"] {
            let catalog: Vec<MapReference> = match (budget_mode, value("--expect")) {
                ("preferowana", Some(id)) => references
                    .iter()
                    .filter(|r| r.metadata.id == id)
                    .cloned()
                    .collect(),
                ("preferowana", None) => continue,
                _ => references.clone(),
            };
            let started = Instant::now();
            let result = match_map_view(&frame, &catalog, &config)?;
            println!(
                "{budget_mode}: {:?} {:?} {:.0} ms",
                result.status,
                result.best.as_ref().map(|b| (
                    &b.map_id,
                    b.inliers,
                    b.verification_correlation,
                    b.transform
                )),
                started.elapsed().as_secs_f64() * 1000.0
            );
            for c in &result.candidates {
                println!(
                    "  kandydat {} inliers {} korelacja {:.2} score {:.2}",
                    c.map_id, c.inliers, c.verification_correlation, c.score
                );
            }
        }
        return Ok(());
    }
    let only: Option<Vec<String>> =
        value("--only").map(|s| s.split(',').map(str::to_owned).collect());
    let focus: Option<Vec<String>> =
        value("--focus").map(|s| s.split(',').map(str::to_owned).collect());
    let modes: Vec<Mode> = match value("--mode").as_deref() {
        Some("catalog") => vec![Mode::Catalog],
        Some("preferred") => vec![Mode::Preferred],
        _ => vec![Mode::Catalog, Mode::Preferred],
    };
    let kinds: &[Kind] = if quick {
        &[Kind::Full1920, Kind::Zoom, Kind::Minimap, Kind::Holdout]
    } else {
        &[
            Kind::Full1920,
            Kind::Full1920,
            Kind::Full1366,
            Kind::Zoom,
            Kind::Zoom,
            Kind::Minimap,
            Kind::Holdout,
        ]
    };
    let mut rng = Rng(seed);
    let mut scenarios = Vec::new();
    for (map_index, image) in images.iter().enumerate() {
        for kind in kinds {
            scenarios.push(make_scenario(*kind, map_index, image, &mut rng));
        }
    }
    let nomap = if quick { 3 } else { 10 };
    for i in 0..nomap {
        scenarios.push(make_scenario(
            Kind::NoMap,
            i % ids.len(),
            &images[0],
            &mut rng,
        ));
    }
    scenarios.retain(|s| {
        only.as_ref().is_none_or(|o| {
            o.iter()
                .any(|k| k.eq_ignore_ascii_case(&format!("{:?}", s.kind)))
        }) && focus
            .as_ref()
            .is_none_or(|f| s.kind == Kind::NoMap || f.contains(&ids[s.map]))
    });
    if let Some(dump) = &dump {
        fs::create_dir_all(dump)?;
        for (i, s) in scenarios
            .iter()
            .enumerate()
            .filter(|(i, _)| i % 5 == 0)
            .take(12)
        {
            s.frame
                .save(dump.join(format!("{i:03}-{}-{:?}.png", ids[s.map], s.kind)))?;
        }
    }

    let mut tallies: BTreeMap<(Mode, Kind), Tally> = BTreeMap::new();
    let mut csv = String::from("mode,kind,map,status,best,error_ref_px,tolerance,ms,description\n");
    for scenario in &scenarios {
        for mode in modes.iter().copied() {
            let catalog: Vec<MapReference> = match (mode, scenario.kind) {
                (Mode::Catalog, Kind::Holdout) => references
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != scenario.map)
                    .map(|(_, r)| r.clone())
                    .collect(),
                (Mode::Catalog, _) => references.clone(),
                (Mode::Preferred, Kind::Holdout | Kind::NoMap) => {
                    vec![references[(scenario.map + 1) % references.len()].clone()]
                }
                (Mode::Preferred, _) => vec![references[scenario.map].clone()],
            };
            if catalog.is_empty() {
                continue;
            }
            let started = Instant::now();
            let result = match_map_view(&scenario.frame, &catalog, &config)?;
            let ms = started.elapsed().as_secs_f64() * 1000.0;
            let tally = tallies.entry((mode, scenario.kind)).or_default();
            tally.total += 1;
            tally.times.push(ms);
            let mut error = f64::NAN;
            let mut tolerance = f64::NAN;
            match result.status {
                MapViewStatus::Matched => {
                    let best = result.best.as_ref().unwrap();
                    let right_map = scenario.kind.positive() && best.map_id == ids[scenario.map];
                    if let (true, Some(truth)) = (right_map, scenario.truth) {
                        let (sx, sy, sw, sh) = truth.screen;
                        let (fw, fh) = (
                            scenario.frame.width() as f64,
                            scenario.frame.height() as f64,
                        );
                        let mut worst: f64 = 0.0;
                        for fy in [0.15, 0.5, 0.85] {
                            for fx in [0.15, 0.5, 0.85] {
                                let q = Point {
                                    x: (sx + fx * sw).clamp(0.0, fw - 1.0),
                                    y: (sy + fy * sh).clamp(0.0, fh - 1.0),
                                };
                                let expected = truth.reference(q);
                                let observed = best.transform.map_point(q)?;
                                worst = worst
                                    .max((observed.x - expected.x).hypot(observed.y - expected.y));
                            }
                        }
                        error = worst;
                        tolerance = (2.0 * truth.scale).max(6.0);
                        if worst <= tolerance {
                            tally.correct += 1;
                            tally.errors.push(worst);
                        } else if worst <= 4.0 * tolerance {
                            // Right map, localisation error a few screen pixels.
                            tally.inaccurate += 1;
                        } else {
                            tally.false_match += 1;
                        }
                    } else {
                        tally.false_match += 1;
                    }
                }
                MapViewStatus::Ambiguous => tally.ambiguous += 1,
                MapViewStatus::TimedOut => tally.timed_out += 1,
                _ => tally.no_match += 1,
            }
            csv.push_str(&format!(
                "{},{:?},{},{:?},{},{:.2},{:.2},{:.1},{}\n",
                if mode == Mode::Catalog {
                    "catalog"
                } else {
                    "preferred"
                },
                scenario.kind,
                ids[scenario.map],
                result.status,
                result
                    .best
                    .as_ref()
                    .map(|b| b.map_id.as_str())
                    .unwrap_or("-"),
                error,
                tolerance,
                ms,
                scenario.description
            ));
        }
    }
    if let Some(path) = csv_path {
        fs::File::create(path)?.write_all(csv.as_bytes())?;
    }
    println!(
        "Mapy: {}, scenariusze: {}, budżet {} ms, profil: {}",
        ids.len(),
        scenarios.len(),
        budget,
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    );
    println!("| Tryb | Scenariusz | N | Poprawne | Niedokładne | Fałszywe | Brak | Niejedn. | Timeout | Med. błąd [px ref] | Med. ms | p95 ms | Max ms |");
    println!("|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|");
    for ((mode, kind), t) in &tallies {
        let positive = kind.positive();
        println!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {:.0} | {:.0} | {:.0} |",
            if *mode == Mode::Catalog {
                format!("katalog {}", ids.len())
            } else {
                "preferowana".into()
            },
            kind.label(),
            t.total,
            if positive {
                format!(
                    "{} ({:.0}%)",
                    t.correct,
                    100.0 * t.correct as f64 / t.total as f64
                )
            } else {
                "—".into()
            },
            t.inaccurate,
            t.false_match,
            t.no_match,
            t.ambiguous,
            t.timed_out,
            if t.errors.is_empty() {
                "—".into()
            } else {
                format!("{:.1}", Tally::percentile(&t.errors, 0.5))
            },
            Tally::percentile(&t.times, 0.5),
            Tally::percentile(&t.times, 0.95),
            Tally::percentile(&t.times, 1.0),
        );
    }
    Ok(())
}
