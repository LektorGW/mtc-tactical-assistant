import type { CommunityMap, Settings, StoredMap, StoredTerrain } from './types';

/**
 * Maps listed as in rotation by multicrew.wiki on 2026-10-15 (current MTC place
 * 95721658376580). The bundled catalog keeps the source snapshot's own `inRotation`
 * flag and its place 9520328730 as provenance; this list only orders the picker.
 */
export const WIKI_ROTATION = {
  checkedAt: '2026-10-15',
  source: 'https://multicrew.wiki — lista map w rotacji',
  currentPlaceId: '95721658376580',
  ids: ['arctic_airbase', 'dustbowl_ii_rework', 'fulvia_gap', 'muddy_fields', 'normandy', 'rohkshort', 'rohkstov', 'roinburg', 'snowy_fields', 'sokolovka', 'villers_sommeil', 'zone_11'],
} as const;

/** Prepared terrain marker of the v2 (letterboxed, not stretched) preparation. */
export const TERRAIN_PROCESSING_V2 = 'red8-bilinear-contain-square-missing-v2';

export function inWikiRotation(id: string): boolean {
  return (WIKI_ROTATION.ids as readonly string[]).includes(id);
}

const fold = (text: string) => text.normalize('NFD').replace(/[̀-ͯ]/g, '').toLowerCase().replace(/[^a-z0-9]+/g, ' ').trim();

/** Rotation maps first, then alphabetical; search matches name, key or ID without diacritics. */
export function filterCatalog(maps: CommunityMap[], query: string): CommunityMap[] {
  const q = fold(query);
  return maps
    .filter(m => !q || [m.name, m.key, m.id].some(value => fold(value).includes(q)))
    .sort((a, b) => Number(inWikiRotation(b.id)) - Number(inWikiRotation(a.id)) || a.name.localeCompare(b.name, 'pl'));
}

export function sameScale(a: number, b: number): boolean {
  return Math.abs(a - b) <= 1e-9 * Math.max(Math.abs(a), Math.abs(b), 1);
}

/** A library entry made by the catalog workflow for this map (full square texture, north up). */
export function isCatalogEntry(entry: StoredMap, known: CommunityMap): boolean {
  return entry.id === known.id && entry.unit === 'game_meters' && entry.width === entry.height
    && entry.calibration.northRotationDeg === 0 && entry.calibration.originPixel.x === 0 && entry.calibration.originPixel.y === 0
    && sameScale(entry.calibration.unitsPerPixel, known.sizeGameMeters / entry.width);
}

/** Older prepared heightmaps stretched non-square sources; they are not reused. */
export function isCurrentTerrain(terrain: StoredTerrain | undefined | null, map: StoredMap): terrain is StoredTerrain {
  return !!terrain && terrain.mapId === map.id && terrain.mapWidth === map.width && terrain.mapHeight === map.height
    && terrain.unit === map.unit && (!terrain.source.startsWith('Community ') || terrain.source.includes(TERRAIN_PROCESSING_V2));
}

/** Installs one catalog map as the active map, replacing stale library entries of the same ID. */
export function applyCatalogMap(current: Settings, known: CommunityMap, map: StoredMap, terrain: StoredTerrain | null): Settings {
  const sameMap = current.communityMapId === map.id && current.mapPath === map.path && current.mapWidth === map.width && current.mapHeight === map.height;
  return {
    ...current,
    maps: [...current.maps.filter(m => m.id !== map.id), map],
    terrains: [...current.terrains.filter(t => t.mapId !== map.id), ...(terrain ? [terrain] : [])],
    terrain: current.terrain?.mapId === map.id ? null : current.terrain,
    mapSource: 'catalog', communityMapId: map.id, catalogSourceAccepted: true,
    mapPath: map.path, mapName: known.name, mapWidth: map.width, mapHeight: map.height,
    scale: map.calibration.unitsPerPixel, rotation: 0, mapUnit: 'game_meters', unit: 'm gry',
    own: sameMap ? current.own : null, target: sameMap ? current.target : null,
    markerConfig: sameMap ? current.markerConfig : { ...current.markerConfig, mapWidth: map.width, mapHeight: map.height, coordinateCalibrationConfirmed: false, viewSignature: null },
  };
}
