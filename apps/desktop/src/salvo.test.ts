import { describe, expect, it } from 'vitest';
import community from '../../../data/weapons/community-profiles.json';
import type { Profile } from './types';
import { ellipseOnMap, offsetToPixels, outlinePath, prepareLocalSalvo, previewScale, validateSalvo, type DispersionEllipse, type SalvoPattern } from './salvo';

const profile: Profile = {
  schemaVersion: 1, id: 'synthetic-salvo', name: 'Test syntetyczny', category: 'rocket_artillery',
  gameVersion: 'syntetyczna', status: 'beta', source: 'Test syntetyczny; nie MTC', calibrated: true,
  speed: 100, gravity: 10, minElevationDeg: 0, maxElevationDeg: 89, dispersionSigma: null, salvoSize: 12,
  notes: '', unit: 'game_units',
};
const wiki = (community as unknown as (Profile & { salvo?: unknown })[]).find(p => p.id === 'community-bm_21_grad-1')!;

describe('rozrzut salwy — jawne dane', () => {
  it('akceptuje cytowaną wartość 9M22 bez dopisywania rozrzutu prędkości', () => {
    const salvo = validateSalvo(wiki.salvo, wiki)!;
    expect(salvo.angularSigmaMrad).toBe(1.75);
    expect(salvo.speedSigma).toBeNull();
    expect(salvo.status).toBe('experimental');
    expect(salvo.calibrated).toBe(false);
    expect(salvo.source).toMatch(/multicrew\.wiki/);
  });
  it('odrzuca brak źródła, brak składowych, złą jednostkę, nieznane pole i fałszywą kalibrację', () => {
    const base = { unit: 'game_units', angularSigmaMrad: 2, speedSigma: null, rippleInterval: null, calibrated: false, status: 'experimental', source: 'pomiar' };
    expect(validateSalvo(base, profile)?.angularSigmaMrad).toBe(2);
    for (const patch of [{ source: ' ' }, { angularSigmaMrad: null }, { unit: 'game_meters' }, { invented: 1 }, { calibrated: true }, { status: 'verified' }, { angularSigmaMrad: -1 }, { angularSigmaMrad: '2' }, { rippleInterval: 601 }]) {
      expect(() => validateSalvo({ ...base, ...patch }, profile)).toThrow();
    }
    expect(validateSalvo({ unit: 'game_units', status: 'no_data' }, profile)?.status).toBe('no_data');
  });
  it('lokalny wpis jest beta tylko dla lokalnie skalibrowanego profilu i wymaga potwierdzenia', () => {
    const draft = { angularSigmaMrad: 3, speedSigma: 0.5, rippleInterval: 0.5, source: 'Pomiar 12 trafień', confirmed: true };
    expect(prepareLocalSalvo(profile, draft).status).toBe('beta');
    expect(prepareLocalSalvo({ ...profile, calibrated: false, status: 'experimental' }, draft).status).toBe('experimental');
    expect(() => prepareLocalSalvo(profile, { ...draft, confirmed: false })).toThrow(/Potwierdź/);
    expect(() => prepareLocalSalvo(profile, { ...draft, angularSigmaMrad: null, speedSigma: null })).toThrow(/Brak danych/);
    expect(() => prepareLocalSalvo({ ...profile, category: 'guided_missile' }, draft)).toThrow(/naprowadzania/);
  });
});

describe('geometria elipsy na mapie', () => {
  const ellipse: DispersionEllipse = { probability: 0.95, centerOffset: [10, 0], semiMajor: 40, semiMinor: 10, majorAxisAzimuthDeg: 0, outline: [] };
  it('odwraca transformację kalibracji mapy (północ do góry i obrócona)', () => {
    expect(offsetToPixels([10, 20], { unitsPerPixel: 2, northRotationDeg: 0 })).toEqual([5, -10]);
    const [dx, dy] = offsetToPixels([1, 0], { unitsPerPixel: 1, northRotationDeg: 90 });
    expect(dx).toBeCloseTo(0, 12); expect(dy).toBeCloseTo(-1, 12);
  });
  it('podaje środek, półosie w pikselach i obrót SVG', () => {
    const svg = ellipseOnMap(ellipse, { x: 100, y: 200 }, { unitsPerPixel: 2, northRotationDeg: 0 });
    expect(svg).toEqual({ cx: 105, cy: 200, rx: 20, ry: 5, rotationDeg: -90 });
    expect(ellipseOnMap({ ...ellipse, majorAxisAzimuthDeg: 90 }, { x: 0, y: 0 }, { unitsPerPixel: 1, northRotationDeg: 30 }).rotationDeg).toBe(-30);
  });
  it('skaluje podgląd do obwiedni i rysuje zamknięty kontur z północą do góry', () => {
    const pattern = { singleProjectile: { ...ellipse, outline: [[0, 50], [10, 0]] }, salvoEnvelope: { ...ellipse, outline: [[0, 100]] }, coverage: null } as unknown as SalvoPattern;
    const scale = previewScale(pattern, 200);
    expect(scale).toBeCloseTo(0.76, 10);
    expect(outlinePath([[0, 100], [10, 0]], scale, 200)).toBe('M100.00 24.00 L107.60 100.00 Z');
  });
});
