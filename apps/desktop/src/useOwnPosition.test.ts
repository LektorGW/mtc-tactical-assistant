import { describe, expect, it } from 'vitest';
import { ownPositionDecision, type OwnPositionReading } from './useOwnPosition';

const settings = { mapSource: 'catalog' as const, communityMapId: 'rohkstov', mapWidth: 1024, mapHeight: 1024, own: null };
const reading = (patch: Partial<OwnPositionReading> = {}, confidence = 0.9): OwnPositionReading => ({
  schemaVersion: 1, status: 'ready', timestampMs: 1, frameWidth: 1920, frameHeight: 1080, mapId: 'rohkstov',
  marker: { pixel: { x: 193, y: 888 }, fraction: { x: 0.5, y: 0.25 }, sector: 'E3', world: { x: 2699, y: 4049 }, headingDeg: 175, shapeScore: 0.86, compassDeg: 175, compassAgrees: true, confidence },
  candidates: [], warnings: [], ...patch,
});

describe('automatyczna własna pozycja', () => {
  it('ustawia działo dopiero po potwierdzonej, ustabilizowanej klatce', () => {
    expect(ownPositionDecision(reading(), settings).apply).toEqual({ x: 512, y: 256 });
    expect(ownPositionDecision(reading({}, 0.6), settings).apply).toBeNull();
    expect(ownPositionDecision(reading({ status: 'unconfirmed' }), settings).apply).toBeNull();
    expect(ownPositionDecision(reading({ marker: null, status: 'notFound' }), settings).apply).toBeNull();
  });
  it('wymaga wczytanej tej samej mapy z katalogu', () => {
    expect(ownPositionDecision(reading({ mapId: 'zone_11' }), settings).apply).toBeNull();
    expect(ownPositionDecision(reading(), { ...settings, mapSource: 'custom' }).apply).toBeNull();
  });
  it('nie przepisuje ustawień przy drganiach poniżej pół piksela', () => {
    expect(ownPositionDecision(reading(), { ...settings, own: { x: 512.2, y: 256.1 } }).apply).toBeNull();
  });
});
