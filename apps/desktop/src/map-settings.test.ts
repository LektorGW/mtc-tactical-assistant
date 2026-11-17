import { describe, expect, it } from 'vitest';
import { applyCatalogMap, filterCatalog, isCatalogEntry, isCurrentTerrain, TERRAIN_PROCESSING_V2 } from './map-catalog';
import { normalizeMapSettings } from './map-settings';
import { defaultSettings, type CommunityMap, type Settings, type StoredMap, type StoredTerrain } from './types';
import { validateSettings } from './validation';

const known = (id: string, name: string, size = 2048): CommunityMap => ({
  schemaVersion: 1, id, name, key: name, sizeStuds: size / 0.36, sizeGameMeters: size, unit: 'game_meters', status: 'experimental', inRotation: false,
  sourceMetadata: { repository: 'https://example.invalid', commit: 'a'.repeat(40), fetchedAt: 'test', license: 'MIT', urls: ['https://example.invalid/x'] },
});
const entry = (id: string, size = 2048, source = 'Community abc; experimental'): StoredMap => ({
  sha256: 'a'.repeat(64), id, name: id, path: `C:/cache/${id}.png`, width: 1024, height: 1024, unit: 'game_meters', source, confirmed: true,
  calibration: { unitsPerPixel: size / 1024, northRotationDeg: 0, originPixel: { x: 0, y: 0 }, originWorld: { x: 0, y: 0 }, uncertaintyUnits: 0 },
});
const terrain = (id: string, source: string): StoredTerrain => ({
  sha256: 'b'.repeat(64), path: `C:/cache/${id}-h.png`, mapId: id, mapWidth: 1024, mapHeight: 1024, minHeight: 0, maxHeight: 100,
  unit: 'game_meters', source, confirmed: true, coordinateAlignmentConfirmed: true,
});
const load = (raw: Record<string, unknown>) => normalizeMapSettings(raw, validateSettings(raw));

describe('migracja ustawień mapy (settings-v4 → 0.5)', () => {
  it('stary plik bez nowych pól dostaje bezpieczne wartości i zachowuje bibliotekę', () => {
    const { mapSource: _a, catalogSourceAccepted: _b, mapGrid: _c, useTerrainHeight: _d, gameGridSize: _e, ...old } = defaultSettings;
    const migrated = load({ ...old, schemaVersion: 4, maps: [entry('normandy')] });
    expect(migrated.mapGrid).toEqual({ visible: true, divisions: 9, columns: 'letters-no-i', rowsFrom: 'top' });
    expect(migrated.useTerrainHeight).toBe(true);
    expect(migrated.gameGridSize).toBeNull();
    // Potwierdzone wpisy społeczności oznaczają wcześniejszą akceptację źródła.
    expect(migrated.catalogSourceAccepted).toBe(true);
    expect(migrated.maps).toHaveLength(1);
  });
  it('rozpoznaje aktywną mapę z katalogu i własny obraz', () => {
    const { mapSource: _a, ...old } = defaultSettings;
    expect(load({ ...old, communityMapId: 'normandy', maps: [entry('normandy')], mapPath: 'C:/cache/normandy.png', mapWidth: 1024, mapHeight: 1024 }).mapSource).toBe('catalog');
    expect(load({ ...old, mapPath: 'C:/moja.png' }).mapSource).toBe('custom');
    expect(load({ ...old, schemaVersion: 2 }).catalogSourceAccepted).toBe(false);
  });
  it('naprawia uszkodzone wartości zamiast blokować pozostałe ustawienia', () => {
    const fixed = load({ ...defaultSettings, mapSource: 'x', mapGrid: { divisions: 99, columns: 'runes', visible: 'tak' }, gameGridSize: -4, useTerrainHeight: 'no' });
    expect(fixed.mapSource).toBe('catalog');
    expect(fixed.mapGrid).toEqual(defaultSettings.mapGrid);
    expect(fixed.gameGridSize).toBeNull();
    expect(fixed.useTerrainHeight).toBe(true);
    expect(load({ ...defaultSettings, catalogSourceAccepted: false, maps: [entry('normandy')] }).catalogSourceAccepted).toBe(false);
  });
});

describe('katalog map', () => {
  const maps = [known('dustbowl', 'Dustbowl'), known('gensokyo', 'Gensokyo'), known('normandy', 'Normandy Bocage'), known('arctic_airbase', 'Arctic Airbase')];
  it('pokazuje najpierw mapy w rotacji i wyszukuje bez wielkości liter', () => {
    expect(filterCatalog(maps, '').map(m => m.id)).toEqual(['arctic_airbase', 'normandy', 'dustbowl', 'gensokyo']);
    expect(filterCatalog(maps, 'NORMANDY').map(m => m.id)).toEqual(['normandy']);
    expect(filterCatalog(maps, 'bocage').map(m => m.id)).toEqual(['normandy']);
  });
  it('rozpoznaje wpis katalogu i odrzuca starą rozciągniętą heightmapę', () => {
    expect(isCatalogEntry(entry('normandy'), known('normandy', 'Normandy Bocage'))).toBe(true);
    expect(isCatalogEntry({ ...entry('normandy'), calibration: { ...entry('normandy').calibration, northRotationDeg: 15 } }, known('normandy', 'N'))).toBe(false);
    expect(isCatalogEntry(entry('normandy', 3000), known('normandy', 'N'))).toBe(false);
    expect(isCurrentTerrain(terrain('normandy', 'Community x; RED8 normalizedXY; source y'), entry('normandy'))).toBe(false);
    expect(isCurrentTerrain(terrain('normandy', `Community x; RED8 normalizedXY; ${TERRAIN_PROCESSING_V2}:1024; source y`), entry('normandy'))).toBe(true);
    expect(isCurrentTerrain(terrain('normandy', 'Własny pomiar'), entry('normandy'))).toBe(true);
  });
  it('instalacja mapy ustawia skalę, północ, metry gry i usuwa nieaktualny teren', () => {
    const before: Settings = validateSettings({ ...defaultSettings, maps: [entry('normandy')], terrains: [terrain('normandy', 'Community stary')], own: { x: 1, y: 1 } });
    const next = applyCatalogMap(before, known('normandy', 'Normandy Bocage'), entry('normandy'), null);
    expect(validateSettings(next)).toBeTruthy();
    expect(next.scale).toBe(2);
    expect(next.rotation).toBe(0);
    expect(next.mapUnit).toBe('game_meters');
    expect(next.terrains).toEqual([]);
    expect(next.own).toBeNull();
    const again = applyCatalogMap({ ...next, own: { x: 5, y: 5 } }, known('normandy', 'Normandy Bocage'), entry('normandy'), null);
    expect(again.own).toEqual({ x: 5, y: 5 });
  });
});
