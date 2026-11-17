use crate::Roi;
use image::RgbaImage;
use serde::{Deserialize, Serialize};

const GRID: usize = 16;
const MIN_GRAY_RMS: f64 = 8.0;
const MIN_GRADIENT_RMS: f64 = 3.0;
const MIN_ACTIVE_GRADIENT_FRACTION: f64 = 0.1;

/// A coarse, local-only brightness descriptor of a view chosen by the user.
/// It is neither a screenshot nor a semantic identification of an MTC map.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MapViewSignature {
    pub schema_version: u32,
    pub grid_size: u32,
    pub gray_samples: Vec<u8>,
    pub texture_rms: f64,
    pub aspect_ratio: f64,
    pub threshold: f64,
}

impl MapViewSignature {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1
            || self.grid_size != GRID as u32
            || self.gray_samples.len() != GRID * GRID
            || !self.texture_rms.is_finite()
            || self.texture_rms < MIN_GRAY_RMS
            || !self.aspect_ratio.is_finite()
            || self.aspect_ratio <= 0.0
            || !self.threshold.is_finite()
            || !(0.85..=1.0).contains(&self.threshold)
        {
            return Err("Niepoprawny deskryptor referencyjnego widoku mapy".into());
        }
        validate_texture(&self.gray_samples)?;
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MapViewMatch {
    pub matched: bool,
    /// Conservative minimum of brightness and gradient correlation, not probability.
    pub correlation: f64,
    pub texture_rms: f64,
    pub warning: Option<String>,
}

pub fn calibrate_view_signature(
    image: &RgbaImage,
    map_roi: Roi,
) -> Result<MapViewSignature, String> {
    let map = map_roi.crop(image)?;
    let samples = describe(&map)?;
    let texture_rms = validate_texture(&samples)?;
    Ok(MapViewSignature {
        schema_version: 1,
        grid_size: GRID as u32,
        gray_samples: samples,
        texture_rms,
        aspect_ratio: f64::from(map.width()) / f64::from(map.height()),
        threshold: 0.85,
    })
}

/// Match the explicitly calibrated complete view. Median cell sampling and a
/// small robust trim suppress local icons; centered correlation tolerates a
/// global brightness offset or gain. A changed layout, zoom or vehicle view
/// should be recalibrated; the score does not prove a map name or orientation.
pub fn match_view_signature(
    image: &RgbaImage,
    map_roi: Roi,
    signature: &MapViewSignature,
) -> Result<MapViewMatch, String> {
    let map = map_roi.crop(image)?;
    match_view_signature_on_crop(&map, signature)
}

/// Same as `match_view_signature` for an already cropped map view.
pub(crate) fn match_view_signature_on_crop(
    map: &RgbaImage,
    signature: &MapViewSignature,
) -> Result<MapViewMatch, String> {
    signature.validate()?;
    let aspect = f64::from(map.width()) / f64::from(map.height());
    if (aspect / signature.aspect_ratio - 1.0).abs() > 0.03 {
        return Ok(MapViewMatch {
            matched: false,
            correlation: 0.0,
            texture_rms: 0.0,
            warning: Some(
                "Proporcje obrazu zmieniły się względem zapamiętanego widoku mapy".into(),
            ),
        });
    }
    let samples = match describe(map) {
        Ok(samples) => samples,
        Err(warning) => {
            return Ok(MapViewMatch {
                matched: false,
                correlation: 0.0,
                texture_rms: 0.0,
                warning: Some(warning),
            });
        }
    };
    let texture_rms = match validate_texture(&samples) {
        Ok(texture_rms) => texture_rms,
        Err(warning) => {
            return Ok(MapViewMatch {
                matched: false,
                correlation: 0.0,
                texture_rms: rms(&samples.iter().map(|v| f64::from(*v)).collect::<Vec<_>>()),
                warning: Some(warning),
            });
        }
    };
    let reference: Vec<f64> = signature
        .gray_samples
        .iter()
        .map(|v| f64::from(*v))
        .collect();
    let observed: Vec<f64> = samples.iter().map(|v| f64::from(*v)).collect();
    let gray_correlation = robust_correlation(&reference, &observed);
    let gradient_correlation =
        robust_correlation(&gradients(&signature.gray_samples), &gradients(&samples));
    let correlation = gray_correlation.min(gradient_correlation).clamp(-1.0, 1.0);
    let matched = correlation >= signature.threshold;
    Ok(MapViewMatch {
        matched,
        correlation,
        texture_rms,
        warning: if matched {
            None
        } else {
            Some(format!(
                "Bieżący obraz nie pasuje do wybranego widoku mapy (korelacja {:.2}, próg {:.2})",
                correlation, signature.threshold
            ))
        },
    })
}

fn describe(map: &RgbaImage) -> Result<Vec<u8>, String> {
    if map.width() < (GRID * 3) as u32 || map.height() < (GRID * 3) as u32 {
        return Err(
            "Obszar mapy jest za mały do deskryptora; wymagane co najmniej 48 × 48 px".into(),
        );
    }
    let mut samples = Vec::with_capacity(GRID * GRID);
    for row in 0..GRID {
        for column in 0..GRID {
            let left = column as u32 * map.width() / GRID as u32;
            let top = row as u32 * map.height() / GRID as u32;
            let right = (column + 1) as u32 * map.width() / GRID as u32;
            let bottom = (row + 1) as u32 * map.height() / GRID as u32;
            let mut cell = Vec::with_capacity(49);
            for sy in 0..7u32 {
                for sx in 0..7u32 {
                    let x = left + (2 * sx + 1) * (right - left) / 14;
                    let y = top + (2 * sy + 1) * (bottom - top) / 14;
                    let pixel = map.get_pixel(x.min(right - 1), y.min(bottom - 1));
                    // Rec.601 integer luminance, no color or object identity stored.
                    cell.push(
                        ((77 * u32::from(pixel[0])
                            + 150 * u32::from(pixel[1])
                            + 29 * u32::from(pixel[2]))
                            / 256) as u8,
                    );
                }
            }
            cell.sort_unstable();
            samples.push(cell[cell.len() / 2]);
        }
    }
    Ok(samples)
}

fn gradients(samples: &[u8]) -> Vec<f64> {
    let mut values = Vec::with_capacity(2 * GRID * (GRID - 1));
    for row in 0..GRID {
        for column in 0..GRID - 1 {
            values.push(
                f64::from(samples[row * GRID + column + 1])
                    - f64::from(samples[row * GRID + column]),
            );
        }
    }
    for row in 0..GRID - 1 {
        for column in 0..GRID {
            values.push(
                f64::from(samples[(row + 1) * GRID + column])
                    - f64::from(samples[row * GRID + column]),
            );
        }
    }
    values
}

fn rms(values: &[f64]) -> f64 {
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    (values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64).sqrt()
}

fn validate_texture(samples: &[u8]) -> Result<f64, String> {
    let gray: Vec<_> = samples.iter().map(|v| f64::from(*v)).collect();
    let texture_rms = rms(&gray);
    let gradient = gradients(samples);
    let active_fraction =
        gradient.iter().filter(|v| v.abs() >= 4.0).count() as f64 / gradient.len() as f64;
    // Evaluate each axis separately so a flat linear brightness gradient is
    // rejected even though its horizontal and vertical slopes differ.
    let half = gradient.len() / 2;
    let gradient_rms = rms(&gradient[..half]).hypot(rms(&gradient[half..]));
    if texture_rms < MIN_GRAY_RMS
        || gradient_rms < MIN_GRADIENT_RMS
        || active_fraction < MIN_ACTIVE_GRADIENT_FRACTION
    {
        return Err("Za mało rozróżnialnej tekstury mapy; użyj Manual lub wybierz bardziej szczegółowy pełny widok".into());
    }
    Ok(texture_rms)
}

fn robust_correlation(reference: &[f64], observed: &[f64]) -> f64 {
    let mean_a = reference.iter().sum::<f64>() / reference.len() as f64;
    let mean_b = observed.iter().sum::<f64>() / observed.len() as f64;
    let scale_a = rms(reference);
    let scale_b = rms(observed);
    if scale_a <= 1e-9 || scale_b <= 1e-9 {
        return 0.0;
    }
    let mut indices: Vec<usize> = (0..reference.len()).collect();
    indices.sort_unstable_by(|a, b| {
        let residual = |index: usize| {
            ((reference[index] - mean_a) / scale_a - (observed[index] - mean_b) / scale_b).abs()
        };
        residual(*a).total_cmp(&residual(*b))
    });
    indices.truncate(reference.len() - reference.len() / 20);
    let mean_a = indices.iter().map(|i| reference[*i]).sum::<f64>() / indices.len() as f64;
    let mean_b = indices.iter().map(|i| observed[*i]).sum::<f64>() / indices.len() as f64;
    let covariance = indices
        .iter()
        .map(|i| (reference[*i] - mean_a) * (observed[*i] - mean_b))
        .sum::<f64>();
    let norm_a = indices
        .iter()
        .map(|i| (reference[*i] - mean_a).powi(2))
        .sum::<f64>()
        .sqrt();
    let norm_b = indices
        .iter()
        .map(|i| (observed[*i] - mean_b).powi(2))
        .sum::<f64>()
        .sqrt();
    if norm_a <= 1e-9 || norm_b <= 1e-9 {
        0.0
    } else {
        covariance / (norm_a * norm_b)
    }
}
