import { describe, expect, it } from 'vitest';
import { acceptResourceProgress, buildResourceRequest, expectedResourceItems, validateResourceCatalog, type KnownMapResource } from './resource-types';

const fixture = (id: string, height: boolean): KnownMapResource => ({
  schemaVersion: 1, id, key: id, name: `TEST SYNTHETIC ${id}`, unit: 'game_meters', status: 'experimental',
  mapUrl: `https://example.invalid/${id}/map.png`, heightmapUrl: height ? `https://example.invalid/${id}/heightmap.png` : null,
  sizeGameMeters: 100, heightRangeGameMeters: height ? { minimum: -10, maximum: 50 } : null,
  sourceMetadata: { repository: 'https://example.invalid/test', commit: 'a'.repeat(40), urls: ['https://example.invalid/test/source'], codeLicense: 'test', rasterLicense: 'unconfirmed', redistributionAllowed: false, alignment: 'test' },
});
const catalog = [fixture('one', true), fixture('two', false)];

describe('jawny wybór lokalnych zasobów map', () => {
  it('odrzuca niekompletny katalog, powtórzone identyfikatory i brak pochodzenia', () => {
    expect(validateResourceCatalog(catalog)).toEqual(catalog);
    for (const value of [[], [catalog[0], catalog[0]], [{ ...catalog[0], sourceMetadata: null }], [{ ...catalog[0], sizeGameMeters: NaN }], [{ ...catalog[0], unit: 'game_units' }], [{ ...catalog[0], heightmapUrl: null }]]) {
      expect(() => validateResourceCatalog(value)).toThrow();
    }
  });
  it('wymaga zgody źródłowej oraz znanych niepowtórzonych map i nie wysyła adresów użytkownika', () => {
    expect(() => buildResourceRequest(catalog, ['one'], false, false, false)).toThrow(/Potwierdź/);
    for (const ids of [[], ['unknown'], ['one', 'one']]) expect(() => buildResourceRequest(catalog, ids, false, false, true)).toThrow();
    const request = buildResourceRequest(catalog, ['two', 'one'], true, false, true);
    expect(request).toEqual({ ids: ['one', 'two'], includeTerrain: true, online: false, sourceAcknowledged: true });
    expect(Object.keys(request)).not.toContain('url');
  });
  it('obsługuje wybór wszystkich 25 i liczy tylko teren występujący w źródle', () => {
    const all = Array.from({ length: 25 }, (_, i) => fixture(`map_${i}`, i % 2 === 0));
    const request = buildResourceRequest(all, all.map(map => map.id), true, true, true);
    expect(request.ids).toHaveLength(25);
    expect(expectedResourceItems(all, request)).toBe(38);
    expect(expectedResourceItems(all, { ...request, includeTerrain: false })).toBe(25);
  });
  it('postęp respektuje wybór, dostępność terenu, kolejność i limit licznika', () => {
    const request = buildResourceRequest(catalog, ['one', 'two'], true, false, true);
    const first = acceptResourceProgress(null, { completed: 1, id: 'one', kind: 'mapImage', status: 'cached' }, catalog, request);
    expect(first?.completed).toBe(1);
    for (const event of [
      { completed: 1, id: 'one', kind: 'mapImage', status: 'cached' },
      { completed: 2, id: 'unknown', kind: 'mapImage', status: 'downloaded' },
      { completed: 2, id: 'two', kind: 'heightmap', status: 'cached' },
      { completed: 4, id: 'one', kind: 'heightmap', status: 'cached' },
      { completed: 2, id: 'one', kind: 'heightmap', status: 'invented' },
    ]) expect(acceptResourceProgress(first, event, catalog, request)).toEqual(first);
    const missing = acceptResourceProgress(first, { completed: 2, id: 'one', kind: 'heightmap', status: 'missing' }, catalog, request);
    expect(missing?.status).toBe('missing');
    expect(acceptResourceProgress(first, { completed: 2, id: 'one', kind: 'heightmap', status: 'cached' }, catalog, { ...request, includeTerrain: false })).toEqual(first);
  });
});
