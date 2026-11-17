import { describe, expect, it } from 'vitest';
import { emptyTracker, holdPositions, respawnGap, trackAutoMap, type AutoMapTracker } from './auto-map';

/** Replays a sequence of observations and returns the active map after each one. */
function replay(start: string | null, observations: { mapId: string | null; timestampMs: number }[]) {
  let current = start;
  let tracker: AutoMapTracker = emptyTracker;
  const history: (string | null)[] = [];
  for (const observation of observations) {
    const decision = trackAutoMap(tracker, observation, current);
    tracker = decision.tracker;
    if (decision.switchTo) current = decision.switchTo;
    history.push(current);
  }
  return history;
}

describe('mapa po śmierci i odrodzeniu', () => {
  it('rozpoznana mapa zostaje przy serii pustych obserwacji (ekran śmierci, loadout)', () => {
    const history = replay('rohkstov', [
      { mapId: 'rohkstov', timestampMs: 1000 },
      ...Array.from({ length: 20 }, (_, i) => ({ mapId: null, timestampMs: 2000 + i * 500 })),
      { mapId: 'rohkstov', timestampMs: 20_000 },
    ]);
    expect(history.every(id => id === 'rohkstov')).toBe(true);
  });
  it('pojedyncze lub przerywane rozpoznanie innej mapy (mała mapa spawnu) jej nie podmienia', () => {
    expect(replay('rohkstov', [
      { mapId: 'zone_11', timestampMs: 1000 }, { mapId: null, timestampMs: 1500 },
      { mapId: 'zone_11', timestampMs: 2000 }, { mapId: 'sokolovka', timestampMs: 2500 },
      { mapId: 'zone_11', timestampMs: 3000 }, { mapId: 'zone_11', timestampMs: 9000 },
    ]).at(-1)).toBe('rohkstov');
    // The same frame delivered twice is one observation.
    expect(replay('rohkstov', [{ mapId: 'zone_11', timestampMs: 1000 }, { mapId: 'zone_11', timestampMs: 1000 }]).at(-1)).toBe('rohkstov');
  });
  it('przełącza dopiero po dwóch kolejnych zgodnych rozpoznaniach innej mapy', () => {
    expect(replay('rohkstov', [{ mapId: 'zone_11', timestampMs: 1000 }, { mapId: 'zone_11', timestampMs: 1500 }])).toEqual(['rohkstov', 'zone_11']);
    expect(replay(null, [{ mapId: 'zone_11', timestampMs: 1000 }, { mapId: 'zone_11', timestampMs: 1500 }]).at(-1)).toBe('zone_11');
  });
});

describe('pozycja po odrodzeniu', () => {
  it('przerwa w odczycie panelu > 10 s oznacza możliwą nieaktualną pozycję', () => {
    expect(respawnGap(1000, 11_001)).toBe(true);
    expect(respawnGap(1000, 11_000)).toBe(false);
    expect(respawnGap(null, 50_000)).toBe(false);
  });
  it('Automatic: znikające markery nie kasują ostatnich znanych pozycji', () => {
    let held = holdPositions({ own: null, target: null, timestampMs: 0 }, { x: 1, y: 2 }, { x: 3, y: 4 }, 100);
    held = holdPositions(held, null, null, 200);
    expect(held).toEqual({ own: { x: 1, y: 2 }, target: { x: 3, y: 4 }, timestampMs: 100 });
    held = holdPositions(held, { x: 5, y: 6 }, null, 300);
    expect(held.own).toEqual({ x: 5, y: 6 });
    expect(held.target).toEqual({ x: 3, y: 4 });
  });
});
