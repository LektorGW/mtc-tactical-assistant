//! Explicit setup-time downloads of known public community assets. No game access.
use image::{GenericImageView, ImageFormat, ImageReader, Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub const SOURCE_COMMIT: &str = "39a7af7689524e57ca2326f3b998cf123df95426";
const REPOSITORY: &str = "https://github.com/grand-hawk/artillery-calculator";
const HOST: &str = "maps.artillery-calculator.com";
/// v2: non-square source heightmaps are centered and fitted (letterboxed) into
/// the square extent of the map texture instead of being stretched; margins are
/// missing data. v1 files are regenerated on the next preparation.
const PROCESSING_VERSION: &str = "red8-bilinear-contain-square-missing-v2";
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AssetKind {
    MapImage,
    Heightmap,
    PreparedHeightmap,
}
impl AssetKind {
    fn filename(self) -> &'static str {
        match self {
            Self::MapImage => "map",
            Self::Heightmap => "heightmap",
            Self::PreparedHeightmap => "prepared-heightmap",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HeightRange {
    pub minimum: f64,
    pub maximum: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssetSourceMetadata {
    pub repository: String,
    pub commit: String,
    pub urls: Vec<String>,
    pub code_license: String,
    pub raster_license: String,
    pub redistribution_allowed: bool,
    pub alignment: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KnownMapResource {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub key: String,
    pub map_url: String,
    pub heightmap_url: Option<String>,
    pub size_game_meters: f64,
    pub height_range_game_meters: Option<HeightRange>,
    pub unit: String,
    pub status: String,
    pub source_metadata: AssetSourceMetadata,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SeedMap {
    schema_version: u32,
    id: String,
    name: String,
    key: String,
    size_game_meters: f64,
    height_range_studs: Option<HeightRange>,
    unit: String,
    source_metadata: SeedSource,
}
#[derive(Deserialize)]
struct SeedSource {
    repository: String,
    commit: String,
}

pub fn known_maps() -> Result<Vec<KnownMapResource>, AssetError> {
    let seeds: Vec<SeedMap> = serde_json::from_str(include_str!(
        "../../../data/maps/community-maps.json"
    ))
    .map_err(|e| {
        error(
            ErrorCode::Integrity,
            format!("Niepoprawny wbudowany katalog: {e}"),
        )
    })?;
    let mut ids = BTreeSet::new();
    seeds.into_iter().map(|seed| {
        if seed.schema_version != 1 || seed.unit != "game_meters" || seed.source_metadata.repository != REPOSITORY || seed.source_metadata.commit != SOURCE_COMMIT || !valid_id(&seed.id) || !ids.insert(seed.id.clone()) || !seed.size_game_meters.is_finite() || seed.size_game_meters <= 0.0 {
            return Err(error(ErrorCode::Integrity, "Niepoprawna tożsamość lub skala katalogu"));
        }
        // Reviewed upstream keys are ASCII; this reproduces slug() for them.
        let slug: String = seed.key.chars().filter(|c| *c != '\'').map(|c| if c == ' ' { '-' } else { c.to_ascii_lowercase() }).collect();
        if slug.is_empty() || !slug.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
            return Err(error(ErrorCode::Integrity, "Klucz mapy wymaga przeglądu reguły slug"));
        }
        let height_range_game_meters = seed.height_range_studs.map(|range| HeightRange { minimum: range.minimum * 0.36, maximum: range.maximum * 0.36 });
        if height_range_game_meters.as_ref().is_some_and(|r| !r.minimum.is_finite() || !r.maximum.is_finite() || r.minimum >= r.maximum) {
            return Err(error(ErrorCode::Integrity, "Niepoprawny zakres wysokości katalogu"));
        }
        let source_paths = ["packages/mtc-artillery/src/config/maps.ts", "packages/mtc-artillery/src/utils/images.ts", "packages/mtc-artillery/src/utils/math.ts", "packages/mtc-artillery/src/hooks/data/useHeightmapZ.ts", "packages/mtc-artillery/src/hooks/data/useHeightDifference.ts", "packages/mtc-artillery/src/components/templates/Canvas.tsx", "LICENSE.md"];
        Ok(KnownMapResource {
            schema_version: 1, id: seed.id, name: seed.name, key: seed.key,
            map_url: format!("https://{HOST}/{slug}/user.png"),
            heightmap_url: height_range_game_meters.as_ref().map(|_| format!("https://{HOST}/{slug}/heightmap.png")),
            size_game_meters: seed.size_game_meters, height_range_game_meters,
            unit: "game_meters".into(), status: "experimental".into(),
            source_metadata: AssetSourceMetadata { repository: REPOSITORY.into(), commit: SOURCE_COMMIT.into(),
                urls: source_paths.iter().map(|path| format!("{REPOSITORY}/blob/{SOURCE_COMMIT}/{path}")).collect(),
                code_license: "MIT; Copyright (c) 2024 Astrid".into(), raster_license: "unconfirmed".into(),
                redistribution_allowed: false, alignment: "upstreamNormalizedXy; orientation/current-game alignment unverified".into() },
        })
    }).collect()
}
fn known_map(id: &str) -> Result<KnownMapResource, AssetError> {
    known_maps()?
        .into_iter()
        .find(|m| m.id == id)
        .ok_or_else(|| {
            error(
                ErrorCode::InvalidRequest,
                "Nieznane ID mapy — adres nie będzie pobierany",
            )
        })
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Only exact catalog URLs are accepted; caller-supplied hosts, queries and redirects are not.
pub fn validate_resource_url(url: &str) -> Result<(), AssetError> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|_| error(ErrorCode::InvalidRequest, "Niepoprawny URL zasobu"))?;
    if parsed.scheme() != "https"
        || parsed.host_str() != Some(HOST)
        || parsed.port().is_some()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || !known_maps()?
            .iter()
            .any(|m| m.map_url == url || m.heightmap_url.as_deref() == Some(url))
    {
        return Err(error(
            ErrorCode::InvalidRequest,
            "URL nie należy do dokładnej allowlisty zasobów map",
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorCode {
    InvalidRequest,
    SourceNotAcknowledged,
    Missing,
    Integrity,
    Limits,
    Network,
    Http,
    InvalidImage,
    Io,
    Unsupported,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetError {
    pub code: ErrorCode,
    pub message: String,
}
impl std::fmt::Display for AssetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}
impl std::error::Error for AssetError {}
impl From<std::io::Error> for AssetError {
    fn from(e: std::io::Error) -> Self {
        error(ErrorCode::Io, e.to_string())
    }
}
fn error(code: ErrorCode, message: impl Into<String>) -> AssetError {
    AssetError {
        code,
        message: message.into(),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct CacheLimits {
    pub max_body_bytes: u64,
    pub max_dimension: u32,
    pub max_pixels: u64,
    pub max_decode_bytes: u64,
    pub max_cache_bytes: u64,
    pub timeout_seconds: u64,
    pub connect_timeout_seconds: u64,
}
impl Default for CacheLimits {
    fn default() -> Self {
        Self {
            max_body_bytes: 40 * 1024 * 1024,
            max_dimension: 8192,
            max_pixels: 64 * 1024 * 1024,
            max_decode_bytes: 320 * 1024 * 1024,
            max_cache_bytes: 512 * 1024 * 1024,
            timeout_seconds: 30,
            connect_timeout_seconds: 10,
        }
    }
}
impl CacheLimits {
    fn validate(&self) -> Result<(), AssetError> {
        if self.max_body_bytes == 0
            || self.max_body_bytes > 40 * 1024 * 1024
            || self.max_dimension < 2
            || self.max_dimension > 8192
            || self.max_pixels == 0
            || self.max_pixels > 64 * 1024 * 1024
            || self.max_decode_bytes == 0
            || self.max_decode_bytes > 320 * 1024 * 1024
            || self.max_cache_bytes < self.max_body_bytes
            || self.max_cache_bytes > 1024 * 1024 * 1024
            || !(1..=60).contains(&self.timeout_seconds)
            || !(1..=30).contains(&self.connect_timeout_seconds)
            || self.connect_timeout_seconds > self.timeout_seconds
        {
            return Err(error(
                ErrorCode::Limits,
                "Niepoprawne lub nieograniczone limity cache",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncMode {
    Online,
    Offline,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncStatus {
    Downloaded,
    Cached,
    Missing,
    Failed,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CachedRaster {
    pub schema_version: u32,
    pub id: String,
    pub kind: AssetKind,
    pub path: String,
    pub sha256: String,
    pub width: u32,
    pub height: u32,
    pub byte_length: u64,
    pub downloaded_at_ms: u64,
    pub url: String,
    pub status: String,
    pub source_metadata: AssetSourceMetadata,
    pub source_sha256: Option<String>,
    pub processing: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetSyncResult {
    pub id: String,
    pub kind: AssetKind,
    pub status: SyncStatus,
    pub cached: Option<CachedRaster>,
    pub changed: bool,
    pub error: Option<AssetError>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    pub schema_version: u32,
    pub mode: SyncMode,
    pub completed_at_ms: u64,
    pub results: Vec<AssetSyncResult>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CacheRecord {
    schema_version: u32,
    id: String,
    kind: AssetKind,
    sha256: String,
    width: u32,
    height: u32,
    byte_length: u64,
    downloaded_at_ms: u64,
    url: String,
    source_metadata: AssetSourceMetadata,
    source_sha256: Option<String>,
    processing: Option<String>,
}
impl CacheRecord {
    fn public(&self, path: PathBuf) -> CachedRaster {
        CachedRaster {
            schema_version: self.schema_version,
            id: self.id.clone(),
            kind: self.kind,
            path: path.to_string_lossy().into(),
            sha256: self.sha256.clone(),
            width: self.width,
            height: self.height,
            byte_length: self.byte_length,
            downloaded_at_ms: self.downloaded_at_ms,
            url: self.url.clone(),
            status: "experimental".into(),
            source_metadata: self.source_metadata.clone(),
            source_sha256: self.source_sha256.clone(),
            processing: self.processing.clone(),
        }
    }
}

/// Holds no networking client or background task. Network access starts only in online sync.
pub struct AssetCache {
    root: PathBuf,
    limits: CacheLimits,
    operation: Mutex<()>,
}
impl AssetCache {
    pub fn new(root: impl AsRef<Path>, limits: CacheLimits) -> Result<Self, AssetError> {
        limits.validate()?;
        fs::create_dir_all(root.as_ref())?;
        let root = root.as_ref().canonicalize()?;
        for directory in ["blobs", "entries"] {
            fs::create_dir_all(root.join(directory))?;
            if !root.join(directory).canonicalize()?.starts_with(&root)
                || fs::symlink_metadata(root.join(directory))?
                    .file_type()
                    .is_symlink()
            {
                return Err(error(
                    ErrorCode::Integrity,
                    "Podkatalog cache wychodzi poza wybrany katalog",
                ));
            }
        }
        Ok(Self {
            root,
            limits,
            operation: Mutex::new(()),
        })
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn sync(
        &self,
        ids: &[String],
        include_terrain: bool,
        source_acknowledged: bool,
        mode: SyncMode,
    ) -> Result<SyncReport, AssetError> {
        self.sync_with_progress(ids, include_terrain, source_acknowledged, mode, |_| {})
    }
    pub fn sync_with_progress(
        &self,
        ids: &[String],
        include_terrain: bool,
        source_acknowledged: bool,
        mode: SyncMode,
        mut progress: impl FnMut(&AssetSyncResult),
    ) -> Result<SyncReport, AssetError> {
        if !source_acknowledged {
            return Err(error(
                ErrorCode::SourceNotAcknowledged,
                "Potwierdź użycie eksperymentalnych źródeł map przed synchronizacją",
            ));
        }
        if ids.is_empty() || ids.len() > 25 {
            return Err(error(
                ErrorCode::InvalidRequest,
                "Wybierz od jednej do 25 znanych map",
            ));
        }
        let mut unique = BTreeSet::new();
        let resources: Vec<_> = ids
            .iter()
            .map(|id| {
                if !unique.insert(id) {
                    return Err(error(ErrorCode::InvalidRequest, "Powtórzone ID żądania"));
                }
                known_map(id)
            })
            .collect::<Result<_, _>>()?;
        let _operation = self
            .operation
            .lock()
            .map_err(|_| error(ErrorCode::Io, "Cache jest zajęty po błędzie wątku"))?;
        let client = if matches!(mode, SyncMode::Online) {
            Some(
                reqwest::blocking::Client::builder()
                    .https_only(true)
                    .no_proxy()
                    .redirect(reqwest::redirect::Policy::none())
                    .connect_timeout(Duration::from_secs(self.limits.connect_timeout_seconds))
                    .timeout(Duration::from_secs(self.limits.timeout_seconds))
                    .user_agent("MTC-Tactical-Assistant-community-assets/0.5")
                    .build()
                    .map_err(|e| error(ErrorCode::Network, e.to_string()))?,
            )
        } else {
            None
        };
        let mut results = Vec::new();
        for resource in resources {
            for kind in [AssetKind::MapImage, AssetKind::Heightmap] {
                if kind == AssetKind::Heightmap
                    && (!include_terrain || resource.heightmap_url.is_none())
                {
                    continue;
                }
                let previous = self.read_record(&resource.id, kind).ok().map(|r| r.sha256);
                let outcome = if let Some(client) = &client {
                    self.download(client, &resource, kind)
                } else {
                    self.validate_cached_unlocked(&resource.id, kind)
                };
                let result = match outcome {
                    Ok(cached) => AssetSyncResult {
                        id: resource.id.clone(),
                        kind,
                        status: if client.is_some() {
                            SyncStatus::Downloaded
                        } else {
                            SyncStatus::Cached
                        },
                        changed: previous.as_ref().is_some_and(|sha| sha != &cached.sha256),
                        cached: Some(cached),
                        error: None,
                    },
                    Err(error) => AssetSyncResult {
                        id: resource.id.clone(),
                        kind,
                        status: if error.code == ErrorCode::Missing {
                            SyncStatus::Missing
                        } else {
                            SyncStatus::Failed
                        },
                        cached: None,
                        changed: false,
                        error: Some(error),
                    },
                };
                progress(&result);
                results.push(result);
            }
        }
        Ok(SyncReport {
            schema_version: 1,
            mode,
            completed_at_ms: now_ms(),
            results,
        })
    }
    pub fn validate_cached(&self, id: &str, kind: AssetKind) -> Result<CachedRaster, AssetError> {
        let _operation = self
            .operation
            .lock()
            .map_err(|_| error(ErrorCode::Io, "Cache jest zajęty"))?;
        self.validate_cached_unlocked(id, kind)
    }
    fn validate_cached_unlocked(
        &self,
        id: &str,
        kind: AssetKind,
    ) -> Result<CachedRaster, AssetError> {
        let resource = known_map(id)?;
        let url = resource_url(&resource, kind)?;
        let record = self.read_record(id, kind)?;
        if record.schema_version != 1
            || record.id != id
            || record.kind != kind
            || record.url != url
            || record.source_metadata != resource.source_metadata
            || !valid_hash(&record.sha256)
            || record.downloaded_at_ms > now_ms() + 1000
        {
            return Err(error(
                ErrorCode::Integrity,
                "Metadane cache nie zgadzają się z katalogiem",
            ));
        }
        if kind == AssetKind::PreparedHeightmap {
            let original = self.validate_cached_unlocked(id, AssetKind::Heightmap)?;
            let processing = record.processing.as_deref().unwrap_or("");
            if record.source_sha256.as_deref() != Some(&original.sha256)
                || !processing.starts_with(&format!("{PROCESSING_VERSION}:"))
            {
                return Err(error(
                    ErrorCode::Integrity,
                    "Przygotowana heightmap pochodzi z innego lub zmienionego źródła",
                ));
            }
            let dimension = processing
                .split(':')
                .next_back()
                .and_then(|s| s.parse::<u32>().ok())
                .ok_or_else(|| {
                    error(
                        ErrorCode::Integrity,
                        "Nieznany wariant przetwarzania heightmap",
                    )
                })?;
            validate_prepared_dimension(dimension)?;
            if processing != format!("{PROCESSING_VERSION}:{dimension}") {
                return Err(error(
                    ErrorCode::Integrity,
                    "Nieznana wersja przetwarzania heightmap",
                ));
            }
            if record.width > dimension
                || record.height > dimension
                || record.width as u64 * record.height as u64 > 4_194_304
            {
                return Err(error(
                    ErrorCode::Integrity,
                    "Niepoprawna geometria przygotowanej heightmap",
                ));
            }
        } else if record.source_sha256.is_some() || record.processing.is_some() {
            return Err(error(
                ErrorCode::Integrity,
                "Oryginalny raster ma metadane pochodnego pliku",
            ));
        }
        let path = self.blob_path(&record.sha256);
        let bytes = self.read_safe_file(&path, self.limits.max_body_bytes)?;
        if bytes.len() as u64 != record.byte_length || hash(&bytes) != record.sha256 {
            return Err(error(
                ErrorCode::Integrity,
                "Zmieniony plik cache — SHA-256 lub długość nie pasuje",
            ));
        }
        let image = decode_png(&bytes, &self.limits)?;
        if image.width() != record.width || image.height() != record.height {
            return Err(error(
                ErrorCode::Integrity,
                "Wymiary cache nie zgadzają się z rastrem",
            ));
        }
        Ok(record.public(path))
    }
    fn download(
        &self,
        client: &reqwest::blocking::Client,
        resource: &KnownMapResource,
        kind: AssetKind,
    ) -> Result<CachedRaster, AssetError> {
        let url = resource_url(resource, kind)?;
        validate_resource_url(&url)?;
        let mut response = client.get(&url).send().map_err(|e| {
            error(
                ErrorCode::Network,
                format!("Nie pobrano {}: {e}", resource.id),
            )
        })?;
        if response.url().as_str() != url || !response.status().is_success() {
            return Err(error(
                ErrorCode::Http,
                format!(
                    "HTTP {} dla {}; przekierowania są wyłączone",
                    response.status(),
                    resource.id
                ),
            ));
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if content_type.split(';').next() != Some("image/png") {
            return Err(error(
                ErrorCode::InvalidImage,
                "Serwer nie zwrócił image/png",
            ));
        }
        if response
            .content_length()
            .is_some_and(|length| length > self.limits.max_body_bytes)
        {
            return Err(error(
                ErrorCode::Limits,
                "Zasób przekracza limit pobierania",
            ));
        }
        let bytes = read_bounded(&mut response, self.limits.max_body_bytes)?;
        let image = decode_png(&bytes, &self.limits)?;
        self.install(resource, kind, &bytes, image.dimensions(), None, None)
    }
    /// Red-channel linear source encoding; conservative transparent/missing samples.
    /// The caller must still confirm shared normalized XY alignment and calibration.
    pub fn prepare_heightmap(
        &self,
        id: &str,
        max_dimension: u32,
    ) -> Result<CachedRaster, AssetError> {
        validate_prepared_dimension(max_dimension)?;
        let _operation = self
            .operation
            .lock()
            .map_err(|_| error(ErrorCode::Io, "Cache jest zajęty"))?;
        let original = self.validate_cached_unlocked(id, AssetKind::Heightmap)?;
        let processing = format!("{PROCESSING_VERSION}:{max_dimension}");
        if let Ok(cached) = self.validate_cached_unlocked(id, AssetKind::PreparedHeightmap) {
            if cached.processing.as_deref() == Some(&processing) {
                return Ok(cached);
            }
        }
        let bytes = self.read_safe_file(Path::new(&original.path), self.limits.max_body_bytes)?;
        let image = decode_png(&bytes, &self.limits)?;
        let prepared = prepare_red_channel(&image, max_dimension);
        let mut encoded = Cursor::new(Vec::new());
        prepared
            .write_to(&mut encoded, ImageFormat::Png)
            .map_err(|e| error(ErrorCode::InvalidImage, e.to_string()))?;
        self.install(
            &known_map(id)?,
            AssetKind::PreparedHeightmap,
            encoded.get_ref(),
            prepared.dimensions(),
            Some(original.sha256),
            Some(processing),
        )
    }
    fn install(
        &self,
        resource: &KnownMapResource,
        kind: AssetKind,
        bytes: &[u8],
        dimensions: (u32, u32),
        source_sha256: Option<String>,
        processing: Option<String>,
    ) -> Result<CachedRaster, AssetError> {
        if bytes.len() as u64 > self.limits.max_body_bytes {
            return Err(error(ErrorCode::Limits, "Raster przekracza limit pliku"));
        }
        let sha256 = hash(bytes);
        let blob = self.blob_path(&sha256);
        let existing = if blob.exists() {
            Some(self.read_safe_file(&blob, self.limits.max_body_bytes)?)
        } else {
            None
        };
        if existing.as_ref().is_none_or(|prior| hash(prior) != sha256) {
            // Fresh decoded data can repair a damaged ordinary blob. Offline
            // reads still reject corruption without replacing the file.
            let replaced_size = existing.as_ref().map_or(0, |prior| prior.len() as u64);
            if self
                .cache_size()?
                .saturating_sub(replaced_size)
                .saturating_add(bytes.len() as u64)
                > self.limits.max_cache_bytes
            {
                return Err(error(
                    ErrorCode::Limits,
                    "Cache przekracza limit dysku — usuń niepotrzebny cache przed synchronizacją",
                ));
            }
            self.atomic_write(&blob, bytes)?;
        }
        let record = CacheRecord {
            schema_version: 1,
            id: resource.id.clone(),
            kind,
            sha256,
            width: dimensions.0,
            height: dimensions.1,
            byte_length: bytes.len() as u64,
            downloaded_at_ms: now_ms(),
            url: resource_url(resource, kind)?,
            source_metadata: resource.source_metadata.clone(),
            source_sha256,
            processing,
        };
        let metadata = serde_json::to_vec_pretty(&record)
            .map_err(|e| error(ErrorCode::Integrity, e.to_string()))?;
        self.atomic_write(&self.record_path(&resource.id, kind), &metadata)?;
        Ok(record.public(blob))
    }
    fn cache_size(&self) -> Result<u64, AssetError> {
        let mut sum = 0u64;
        for (count, file) in fs::read_dir(self.root.join("blobs"))?.enumerate() {
            if count >= 2048 {
                return Err(error(ErrorCode::Limits, "Zbyt wiele plików cache"));
            }
            let file = file?;
            let metadata = file.metadata()?;
            if !metadata.is_file() || !file.path().canonicalize()?.starts_with(&self.root) {
                return Err(error(
                    ErrorCode::Integrity,
                    "Niepoprawny plik w katalogu blobów",
                ));
            }
            sum = sum.saturating_add(metadata.len());
        }
        Ok(sum)
    }
    fn record_path(&self, id: &str, kind: AssetKind) -> PathBuf {
        self.root
            .join("entries")
            .join(format!("{id}-{}.json", kind.filename()))
    }
    fn blob_path(&self, sha256: &str) -> PathBuf {
        self.root.join("blobs").join(format!("{sha256}.png"))
    }
    fn read_record(&self, id: &str, kind: AssetKind) -> Result<CacheRecord, AssetError> {
        known_map(id)?;
        let bytes = self.read_safe_file(&self.record_path(id, kind), 65_536)?;
        serde_json::from_slice(&bytes).map_err(|e| {
            error(
                ErrorCode::Integrity,
                format!("Niepoprawny indeks cache: {e}"),
            )
        })
    }
    fn read_safe_file(&self, path: &Path, limit: u64) -> Result<Vec<u8>, AssetError> {
        let metadata = fs::symlink_metadata(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                error(ErrorCode::Missing, "Zasób nie jest w lokalnym cache")
            } else {
                e.into()
            }
        })?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || !path.canonicalize()?.starts_with(&self.root)
        {
            return Err(error(
                ErrorCode::Integrity,
                "Plik cache wychodzi poza katalog lub nie jest zwykłym plikiem",
            ));
        }
        if metadata.len() > limit {
            return Err(error(ErrorCode::Limits, "Plik cache przekracza limit"));
        }
        read_bounded(&mut File::open(path)?, limit)
    }
    fn atomic_write(&self, path: &Path, bytes: &[u8]) -> Result<(), AssetError> {
        let parent = path
            .parent()
            .ok_or_else(|| error(ErrorCode::Io, "Brak katalogu docelowego"))?;
        if !parent.canonicalize()?.starts_with(&self.root) {
            return Err(error(
                ErrorCode::Integrity,
                "Zapis cache poza wybranym katalogiem",
            ));
        }
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        temporary.write_all(bytes)?;
        temporary.as_file().sync_all()?;
        temporary.persist(path).map_err(|e| {
            error(
                ErrorCode::Io,
                format!("Nie zatwierdzono pliku cache: {}", e.error),
            )
        })?;
        Ok(())
    }
}
fn resource_url(resource: &KnownMapResource, kind: AssetKind) -> Result<String, AssetError> {
    match kind {
        AssetKind::MapImage => Ok(resource.map_url.clone()),
        _ => resource.heightmap_url.clone().ok_or_else(|| {
            error(
                ErrorCode::Unsupported,
                "Źródło nie deklaruje heightmap dla tej mapy",
            )
        }),
    }
}
fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn read_bounded(reader: &mut impl Read, limit: u64) -> Result<Vec<u8>, AssetError> {
    let mut bytes = Vec::new();
    reader
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| {
            error(
                ErrorCode::Network,
                format!("Nie odczytano całego zasobu: {e}"),
            )
        })?;
    if bytes.len() as u64 > limit {
        return Err(error(ErrorCode::Limits, "Zasób przekracza limit bajtów"));
    }
    Ok(bytes)
}
fn decode_png(bytes: &[u8], limits: &CacheLimits) -> Result<image::DynamicImage, AssetError> {
    if bytes.len() < 33
        || &bytes[..8] != PNG_SIGNATURE
        || &bytes[12..16] != b"IHDR"
        || bytes[24] != 8
    {
        return Err(error(
            ErrorCode::InvalidImage,
            "Wymagany poprawny PNG 8-bitowy",
        ));
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    if width < 2
        || height < 2
        || width > limits.max_dimension
        || height > limits.max_dimension
        || width as u64 * height as u64 > limits.max_pixels
    {
        return Err(error(
            ErrorCode::Limits,
            "Wymiary lub liczba pikseli zasobu przekraczają limit",
        ));
    }
    let mut reader = ImageReader::with_format(Cursor::new(bytes), ImageFormat::Png);
    let mut decoder_limits = image::Limits::default();
    decoder_limits.max_image_width = Some(limits.max_dimension);
    decoder_limits.max_image_height = Some(limits.max_dimension);
    decoder_limits.max_alloc = Some(limits.max_decode_bytes);
    reader.limits(decoder_limits);
    reader.decode().map_err(|e| {
        error(
            ErrorCode::InvalidImage,
            format!("Nie odkodowano PNG w limicie pamięci: {e}"),
        )
    })
}
fn validate_prepared_dimension(value: u32) -> Result<(), AssetError> {
    if !(32..=2048).contains(&value) {
        return Err(error(
            ErrorCode::Limits,
            "Wymiar przygotowanej heightmap musi być w zakresie 32–2048",
        ));
    }
    Ok(())
}
fn prepare_red_channel(image: &image::DynamicImage, max_dimension: u32) -> RgbaImage {
    // Square output: the community map texture is a square that contains the
    // whole playable area; the heightmap's longer side spans that square.
    let longest = image.width().max(image.height());
    let side = longest.min(max_dimension).max(2);
    let scale = side as f64 / longest as f64;
    let width = ((image.width() as f64 * scale).round() as u32).clamp(2, side);
    let height = ((image.height() as f64 * scale).round() as u32).clamp(2, side);
    let offset_x = (side - width) / 2;
    let offset_y = (side - height) / 2;
    RgbaImage::from_fn(side, side, |x, y| {
        if x < offset_x || y < offset_y || x >= offset_x + width || y >= offset_y + height {
            return Rgba([0, 0, 0, 0]);
        }
        let (x, y) = (x - offset_x, y - offset_y);
        let sx = x as f64 * (image.width() - 1) as f64 / (width - 1) as f64;
        let sy = y as f64 * (image.height() - 1) as f64 / (height - 1) as f64;
        let x0 = sx.floor() as u32;
        let y0 = sy.floor() as u32;
        let fx = sx - x0 as f64;
        let fy = sy - y0 as f64;
        let points = [
            (x0, y0, (1.0 - fx) * (1.0 - fy)),
            ((x0 + 1).min(image.width() - 1), y0, fx * (1.0 - fy)),
            (x0, (y0 + 1).min(image.height() - 1), (1.0 - fx) * fy),
            (
                (x0 + 1).min(image.width() - 1),
                (y0 + 1).min(image.height() - 1),
                fx * fy,
            ),
        ];
        let mut red = 0.0;
        for (px, py, weight) in points {
            if weight <= 1e-12 {
                continue;
            }
            let pixel = image.get_pixel(px, py);
            if pixel[3] != 255 {
                return Rgba([0, 0, 0, 0]);
            }
            red += pixel[0] as f64 * weight;
        }
        let red = red.round().clamp(0.0, 255.0) as u8;
        Rgba([red, red, red, 255])
    })
}

#[cfg(test)]
mod tests;
