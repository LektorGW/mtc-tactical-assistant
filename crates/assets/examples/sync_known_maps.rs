use mtc_assets::{known_maps, AssetCache, AssetKind, CacheLimits, SyncMode};
use std::{env, fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    let value = |name: &str| args.windows(2).find(|a| a[0] == name).map(|a| a[1].clone());
    let resources = known_maps()?;
    let ids: Vec<String> = if args.iter().any(|a| a == "--all") {
        resources.iter().map(|m| m.id.clone()).collect()
    } else {
        vec![value("--map").ok_or("Pass --map <known-id> or --all")?]
    };
    let cache_path = value("--cache").map(PathBuf::from).unwrap_or_else(|| {
        env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(env::temp_dir)
            .join("MTC-Tactical-Assistant/community-assets-v1")
    });
    let cache = AssetCache::new(cache_path, CacheLimits::default())?;
    let mode = if args.iter().any(|a| a == "--offline") {
        SyncMode::Offline
    } else {
        SyncMode::Online
    };
    let sync = cache.sync_with_progress(
        &ids,
        args.iter().any(|a| a == "--terrain"),
        args.iter().any(|a| a == "--ack"),
        mode,
        |result| {
            eprintln!(
                "{} {:?}: {:?} {}",
                result.id,
                result.kind,
                result.status,
                result
                    .error
                    .as_ref()
                    .map(|e| e.message.as_str())
                    .unwrap_or("")
            )
        },
    )?;
    let mut prepared = Vec::new();
    let mut preparation_errors = Vec::new();
    if args.iter().any(|a| a == "--prepare") {
        for result in &sync.results {
            if result.kind != AssetKind::Heightmap || result.cached.is_none() {
                continue;
            }
            match cache.prepare_heightmap(&result.id, 1024) {
                Ok(raster) => {
                    eprintln!("{}: prepared {}x{}", result.id, raster.width, raster.height);
                    prepared.push(raster);
                }
                Err(error) => {
                    eprintln!("{}: preparation failed: {error}", result.id);
                    preparation_errors.push(serde_json::json!({"id":result.id,"error":error}));
                }
            }
        }
    }
    let failed = sync.results.iter().any(|r| r.error.is_some()) || !preparation_errors.is_empty();
    let report = serde_json::json!({"schemaVersion":1,"cacheRoot":cache.root().to_string_lossy(),"resources":resources,"sync":sync,"preparedHeightmaps":prepared,"preparationErrors":preparation_errors,"passed":!failed});
    let json = serde_json::to_string_pretty(&report)?;
    if let Some(path) = value("--output") {
        fs::write(path, &json)?;
    }
    println!("{json}");
    if failed {
        return Err(
            "One or more assets unavailable or rejected; inspect the explicit report".into(),
        );
    }
    Ok(())
}
