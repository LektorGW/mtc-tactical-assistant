//! One-click preparation of a single community catalog map: local cache first,
//! then (only when allowed) the exact pinned URL. No game access.
use crate::{
    automation::{StoredMap, StoredTerrain},
    resources::{stored_map_from, stored_terrain_from, with_cache},
};
use mtc_assets::{
    AssetCache, AssetKind, AssetSyncResult, CachedRaster, KnownMapResource, SyncMode,
};
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogMapResult {
    pub map: StoredMap,
    /// `None` when the source has no heightmap or it could not be prepared (see warnings).
    pub terrain: Option<StoredTerrain>,
    pub terrain_in_source: bool,
    /// `cache` — everything came from the verified local cache; `downloaded` — at least one file was fetched.
    pub origin: &'static str,
    pub size_game_meters: f64,
    pub warnings: Vec<String>,
}

/// Contract: `id` must be a catalog ID; `sourceAcknowledged` must be true.
/// With `allowOnline: false` only the local cache is read. Errors are plain
/// Polish messages that mention the manual import fallback.
#[tauri::command]
pub async fn prepare_catalog_map(
    app: tauri::AppHandle,
    id: String,
    include_terrain: bool,
    allow_online: bool,
    source_acknowledged: bool,
) -> Result<CatalogMapResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if !source_acknowledged {
            return Err("Potwierdź użycie eksperymentalnego źródła map społeczności".into());
        }
        let known = mtc_assets::known_maps()
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|m| m.id == id)
            .ok_or("Nieznana mapa katalogu")?;
        with_cache(&app, |cache| {
            prepare(
                cache,
                &known,
                include_terrain,
                allow_online,
                &mut |cache, mode| {
                    cache
                        .sync(std::slice::from_ref(&known.id), include_terrain, true, mode)
                        .map(|report| report.results)
                        .map_err(|e| e.to_string())
                },
            )
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

type SyncFn<'a> = dyn FnMut(&AssetCache, SyncMode) -> Result<Vec<AssetSyncResult>, String> + 'a;

/// Picks per kind the first successful result: verified cache, then download.
fn prepare(
    cache: &AssetCache,
    known: &KnownMapResource,
    include_terrain: bool,
    allow_online: bool,
    sync: &mut SyncFn<'_>,
) -> Result<CatalogMapResult, String> {
    let wants_terrain = include_terrain && known.height_range_game_meters.is_some();
    let offline = sync(cache, SyncMode::Offline)?;
    let ok = |results: &[AssetSyncResult], kind| -> Option<CachedRaster> {
        results
            .iter()
            .find(|r| r.kind == kind && r.error.is_none())
            .and_then(|r| r.cached.clone())
    };
    let message = |results: &[AssetSyncResult], kind| -> Option<String> {
        results
            .iter()
            .find(|r| r.kind == kind)
            .and_then(|r| r.error.as_ref())
            .map(|e| e.message.clone())
    };
    let mut map_raster = ok(&offline, AssetKind::MapImage);
    let mut height_ok = ok(&offline, AssetKind::Heightmap).is_some();
    let mut origin = "cache";
    let mut warnings = Vec::new();
    let mut online_error = None;
    if (map_raster.is_none() || (wants_terrain && !height_ok)) && allow_online {
        match sync(cache, SyncMode::Online) {
            Ok(online) => {
                if let Some(raster) = ok(&online, AssetKind::MapImage) {
                    map_raster = Some(raster);
                    origin = "downloaded";
                }
                if ok(&online, AssetKind::Heightmap).is_some() {
                    height_ok = true;
                    origin = "downloaded";
                }
                online_error = message(&online, AssetKind::MapImage)
                    .or_else(|| message(&online, AssetKind::Heightmap));
            }
            Err(e) => online_error = Some(e),
        }
    }
    let Some(map_raster) = map_raster else {
        return Err(match (allow_online, online_error) {
            (false, _) => format!(
                "Mapy {} nie ma w lokalnym cache. Zezwól na pobranie albo zaimportuj obraz ręcznie (tryb zaawansowany).",
                known.name
            ),
            (true, Some(e)) => format!(
                "Nie udało się pobrać mapy {} ({e}). Sprawdź połączenie z internetem albo zaimportuj obraz ręcznie i skalibruj dwa punkty (tryb zaawansowany).",
                known.name
            ),
            (true, None) => format!(
                "Nie udało się przygotować mapy {}. Zaimportuj obraz ręcznie (tryb zaawansowany).",
                known.name
            ),
        });
    };
    let map = stored_map_from(known, map_raster, true)?;
    let mut terrain = None;
    if include_terrain && !wants_terrain {
        warnings.push(format!(
            "{} — źródło nie podaje mapy wysokości; różnicę wysokości wpisz ręcznie",
            known.name
        ));
    } else if wants_terrain && !height_ok {
        warnings.push(format!(
            "{} — mapa wysokości niedostępna{}; różnicę wysokości wpisz ręcznie",
            known.name,
            online_error
                .as_ref()
                .or(message(&offline, AssetKind::Heightmap).as_ref())
                .map(|e| format!(" ({e})"))
                .unwrap_or_default()
        ));
    } else if wants_terrain {
        match cache
            .prepare_heightmap(&known.id, 1024)
            .map_err(|e| e.to_string())
            .and_then(|height| stored_terrain_from(known, &map, height, true))
        {
            Ok(t) => terrain = Some(t),
            Err(e) => warnings.push(format!("{} — teren: {e}", known.name)),
        }
    }
    Ok(CatalogMapResult {
        map,
        terrain,
        terrain_in_source: known.height_range_game_meters.is_some(),
        origin,
        size_game_meters: known.size_game_meters,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::tests::{known, raster};

    fn cache() -> (AssetCache, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "mtc-catalog-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        (
            AssetCache::new(&root, mtc_assets::CacheLimits::default()).unwrap(),
            root,
        )
    }
    fn result(kind: AssetKind, ok: bool, message: &str) -> AssetSyncResult {
        AssetSyncResult {
            id: "normandy".into(),
            kind,
            status: if ok {
                mtc_assets::SyncStatus::Cached
            } else {
                mtc_assets::SyncStatus::Missing
            },
            cached: ok.then(|| raster(kind, 1024, 1024)),
            changed: false,
            error: (!ok).then(|| mtc_assets::AssetError {
                code: mtc_assets::ErrorCode::Missing,
                message: message.into(),
            }),
        }
    }

    #[test]
    fn offline_cache_hit_never_starts_a_download() {
        let (cache, root) = cache();
        let mut modes = Vec::new();
        let out = prepare(&cache, &known("normandy"), false, true, &mut |_, mode| {
            modes.push(format!("{mode:?}"));
            Ok(vec![result(AssetKind::MapImage, true, "")])
        })
        .unwrap();
        assert_eq!(modes, ["Offline"]);
        assert_eq!(out.origin, "cache");
        assert!(
            (out.map.calibration.units_per_pixel - out.size_game_meters / 1024.0).abs() < 1e-12
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn missing_cache_downloads_only_when_allowed_and_reports_offline_failure() {
        let (cache, root) = cache();
        let missing = || {
            vec![result(
                AssetKind::MapImage,
                false,
                "Zasób nie jest w lokalnym cache",
            )]
        };
        let error = prepare(&cache, &known("normandy"), false, false, &mut |_, _| {
            Ok(missing())
        })
        .unwrap_err();
        assert!(error.contains("ręcznie"));
        let mut calls = 0;
        let error = prepare(&cache, &known("normandy"), false, true, &mut |_, mode| {
            calls += 1;
            match mode {
                SyncMode::Offline => Ok(missing()),
                SyncMode::Online => Err("Nie pobrano normandy: dns error".into()),
            }
        })
        .unwrap_err();
        assert_eq!(calls, 2);
        assert!(error.contains("internetem") && error.contains("dns error"));
        let downloaded = prepare(&cache, &known("normandy"), false, true, &mut |_, mode| {
            Ok(match mode {
                SyncMode::Offline => missing(),
                SyncMode::Online => vec![result(AssetKind::MapImage, true, "")],
            })
        })
        .unwrap();
        assert_eq!(downloaded.origin, "downloaded");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn cached_map_survives_failed_terrain_download_with_explicit_warning() {
        let (cache, root) = cache();
        let out = prepare(&cache, &known("normandy"), true, true, &mut |_, mode| {
            Ok(match mode {
                SyncMode::Offline => vec![
                    result(AssetKind::MapImage, true, ""),
                    result(AssetKind::Heightmap, false, "brak"),
                ],
                SyncMode::Online => vec![
                    result(AssetKind::MapImage, false, "offline"),
                    result(AssetKind::Heightmap, false, "offline"),
                ],
            })
        })
        .unwrap();
        assert!(out.terrain.is_none());
        assert_eq!(out.origin, "cache");
        assert!(out.warnings[0].contains("wysokości"));
        // A source without terrain never receives a fabricated heightmap.
        let gensokyo = prepare(&cache, &known("gensokyo"), true, false, &mut |_, _| {
            Ok(vec![result(AssetKind::MapImage, true, "")])
        })
        .unwrap();
        assert!(gensokyo.terrain.is_none() && !gensokyo.terrain_in_source);
        let _ = std::fs::remove_dir_all(root);
    }
}
