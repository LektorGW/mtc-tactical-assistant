use super::*;
fn bytes(width: u32, height: u32) -> Vec<u8> {
    let image = RgbaImage::from_fn(width, height, |x, y| {
        Rgba([((x + y) * 10) as u8, 0, 255, 255])
    });
    let mut encoded = Cursor::new(Vec::new());
    image.write_to(&mut encoded, ImageFormat::Png).unwrap();
    encoded.into_inner()
}
fn install(cache: &AssetCache, id: &str, kind: AssetKind, bytes: &[u8]) -> CachedRaster {
    let image = decode_png(bytes, &cache.limits).unwrap();
    cache
        .install(
            &known_map(id).unwrap(),
            kind,
            bytes,
            image.dimensions(),
            None,
            None,
        )
        .unwrap()
}
#[test]
fn catalog_contains_25_exact_maps_and_21_declared_height_encodings() {
    let catalog = known_maps().unwrap();
    assert_eq!(catalog.len(), 25);
    assert_eq!(
        catalog.iter().filter(|r| r.heightmap_url.is_some()).count(),
        21
    );
    let airbase = catalog.iter().find(|r| r.id == "arctic_airbase").unwrap();
    assert_eq!(
        airbase.map_url,
        "https://maps.artillery-calculator.com/airbase/user.png"
    );
    assert!(
        (airbase.height_range_game_meters.as_ref().unwrap().maximum - 566.8941650390625 * 0.36)
            .abs()
            < 1e-9
    );
    assert!(catalog.iter().all(|r| r.status == "experimental"
        && !r.source_metadata.redistribution_allowed
        && r.source_metadata.raster_license == "unconfirmed"));
    assert_eq!(
        catalog
            .iter()
            .find(|r| r.id == "no_mans_land")
            .unwrap()
            .map_url,
        "https://maps.artillery-calculator.com/no-mans-land/user.png"
    );
}
#[test]
fn rejects_non_https_alternate_hosts_credentials_ports_queries_and_unknown_paths() {
    validate_resource_url("https://maps.artillery-calculator.com/airbase/user.png").unwrap();
    for url in [
        "http://maps.artillery-calculator.com/airbase/user.png",
        "https://example.com/airbase/user.png",
        "https://maps.artillery-calculator.com.evil.test/airbase/user.png",
        "https://user:password@maps.artillery-calculator.com/airbase/user.png",
        "https://maps.artillery-calculator.com:443/airbase/user.png",
        "https://maps.artillery-calculator.com/airbase/user.png?q=90",
        "https://maps.artillery-calculator.com/airbase/user.png#x",
        "https://maps.artillery-calculator.com/../airbase/user.png",
        "https://maps.artillery-calculator.com/new-map/user.png",
    ] {
        assert_eq!(
            validate_resource_url(url).unwrap_err().code,
            ErrorCode::InvalidRequest,
            "{url}"
        );
    }
}
#[test]
fn requires_acknowledgement_and_known_distinct_requests_before_network_or_cache_use() {
    let dir = tempfile::tempdir().unwrap();
    let cache = AssetCache::new(dir.path(), CacheLimits::default()).unwrap();
    assert_eq!(
        cache
            .sync(&["arctic_airbase".into()], false, false, SyncMode::Online)
            .unwrap_err()
            .code,
        ErrorCode::SourceNotAcknowledged
    );
    assert_eq!(
        cache
            .sync(
                &["arctic_airbase".into(), "arctic_airbase".into()],
                false,
                true,
                SyncMode::Online
            )
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        cache
            .sync(&["../outside".into()], false, true, SyncMode::Online)
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    assert!(fs::read_dir(dir.path().join("blobs"))
        .unwrap()
        .next()
        .is_none());
}
#[test]
fn offline_mode_reports_missing_and_validates_cached_content_without_network() {
    let dir = tempfile::tempdir().unwrap();
    let cache = AssetCache::new(dir.path(), CacheLimits::default()).unwrap();
    let report = cache
        .sync(&["arctic_airbase".into()], true, true, SyncMode::Offline)
        .unwrap();
    assert_eq!(report.results.len(), 2);
    assert!(report
        .results
        .iter()
        .all(|r| r.cached.is_none() && r.error.as_ref().unwrap().code == ErrorCode::Missing));
    let installed = install(&cache, "arctic_airbase", AssetKind::MapImage, &bytes(8, 8));
    let report = cache
        .sync(&["arctic_airbase".into()], false, true, SyncMode::Offline)
        .unwrap();
    assert_eq!(
        report.results[0].cached.as_ref().unwrap().sha256,
        installed.sha256
    );
    fs::write(&installed.path, bytes(9, 9)).unwrap();
    assert_eq!(
        cache
            .validate_cached("arctic_airbase", AssetKind::MapImage)
            .unwrap_err()
            .code,
        ErrorCode::Integrity
    );
    let repaired = install(&cache, "arctic_airbase", AssetKind::MapImage, &bytes(8, 8));
    assert_eq!(repaired.sha256, installed.sha256);
    assert_eq!(
        cache
            .validate_cached("arctic_airbase", AssetKind::MapImage)
            .unwrap()
            .sha256,
        installed.sha256
    );
}
#[test]
fn cache_commit_replaces_indices_atomically_and_does_not_trust_metadata_geometry() {
    let dir = tempfile::tempdir().unwrap();
    let cache = AssetCache::new(dir.path(), CacheLimits::default()).unwrap();
    let a = install(&cache, "arctic_airbase", AssetKind::MapImage, &bytes(8, 8));
    let b = install(
        &cache,
        "arctic_airbase",
        AssetKind::MapImage,
        &bytes(10, 12),
    );
    assert_ne!(a.sha256, b.sha256);
    assert_eq!(
        cache
            .validate_cached("arctic_airbase", AssetKind::MapImage)
            .unwrap()
            .sha256,
        b.sha256
    );
    let path = cache.record_path("arctic_airbase", AssetKind::MapImage);
    let mut record = cache
        .read_record("arctic_airbase", AssetKind::MapImage)
        .unwrap();
    record.width += 1;
    fs::write(path, serde_json::to_vec(&record).unwrap()).unwrap();
    assert_eq!(
        cache
            .validate_cached("arctic_airbase", AssetKind::MapImage)
            .unwrap_err()
            .code,
        ErrorCode::Integrity
    );
    assert!(fs::read_dir(dir.path().join("entries"))
        .unwrap()
        .all(|entry| entry.unwrap().path().extension().unwrap() == "json"));
}
#[test]
fn bounds_stream_bytes_png_headers_decoded_pixels_memory_and_disk_quota() {
    assert_eq!(
        read_bounded(&mut Cursor::new(vec![0; 11]), 10)
            .unwrap_err()
            .code,
        ErrorCode::Limits
    );
    assert_eq!(
        decode_png(b"not a PNG", &CacheLimits::default())
            .unwrap_err()
            .code,
        ErrorCode::InvalidImage
    );
    let mut huge = bytes(2, 2);
    huge[16..20].copy_from_slice(&9000u32.to_be_bytes());
    assert_eq!(
        decode_png(&huge, &CacheLimits::default()).unwrap_err().code,
        ErrorCode::Limits
    );
    let limit = CacheLimits {
        max_pixels: 10,
        ..Default::default()
    };
    assert_eq!(
        decode_png(&bytes(8, 8), &limit).unwrap_err().code,
        ErrorCode::Limits
    );
    let limit = CacheLimits {
        max_decode_bytes: 1,
        ..Default::default()
    };
    assert_eq!(
        decode_png(&bytes(8, 8), &limit).unwrap_err().code,
        ErrorCode::InvalidImage
    );
    let limit = CacheLimits {
        max_body_bytes: 100,
        max_cache_bytes: 100,
        ..Default::default()
    };
    let dir = tempfile::tempdir().unwrap();
    let cache = AssetCache::new(dir.path(), limit).unwrap();
    fs::write(cache.root.join("blobs/occupied"), vec![0; 90]).unwrap();
    let tiny = bytes(2, 2);
    assert_eq!(
        cache
            .install(
                &known_map("arctic_airbase").unwrap(),
                AssetKind::MapImage,
                &tiny,
                (2, 2),
                None,
                None
            )
            .unwrap_err()
            .code,
        ErrorCode::Limits
    );
    assert!(fs::read_dir(cache.root.join("entries"))
        .unwrap()
        .next()
        .is_none());
}
#[test]
fn height_preparation_reads_red_only_and_preserves_missing_samples_and_xy_endpoints() {
    let image = image::DynamicImage::ImageRgba8(RgbaImage::from_fn(4, 2, |x, y| {
        Rgba([((x + 4 * y) * 30) as u8, 255, 1, 255])
    }));
    // A 4×2 source is centered in the 4×4 square map extent; margins are missing.
    let prepared = prepare_red_channel(&image, 32);
    assert_eq!(prepared.dimensions(), (4, 4));
    assert_eq!(prepared.get_pixel(3, 2), &Rgba([210, 210, 210, 255]));
    assert_eq!(prepared.get_pixel(0, 1), &Rgba([0, 0, 0, 255]));
    assert_eq!(prepared.get_pixel(0, 0)[3], 0);
    assert_eq!(prepared.get_pixel(3, 3)[3], 0);
    let mut missing = image.to_rgba8();
    missing.put_pixel(1, 0, Rgba([100, 100, 100, 0]));
    let prepared = prepare_red_channel(&image::DynamicImage::ImageRgba8(missing), 32);
    assert_eq!(prepared.get_pixel(1, 1)[3], 0);
    let large = image::DynamicImage::ImageRgba8(RgbaImage::from_fn(64, 48, |x, y| {
        Rgba([((x + y) * 2) as u8, 0, 0, 255])
    }));
    let small = prepare_red_channel(&large, 32);
    assert_eq!(small.dimensions(), (32, 32));
    assert_eq!(small.get_pixel(31, 27)[0], 220);
    assert_eq!(small.get_pixel(0, 4)[0], 0);
    assert_eq!(small.get_pixel(0, 3)[3], 0);
}
#[test]
fn tall_height_preparation_is_pillarboxed_not_stretched() {
    // Dustbowl-like portrait raster: width/height ≈ 0.86.
    let tall =
        image::DynamicImage::ImageRgba8(RgbaImage::from_fn(43, 50, |_, _| Rgba([100, 0, 0, 255])));
    let prepared = prepare_red_channel(&tall, 50);
    assert_eq!(prepared.dimensions(), (50, 50));
    assert_eq!(prepared.get_pixel(0, 25)[3], 0);
    assert_eq!(prepared.get_pixel(25, 0), &Rgba([100, 100, 100, 255]));
    assert_eq!(prepared.get_pixel(49, 49)[3], 0);
}
#[test]
fn prepared_heightmap_tracks_raw_hash_and_rejects_changed_source_or_bad_variant() {
    let dir = tempfile::tempdir().unwrap();
    let cache = AssetCache::new(dir.path(), CacheLimits::default()).unwrap();
    install(
        &cache,
        "arctic_airbase",
        AssetKind::Heightmap,
        &bytes(64, 48),
    );
    let prepared = cache.prepare_heightmap("arctic_airbase", 32).unwrap();
    assert_eq!((prepared.width, prepared.height), (32, 32));
    assert!(prepared.source_sha256.is_some());
    assert_eq!(
        cache
            .prepare_heightmap("arctic_airbase", 32)
            .unwrap()
            .sha256,
        prepared.sha256
    );
    install(
        &cache,
        "arctic_airbase",
        AssetKind::Heightmap,
        &bytes(65, 48),
    );
    assert_eq!(
        cache
            .validate_cached("arctic_airbase", AssetKind::PreparedHeightmap)
            .unwrap_err()
            .code,
        ErrorCode::Integrity
    );
    assert_ne!(
        cache
            .prepare_heightmap("arctic_airbase", 32)
            .unwrap()
            .source_sha256,
        prepared.source_sha256
    );
    assert_eq!(
        cache
            .prepare_heightmap("arctic_airbase", 4096)
            .unwrap_err()
            .code,
        ErrorCode::Limits
    );
    assert_eq!(
        cache.prepare_heightmap("gensokyo", 32).unwrap_err().code,
        ErrorCode::Unsupported
    );
}
