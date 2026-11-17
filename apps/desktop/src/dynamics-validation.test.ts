import { describe, expect, it } from 'vitest';
import type { Profile, ProjectileDynamics } from './types';
import { prepareLocalDynamics, validateDynamics, type DynamicsDraft } from './dynamics-validation';

const profile: Profile = {
  schemaVersion: 1, id: 'synthetic-dynamics', name: 'Test syntetyczny', category: 'tube_artillery',
  gameVersion: 'syntetyczna', status: 'beta', source: 'Test syntetyczny v100 g10; nie MTC',
  calibrated: true, speed: 100, gravity: 10, minElevationDeg: -10, maxElevationDeg: 89,
  dispersionSigma: null, salvoSize: 1, notes: '', unit: 'game_units', sourceAcknowledged: false,
};
const drag: ProjectileDynamics = {
  model: 'drag', unit: 'game_units', linearDrag: 0, quadraticDrag: 0,
  rocketAcceleration: null, burnTime: null, thrustDirectionFixedLaunch: null,
  calibrated: true, status: 'beta', source: 'TEST SYNTHETIC: jawne współczynniki; nie MTC',
};
const draft: DynamicsDraft = {
  model: 'drag', linearDrag: 0, quadraticDrag: 0, rocketAcceleration: null,
  burnTime: null, thrustDirectionFixedLaunch: false, source: drag.source,
  localCalibrationConfirmed: true,
};

describe('jawne lokalne parametry oporu i napędu', () => {
  it('zachowuje legacy vacuum i rozróżnia brak współczynnika od potwierdzonego zera', () => {
    expect(validateDynamics(undefined, profile)).toBeNull();
    expect(validateDynamics(null, profile)).toBeNull();
    expect(validateDynamics(drag, profile)?.linearDrag).toBe(0);
    expect(() => validateDynamics({ ...drag, linearDrag: null }, profile)).toThrow(/oba współczynniki/);
    const unavailable = validateDynamics({ model: 'drag', unit: 'game_units' }, profile);
    expect(unavailable?.calibrated).toBe(false);
    expect(unavailable?.linearDrag).toBeNull();
  });

  it('odrzuca błędne typy, wartości ujemne i nieskończone zamiast konwertować tekst na liczby', () => {
    for (const value of [-1, NaN, Infinity, '0', false]) {
      expect(() => validateDynamics({ ...drag, quadraticDrag: value }, profile)).toThrow();
    }
    for (const patch of [{ calibrated: 'true' }, { source: 123 }, { unit: null }, { model: 'guided' }, { thrustDirectionFixedLaunch: 'true' }, { inventedCoefficient: 0 }]) {
      expect(() => validateDynamics({ ...drag, ...patch }, profile)).toThrow();
    }
    expect(() => validateDynamics([], profile)).toThrow();
  });

  it('wymaga spójnych jednostek i źródła ze statusem beta bez deklaracji verified', () => {
    expect(() => validateDynamics({ ...drag, unit: 'game_meters' }, profile)).toThrow(/Jednostka/);
    expect(() => validateDynamics({ ...drag, source: ' ' }, profile)).toThrow(/źródła/);
    expect(() => validateDynamics({ ...drag, status: 'verified' }, profile)).toThrow();
    expect(() => validateDynamics({ ...drag, source: 'ą'.repeat(1025) }, profile)).toThrow();
  });

  it('przygotowuje beta tylko po jawnym potwierdzeniu i nie modyfikuje źródła profilu', () => {
    const original = structuredClone(profile);
    const result = prepareLocalDynamics(profile, draft);
    expect(result?.status).toBe('beta');
    expect(result?.calibrated).toBe(true);
    expect(profile).toEqual(original);
    expect(() => prepareLocalDynamics(profile, { ...draft, localCalibrationConfirmed: false })).toThrow(/Potwierdź/);
    expect(() => prepareLocalDynamics(profile, { ...draft, linearDrag: null })).toThrow();
    expect(prepareLocalDynamics(profile, { ...draft, model: 'vacuum' })).toBeNull();
  });

  it('nie traktuje community acknowledgement lub verified jako lokalnej kalibracji beta', () => {
    expect(() => prepareLocalDynamics({ ...profile, calibrated: false, sourceAcknowledged: true }, draft)).toThrow(/kalibrację beta/);
    expect(() => prepareLocalDynamics({ ...profile, status: 'verified' }, draft)).toThrow(/kalibrację beta/);
    expect(() => prepareLocalDynamics({ ...profile, speed: null }, draft)).toThrow();
    expect(() => prepareLocalDynamics({ ...profile, gravity: NaN }, draft)).toThrow();
    expect(() => prepareLocalDynamics({ ...profile, calibrated: 'true' } as unknown as Profile, draft)).toThrow();
    expect(() => prepareLocalDynamics(undefined, draft)).toThrow(/Wybierz/);
  });

  it('rakieta wymaga wszystkich pomiarów i jawnego założenia stałego kierunku napędu', () => {
    const rocket: DynamicsDraft = { ...draft, model: 'rocket', rocketAcceleration: 20, burnTime: 1.27, thrustDirectionFixedLaunch: true };
    expect(prepareLocalDynamics(profile, rocket)?.burnTime).toBe(1.27);
    expect(prepareLocalDynamics(profile, { ...rocket, rocketAcceleration: 0 })?.rocketAcceleration).toBe(0);
    for (const patch of [{ rocketAcceleration: null }, { burnTime: null }, { burnTime: 0 }, { thrustDirectionFixedLaunch: false }, { source: '' }]) {
      expect(() => prepareLocalDynamics(profile, { ...rocket, ...patch })).toThrow();
    }
  });

  it('nie ignoruje aktywnych parametrów po zmianie modelu i odmawia fikcyjnego naprowadzania', () => {
    expect(() => validateDynamics({ ...drag, model: 'vacuum' }, profile)).toThrow(/ignorować/);
    expect(() => validateDynamics({ ...drag, rocketAcceleration: 10 }, profile)).toThrow(/ignorować/);
    const guided = { ...profile, category: 'guided_missile' as const };
    expect(() => validateDynamics(drag, guided)).toThrow(/naprowadzania/);
    expect(() => prepareLocalDynamics(guided, draft)).toThrow(/naprowadzania/);
  });
});
