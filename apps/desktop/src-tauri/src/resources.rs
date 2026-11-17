use crate::{
    automation::{StoredMap, StoredTerrain},
    Runtime,
};
use mtc_assets::{AssetCache, AssetKind, CacheLimits, CachedRaster, KnownMapResource, SyncMode};
use mtc_maps::{MapCalibration, Point};
use serde::Serialize;
use tauri::{Emitter, Manager};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceLibrary {
    pub maps: Vec<StoredMap>,
    pub terrains: Vec<StoredTerrain>,
    pub warnings: Vec<String>,
    pub report: mtc_assets::SyncReport,
}

pub(crate) fn cache_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let args: Vec<_> = std::env::args().collect();
    if let Some(index) = args.iter().position(|s| s == "--test-data-dir") {
        return Ok(
            std::path::PathBuf::from(args.get(index + 1).ok_or("Brak katalogu testowego")?)
                .join("community-assets-v1"),
        );
    }
    // Same explicit local cache used by the setup CLI. Assets stay outside releases.
    Ok(app
        .path()
        .local_data_dir()
        .map_err(|e| e.to_string())?
        .join("MTC-Tactical-Assistant/community-assets-v1"))
}

/// Runs `f` with the lazily created shared cache (one mutex for UI and CLI-like calls).
pub(crate) fn with_cache<T>(
    app: &tauri::AppHandle,
    f: impl FnOnce(&AssetCache) -> Result<T, String>,
) -> Result<T, String> {
    let state = app.state::<Runtime>();
    let mut stored = state.resource_cache.lock().map_err(|e| e.to_string())?;
    if stored.is_none() {
        *stored = Some(
            AssetCache::new(cache_path(app)?, CacheLimits::default())
                .map_err(|e| format!("Nie można utworzyć lokalnego cache map: {e}"))?,
        );
    }
    f(stored.as_ref().ok_or("Brak cache zasobów")?)
}

/// Library entry of a complete community texture. North is up and the source
/// size covers the whole square raster (normalized XY of the source calculator).
pub(crate) fn stored_map_from(
    known: &KnownMapResource,
    raster: CachedRaster,
    source_acknowledged: bool,
) -> Result<StoredMap, String> {
    let scale =
        mtc_maps::full_square_map_scale(known.size_game_meters, raster.width, raster.height)
            .map_err(|e| format!("{}: {e}", known.name))?;
    Ok(StoredMap {
        sha256: raster.sha256,
        id: known.id.clone(),
        name: known.name.clone(),
        path: raster.path,
        width: raster.width,
        height: raster.height,
        unit: known.unit.clone(),
        source: format!(
            "Community {}; {}; source axes north-up; experimental",
            mtc_assets::SOURCE_COMMIT,
            raster.url
        ),
        confirmed: source_acknowledged,
        calibration: MapCalibration {
            units_per_pixel: scale,
            north_rotation_deg: 0.0,
            origin_pixel: Point { x: 0.0, y: 0.0 },
            origin_world: Point { x: 0.0, y: 0.0 },
            uncertainty_units: 0.0,
        },
    })
}

/// Terrain entry bound to the same full map extent (shared normalized XY).
pub(crate) fn stored_terrain_from(
    known: &KnownMapResource,
    map: &StoredMap,
    prepared: CachedRaster,
    source_acknowledged: bool,
) -> Result<StoredTerrain, String> {
    let range = known
        .height_range_game_meters
        .as_ref()
        .ok_or_else(|| format!("{} — źródło nie podaje mapy wysokości", known.name))?;
    Ok(StoredTerrain {
        sha256: prepared.sha256.clone(),
        path: prepared.path,
        map_id: known.id.clone(),
        map_width: map.width,
        map_height: map.height,
        min_height: range.minimum,
        max_height: range.maximum,
        unit: known.unit.clone(),
        source: format!(
            "Community {}; RED8 normalizedXY; {}; source {}; parentSHA {}; preparedSHA {}",
            mtc_assets::SOURCE_COMMIT,
            prepared
                .processing
                .as_deref()
                .unwrap_or("processing unknown"),
            prepared.url,
            prepared.source_sha256.unwrap_or_default(),
            prepared.sha256
        ),
        confirmed: source_acknowledged,
        coordinate_alignment_confirmed: source_acknowledged,
    })
}

#[tauri::command]
pub fn resource_catalog() -> Result<Vec<mtc_assets::KnownMapResource>, String> {
    mtc_assets::known_maps().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sync_resources(
    app: tauri::AppHandle,
    ids: Vec<String>,
    include_terrain: bool,
    online: bool,
    source_acknowledged: bool,
) -> Result<ResourceLibrary, String> {
    tauri::async_runtime::spawn_blocking(move || {
        with_cache(&app, |cache| {
            let mut completed = 0;
            let report = cache
                .sync_with_progress(
                    &ids,
                    include_terrain,
                    source_acknowledged,
                    if online {
                        SyncMode::Online
                    } else {
                        SyncMode::Offline
                    },
                    |item| {
                        completed += 1;
                        let _ = app.emit(
                            "map-resource-progress",
                            serde_json::json!({"completed":completed,"id":item.id,"kind":item.kind,"status":item.status}),
                        );
                    },
                )
                .map_err(|e| e.to_string())?;
            let catalog = mtc_assets::known_maps().map_err(|e| e.to_string())?;
            let mut maps = Vec::new();
            let mut terrains = Vec::new();
            let mut warnings = Vec::new();
            for id in &ids {
                let known = catalog
                    .iter()
                    .find(|m| &m.id == id)
                    .ok_or("Nieznana mapa")?;
                for failure in report
                    .results
                    .iter()
                    .filter(|r| &r.id == id && r.error.is_some())
                {
                    if let Some(error) = &failure.error {
                        warnings.push(format!("{} — {:?}: {}", known.name, failure.kind, error.message));
                    }
                }
                let success = |kind| {
                    report.results.iter().any(|r| {
                        &r.id == id && r.kind == kind && r.error.is_none() && r.cached.is_some()
                    })
                };
                if !success(AssetKind::MapImage) {
                    continue;
                }
                let map = match cache
                    .validate_cached(id, AssetKind::MapImage)
                    .map_err(|e| e.to_string())
                    .and_then(|raster| stored_map_from(known, raster, source_acknowledged))
                {
                    Ok(map) => map,
                    Err(error) => {
                        warnings.push(format!("{}: {error}", known.name));
                        continue;
                    }
                };
                if include_terrain && known.height_range_game_meters.is_none() {
                    warnings.push(format!("{} — źródło nie podaje mapy wysokości", known.name));
                }
                if include_terrain
                    && known.height_range_game_meters.is_some()
                    && success(AssetKind::Heightmap)
                {
                    match cache
                        .prepare_heightmap(id, 1024)
                        .map_err(|e| e.to_string())
                        .and_then(|height| {
                            stored_terrain_from(known, &map, height, source_acknowledged)
                        }) {
                        Ok(terrain) => terrains.push(terrain),
                        Err(error) => warnings.push(format!("{} — teren: {error}", known.name)),
                    }
                }
                maps.push(map);
            }
            Ok(ResourceLibrary {
                maps,
                terrains,
                warnings,
                report,
            })
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn known(id: &str) -> KnownMapResource {
        mtc_assets::known_maps()
            .unwrap()
            .into_iter()
            .find(|m| m.id == id)
            .unwrap()
    }
    pub(crate) fn raster(kind: AssetKind, width: u32, height: u32) -> CachedRaster {
        let resource = known("normandy");
        CachedRaster {
            schema_version: 1,
            id: "normandy".into(),
            kind,
            path: "C:/synthetic/blob.png".into(),
            sha256: "a".repeat(64),
            width,
            height,
            byte_length: 10,
            downloaded_at_ms: 0,
            url: resource.map_url.clone(),
            status: "experimental".into(),
            source_metadata: resource.source_metadata.clone(),
            source_sha256: Some("b".repeat(64)),
            processing: None,
        }
    }

    #[test]
    fn catalog_scale_is_full_size_over_texture_width_with_north_up() {
        let known = known("normandy");
        let map = stored_map_from(&known, raster(AssetKind::MapImage, 1024, 1024), true).unwrap();
        assert!((map.calibration.units_per_pixel - known.size_game_meters / 1024.0).abs() < 1e-12);
        assert_eq!(map.calibration.north_rotation_deg, 0.0);
        assert_eq!(map.unit, "game_meters");
        assert!(map.confirmed);
        assert!(map.source.contains(mtc_assets::SOURCE_COMMIT));
    }

    #[test]
    fn non_square_texture_is_rejected_instead_of_distorting_one_axis() {
        assert!(stored_map_from(
            &known("normandy"),
            raster(AssetKind::MapImage, 1024, 900),
            true
        )
        .unwrap_err()
        .contains("kwadratowy"));
    }

    #[test]
    fn terrain_is_bound_to_map_extent_and_source_range_in_game_meters() {
        let known = known("normandy");
        let map = stored_map_from(&known, raster(AssetKind::MapImage, 1024, 1024), true).unwrap();
        let terrain = stored_terrain_from(
            &known,
            &map,
            raster(AssetKind::PreparedHeightmap, 512, 512),
            true,
        )
        .unwrap();
        assert_eq!((terrain.map_width, terrain.map_height), (1024, 1024));
        let range = known.height_range_game_meters.unwrap();
        assert_eq!(
            (terrain.min_height, terrain.max_height),
            (range.minimum, range.maximum)
        );
        assert!(terrain.confirmed && terrain.coordinate_alignment_confirmed);
        assert!(terrain.source.contains("RED8"));
        // Gensokyo has no source heightmap: no fabricated terrain.
        let gensokyo = super::tests::known("gensokyo");
        assert!(stored_terrain_from(
            &gensokyo,
            &map,
            raster(AssetKind::PreparedHeightmap, 512, 512),
            true
        )
        .is_err());
    }
}
