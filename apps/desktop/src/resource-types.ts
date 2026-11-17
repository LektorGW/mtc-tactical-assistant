import type { StoredMap, StoredTerrain, Unit } from './types';

export type AssetKind = 'mapImage' | 'heightmap' | 'preparedHeightmap';
export type SyncStatus = 'downloaded' | 'cached' | 'missing' | 'failed';
export type AssetSourceMetadata = {
  repository: string; commit: string; urls: string[]; codeLicense: string;
  rasterLicense: string; redistributionAllowed: boolean; alignment: string;
};
export type KnownMapResource = {
  schemaVersion: number; id: string; name: string; key: string;
  mapUrl: string; heightmapUrl: string | null; sizeGameMeters: number;
  heightRangeGameMeters: { minimum: number; maximum: number } | null;
  unit: Unit; status: string; sourceMetadata: AssetSourceMetadata;
};
export type CachedRaster = {
  schemaVersion: number; id: string; kind: AssetKind; path: string; sha256: string;
  width: number; height: number; byteLength: number; downloadedAtMs: number;
  url: string; status: string; sourceMetadata: AssetSourceMetadata;
  sourceSha256: string | null; processing: string | null;
};
export type SyncReport = {
  schemaVersion: number; mode: 'online' | 'offline'; completedAtMs: number;
  results: { id: string; kind: AssetKind; status: SyncStatus; cached: CachedRaster | null;
    changed: boolean; error: { code: string; message: string } | null }[];
};
export type ResourceLibrary = {
  maps: StoredMap[]; terrains: StoredTerrain[]; warnings: string[]; report: SyncReport;
};
export type ResourceSyncRequest = {
  ids: string[]; includeTerrain: boolean; online: boolean; sourceAcknowledged: boolean;
};
export type ResourceProgress = { completed: number; id: string; kind: AssetKind; status: SyncStatus };

export function validateResourceCatalog(value: unknown): KnownMapResource[] {
  if (!Array.isArray(value) || value.length < 1 || value.length > 25) throw new Error('Katalog musi zawierać od jednej do 25 znanych map');
  const ids = new Set<string>();
  for (const entry of value as KnownMapResource[]) {
    const source = entry?.sourceMetadata;
    const range = entry?.heightRangeGameMeters;
    if (!entry || entry.schemaVersion !== 1 || typeof entry.id !== 'string' || !/^[a-z0-9_]{1,80}$/.test(entry.id) || ids.has(entry.id)
      || typeof entry.name !== 'string' || !entry.name.trim() || typeof entry.key !== 'string' || !entry.key.trim()
      || entry.unit !== 'game_meters' || entry.status !== 'experimental'
      || !Number.isFinite(entry.sizeGameMeters) || entry.sizeGameMeters <= 0
      || typeof entry.mapUrl !== 'string' || !entry.mapUrl.startsWith('https://')
      || (entry.heightmapUrl !== null && (typeof entry.heightmapUrl !== 'string' || !entry.heightmapUrl.startsWith('https://')))
      || (entry.heightmapUrl === null) !== (range === null)
      || (range !== null && (!range || !Number.isFinite(range.minimum) || !Number.isFinite(range.maximum) || range.minimum >= range.maximum))
      || !source || typeof source.repository !== 'string' || !source.repository.startsWith('https://')
      || typeof source.commit !== 'string' || !/^[a-f0-9]{40}$/i.test(source.commit)
      || !Array.isArray(source.urls) || source.urls.length === 0 || source.urls.some(url => typeof url !== 'string' || !url.startsWith('https://'))
      || typeof source.codeLicense !== 'string' || !source.codeLicense.trim()
      || typeof source.rasterLicense !== 'string' || !source.rasterLicense.trim()
      || typeof source.redistributionAllowed !== 'boolean' || typeof source.alignment !== 'string' || !source.alignment.trim()) {
      throw new Error('Katalog zawiera niepoprawną tożsamość, jednostki lub pochodzenie mapy');
    }
    ids.add(entry.id);
  }
  return value as KnownMapResource[];
}

export function buildResourceRequest(catalog: KnownMapResource[], ids: string[], includeTerrain: boolean, online: boolean, sourceAcknowledged: boolean): ResourceSyncRequest {
  if (sourceAcknowledged !== true) throw new Error('Potwierdź użycie eksperymentalnego źródła map');
  if (typeof includeTerrain !== 'boolean' || typeof online !== 'boolean') throw new Error('Niepoprawny tryb synchronizacji');
  if (!Array.isArray(ids) || ids.length < 1 || ids.length > 25 || new Set(ids).size !== ids.length
    || ids.some(id => !catalog.some(map => map.id === id))) {
    throw new Error('Wybierz od jednej do 25 różnych map z katalogu');
  }
  // Native reselects exact pinned URLs by ID; no caller-supplied URLs are sent.
  return { ids: catalog.filter(map => ids.includes(map.id)).map(map => map.id), includeTerrain, online, sourceAcknowledged: true };
}

export function expectedResourceItems(catalog: KnownMapResource[], request: ResourceSyncRequest): number {
  return request.ids.length + (request.includeTerrain ? catalog.filter(map => request.ids.includes(map.id) && map.heightmapUrl !== null).length : 0);
}

/** Events are informational. Only the RPC response and onInstall establish
 * completion; foreign IDs, unsupported kinds and regressing counters are ignored. */
export function acceptResourceProgress(previous: ResourceProgress | null, value: unknown, catalog: KnownMapResource[], request: ResourceSyncRequest): ResourceProgress | null {
  if (!value || typeof value !== 'object') return previous;
  const next = value as ResourceProgress;
  const map = catalog.find(map => map.id === next.id);
  if (!map || !request.ids.includes(next.id) || !Number.isSafeInteger(next.completed)
    || next.completed < 1 || next.completed > expectedResourceItems(catalog, request)
    || (previous !== null && next.completed <= previous.completed)
    || !['downloaded', 'cached', 'missing', 'failed'].includes(next.status)
    || (next.kind !== 'mapImage' && !(next.kind === 'heightmap' && request.includeTerrain && map.heightmapUrl !== null))) return previous;
  return { completed: next.completed, id: next.id, kind: next.kind, status: next.status };
}

export const resourceStatusLabel: Record<SyncStatus, string> = {
  downloaded: 'pobrano', cached: 'sprawdzono lokalny cache', missing: 'brak w cache', failed: 'błąd',
};
export const resourceKindLabel: Record<AssetKind, string> = {
  mapImage: 'mapa', heightmap: 'teren źródłowy', preparedHeightmap: 'przygotowany teren',
};
