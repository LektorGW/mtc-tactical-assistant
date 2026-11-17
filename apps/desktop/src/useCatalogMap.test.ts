import { describe, expect, it, vi } from 'vitest';
import { invalidateCatalogLoads, loadCatalogMap } from './useCatalogMap';
import { defaultSettings, type CommunityMap, type Settings } from './types';

const known: CommunityMap = { schemaVersion: 1, id: 'normandy', name: 'Normandy Bocage', key: 'Normandy Bocage', sizeStuds: 5976, sizeGameMeters: 2151.36, unit: 'game_meters', status: 'experimental', inRotation: true, heightRangeStuds: null,
  sourceMetadata: { repository: 'https://example.invalid', commit: 'a'.repeat(40), fetchedAt: 'test', license: 'MIT', urls: ['https://example.invalid/x'] } };
const map = { sha256: 'a'.repeat(64), id: 'normandy', name: 'Normandy Bocage', path: 'C:/cache/n.png', width: 1024, height: 1024, unit: 'game_meters' as const, source: 'Community x', confirmed: true,
  calibration: { unitsPerPixel: 2151.36 / 1024, northRotationDeg: 0, originPixel: { x: 0, y: 0 }, originWorld: { x: 0, y: 0 }, uncertaintyUnits: 0 } };

function setup() {
  let settings: Settings = { ...defaultSettings, catalogSourceAccepted: true };
  let release!: () => void;
  const gate = new Promise<void>(resolve => { release = resolve; });
  const call = vi.fn(async (command: string) => {
    if (command === 'prepare_catalog_map') { await gate; return { map, terrain: null, terrainInSource: false, origin: 'downloaded', sizeGameMeters: 2151.36, warnings: [] }; }
    if (command === 'import_image') return { dataUrl: 'data:', width: 1024, height: 1024, sha256: map.sha256 };
    throw new Error(command);
  }) as unknown as <T>(command: string) => Promise<T>;
  const setSettings = vi.fn((next: (s: Settings) => Settings) => { settings = next(settings); });
  const showImage = vi.fn();
  return { release, deps: { settings: () => settings, setSettings, showImage, log: () => {}, call }, get settings() { return settings; } };
}

describe('wczytywanie mapy z katalogu', () => {
  it('ustawia mapę, skalę i północ po zakończeniu pobierania', async () => {
    const t = setup();
    const pending = loadCatalogMap(known, t.deps);
    t.release();
    expect((await pending)?.phase).toBe('ready');
    expect(t.settings.scale).toBeCloseTo(2151.36 / 1024, 12);
    expect(t.settings.mapSource).toBe('catalog');
  });
  it('nowsza zmiana mapy (import, symulacja, zmiana źródła) wygrywa z wolnym pobieraniem', async () => {
    const t = setup();
    const pending = loadCatalogMap(known, t.deps);
    invalidateCatalogLoads(); // e.g. own image imported meanwhile
    t.release();
    expect(await pending).toBeNull();
    expect(t.deps.setSettings).not.toHaveBeenCalled();
    expect(t.deps.showImage).not.toHaveBeenCalled();
  });
  it('wymaga zgody na źródło', async () => {
    const t = setup();
    await expect(loadCatalogMap(known, { ...t.deps, settings: () => ({ ...defaultSettings }) })).rejects.toThrow(/potwierdź/);
  });
});
