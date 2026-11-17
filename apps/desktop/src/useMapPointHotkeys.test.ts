import { describe, expect, it } from 'vitest';
import { planMapPoint, type MapPointPicked } from './useMapPointHotkeys';

const maps = [{ id: 'zone_11', name: 'Zone 11' }, { id: 'japan', name: 'Japan' }, { id: 'gensokyo', name: 'Gensokyo' }];
const settings = { communityMapId: 'zone_11', mapSource: 'catalog' as const, mapWidth: 1024, mapHeight: 1024 };
const event = (patch: Partial<MapPointPicked> = {}): MapPointPicked => ({
  role: 'target', ok: true, error: null, fraction: { x: 0.25, y: 0.75 }, world: { x: 729.6, y: 729.6 }, sector: 'C7',
  mapId: 'zone_11', mapStatus: 'ready', candidates: ['zone_11'], gridSizeMeters: 324, timestampMs: 1, cursor: { x: 800, y: 700 }, ...patch,
});

describe('wskazanie punktu na mapie w grze', () => {
  it('przelicza ułamek na piksel tekstury bieżącej mapy katalogu', () => {
    expect(planMapPoint(event(), settings, maps)).toEqual({ kind: 'apply', role: 'target', texture: { x: 256, y: 768 }, sector: 'C7', note: null });
  });
  it('wczytuje inną rozpoznaną mapę przed ustawieniem punktu', () => {
    expect(planMapPoint(event({ mapId: 'japan', role: 'own' }), settings, maps)).toMatchObject({ kind: 'load', mapId: 'japan', role: 'own' });
  });
  it('nie ustawia punktu przy zamkniętej mapie, kursorze poza siatką lub niejednoznacznej mapie', () => {
    expect(planMapPoint(event({ ok: false, error: 'otwórz mapę klawiszem M', fraction: null, sector: null }), settings, maps)).toEqual({ kind: 'error', message: 'otwórz mapę klawiszem M' });
    expect(planMapPoint(event({ mapId: null, mapStatus: 'ambiguous', candidates: ['japan', 'gensokyo'] }), settings, maps).kind).toBe('error');
    expect(planMapPoint(event({ mapId: null, mapStatus: 'ambiguous', candidates: ['japan', 'gensokyo'] }), { ...settings, communityMapId: 'japan' }, maps)).toMatchObject({ kind: 'apply' });
    expect(planMapPoint(event({ mapId: 'unknown_map' }), settings, maps).kind).toBe('error');
  });
  it('siatkę bez podpisu (minimapa) przyjmuje tylko dla już wczytanej mapy, z adnotacją', () => {
    const unlabeled = event({ mapId: null, mapStatus: 'unidentified', candidates: [] });
    const plan = planMapPoint(unlabeled, settings, maps);
    expect(plan.kind).toBe('apply');
    expect(plan.kind === 'apply' && plan.note).toMatch(/Zone 11/);
    expect(planMapPoint(unlabeled, { ...settings, mapSource: 'custom' }, maps).kind).toBe('error');
  });
});
