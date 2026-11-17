import { describe, expect, it } from 'vitest';
import { CALCULATOR_GRAVITY, DEFAULT_PRESETS, PGO7_ASSUMED, createDirectFireProfile, parseRanges, parseReticle, quickLine, stepPreset, validateObservedSpecs, type QuickAim } from './direct-fire';
import { validateProfile } from './validation';

const base: QuickAim = { range: 800, pointBlankRange: 480, centerAim: false, reachable: true, dropBelowLineOfSight: 4.21, holdOverMrad: 2.63, elevationMrad: 3, timeOfFlight: 0.93, reticle: null, model: 'vacuum', status: 'experimental', warnings: [] };
const draft = { name: 'RPG-7V2 / PG-7VR', velocity: 120, gravity: CALCULATOR_GRAVITY, screen: 'loadout' as const, observedAt: '2026-10-16T10:00:00.000Z', weaponName: 'RPG-7V2', acknowledged: true, reticle: null };

describe('linia trybu szybkiego', () => {
  it('formatuje poprawkę, czas i źródło dystansu po polsku', () => {
    expect(quickLine(base, { rangefinder: false, missingSpeed: false })).toBe('800 m · PODNIEŚ 4,2 m (≈2,6 mrad) · t 0,9 s');
    expect(quickLine(base, { rangefinder: true, missingSpeed: false })).toMatch(/z dalmierza gry$/);
    expect(quickLine({ ...base, dropBelowLineOfSight: -0.6, holdOverMrad: -0.4, centerAim: false }, { rangefinder: false, missingSpeed: false })).toMatch(/OBNIŻ 0,6 m/);
  });
  it('próg celowania w środek pochodzi z danych (zasięg strzału bezpośredniego)', () => {
    expect(quickLine({ ...base, range: null, centerAim: true }, { rangefinder: false, missingSpeed: false })).toBe('< 480 m · CELUJ W ŚRODEK');
    expect(quickLine({ ...base, range: 300, centerAim: true }, { rangefinder: false, missingSpeed: false })).toMatch(/^300 m · CELUJ W ŚRODEK/);
  });
  it('pokazuje kreskę celownika z resztą poprawki', () => {
    const reticle = { mark: '3', markRange: 300, residualDrop: 0.42, residualMrad: 1.3, status: 'unverified' };
    expect(quickLine({ ...base, range: 300, reticle }, { rangefinder: false, missingSpeed: false })).toBe('300 m → kreska 3 (+0,4 m) · t 0,9 s');
  });
  it('bez prędkości wylotowej żąda odczytu Loadout zamiast liczyć', () => {
    expect(quickLine(base, { rangefinder: false, missingSpeed: true })).toBe('brak danych pocisku — odczytaj z Loadout');
    expect(quickLine({ ...base, reachable: false }, { rangefinder: false, missingSpeed: false })).toMatch(/poza zasięgiem/);
  });
});

describe('presety i formularze', () => {
  it('Numpad +/−/0 przechodzi po presetach z resetem do bliskiego zasięgu', () => {
    expect(stepPreset(-1, 'next', DEFAULT_PRESETS.length)).toBe(0);
    expect(stepPreset(DEFAULT_PRESETS.length - 1, 'next', DEFAULT_PRESETS.length)).toBe(DEFAULT_PRESETS.length - 1);
    expect(stepPreset(0, 'previous', DEFAULT_PRESETS.length)).toBe(-1);
    expect(stepPreset(-1, 'previous', DEFAULT_PRESETS.length)).toBe(-1);
    expect(stepPreset(5, 'close', DEFAULT_PRESETS.length)).toBe(-1);
  });
  it('parsuje zakres i listę dystansów oraz kreski celownika', () => {
    expect(parseRanges('50-800/50')).toHaveLength(16);
    expect(parseRanges('400, 200 200;100')).toEqual([100, 200, 400]);
    expect(() => parseRanges('0, -5')).toThrow();
    expect(() => parseRanges('1-1000/1')).toThrow();
    expect(parseReticle('2=200, 3=300')).toEqual(PGO7_ASSUMED.marks.slice(0, 2));
    expect(() => parseReticle('trzy')).toThrow();
  });
  it('tworzy eksperymentalny profil z jawnymi założeniami, zgodny z walidacją Rust/TS', () => {
    const p = createDirectFireProfile({ ...draft, reticle: PGO7_ASSUMED.marks }, 'direct-test');
    expect(p.category).toBe('direct_fire'); expect(p.status).toBe('experimental'); expect(p.calibrated).toBe(false);
    expect(p.source).toMatch(/^odczytane z UI gry 2026-10-16/);
    expect(p.observedSpecs?.gravitySource).toMatch(/17\.64.*broni ręcznej/);
    expect(p.reticle?.status).toBe('unverified');
    expect(validateProfile(JSON.parse(JSON.stringify(p))).id).toBe('direct-test');
    expect(() => createDirectFireProfile({ ...draft, acknowledged: false })).toThrow(/założenia/);
    expect(() => createDirectFireProfile({ ...draft, velocity: null })).toThrow();
  });
  it('odrzuca niespójną prędkość odczytu i nieznane pola', () => {
    const p = createDirectFireProfile(draft, 'direct-test');
    expect(() => validateProfile({ ...p, speed: 130 })).toThrow(/odczytanej/);
    expect(validateProfile({ ...p, speed: 130, calibrated: true, status: 'beta' }).speed).toBe(130);
    expect(() => validateObservedSpecs({ ...p.observedSpecs, guessed: 1 }, 120, false)).toThrow(/Nieznane/);
    expect(() => validateProfile({ ...p, reticle: { ...PGO7_ASSUMED, marks: [{ mark: '3', range: 300 }, { mark: '2', range: 200 }] } })).toThrow(/rosnące/);
  });
});
