import { describe, expect, it } from 'vitest';
import { currentSalvo, salvoLayerFor, salvoScopeKey } from './map-salvo';
import type { SalvoPattern } from './salvo';

const ellipse = (semiMajor: number, semiMinor: number, azimuth: number, center: [number, number] = [0, 0]) => ({ probability: 0.95, centerOffset: center, semiMajor, semiMinor, majorAxisAzimuthDeg: azimuth, outline: [] });
const pattern = (unit: 'game_meters' | 'game_units' = 'game_meters'): SalvoPattern => ({
  unit, status: 'experimental', salvoSize: 40, dispersionSource: 'launch_dispersion', targetPosition: [0, 0], centerOffset: [10, 20],
  covariance: [[1, 0], [0, 1]], rangeSigma: 1, crossRangeSigma: 1,
  singleProjectile: ellipse(40, 20, 0), salvoEnvelope: ellipse(60, 30, 90, [10, 20]), meanPointOfImpact: ellipse(8, 4, 0, [10, 20]),
  nonlinearity: 0, coverage: null, timing: null, missingComponents: ['wiatr'], assumptions: [],
});

describe('elipsy salwy na mapie', () => {
  it('przelicza środek [wschód, północ] na piksele z osią Y w dół', () => {
    const { layer } = salvoLayerFor(pattern(), { x: 500, y: 500 }, 2, 0, 'game_meters');
    expect(layer!.center).toEqual({ x: 505, y: 490 });
    const salvo = layer!.ellipses.find(e => e.key === 'salvo')!;
    expect([salvo.cx, salvo.cy, salvo.rx, salvo.ry]).toEqual([505, 490, 30, 15]);
    // Oś wielka na azymucie 90° (wschód) leży poziomo w SVG.
    expect(salvo.rotationDeg).toBe(0);
    expect(layer!.label).toContain('niepełny');
  });
  it('nie rysuje elips przy innych jednostkach, braku celu lub skali', () => {
    expect(salvoLayerFor(pattern('game_units'), { x: 1, y: 1 }, 2, 0, 'game_meters').layer).toBeNull();
    expect(salvoLayerFor(pattern(), null, 2, 0, 'game_meters').reason).toMatch(/celu/);
    expect(salvoLayerFor(pattern(), { x: 1, y: 1 }, null, 0, 'game_meters').layer).toBeNull();
    expect(salvoLayerFor(null, { x: 1, y: 1 }, 2, 0, 'game_meters')).toEqual({ layer: null, reason: '' });
  });
});

describe('aktualność prognozy salwy', () => {
  const base = { profileId: 'p', profileSalvo: null, profileSpeed: 180, profileGravity: 17.64, salvoSize: 40, calibration: { unitsPerPixel: 2, northRotationDeg: 0 }, mapPath: 'm.png', own: { x: 1, y: 1 }, target: { x: 50, y: 50 }, heightDelta: 2, arcs: [{ elevationDeg: 10, flightTime: 3 }] };
  it('odrzuca prognozę po zmianie celu, profilu, mapy, wysokości lub toru', () => {
    const forecast = { pattern: pattern(), scope: salvoScopeKey(base) };
    expect(currentSalvo(forecast, salvoScopeKey(base))).toBe(forecast.pattern);
    for (const change of [{ target: { x: 51, y: 50 } }, { profileId: 'q' }, { mapPath: 'inna.png' }, { heightDelta: 3 }, { arcs: [{ elevationDeg: 11, flightTime: 3 }] }, { calibration: { unitsPerPixel: 2, northRotationDeg: 5 } }]) {
      expect(currentSalvo(forecast, salvoScopeKey({ ...base, ...change }))).toBeNull();
    }
    expect(currentSalvo(null, salvoScopeKey(base))).toBeNull();
  });
});
