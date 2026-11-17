import { describe, expect, it } from 'vitest';
import { aimCorrection, equivalentNames, formatAngle, fractionToTexturePixel, panelFresh, requiredAimFrom, wrapDegrees, type RequiredAim } from './aim-correction';
import type { WeaponPanelReading } from './mtc-hud-types';
import type { Result } from './types';

const now = 1_000_000;
const panel = (elevation: number | null, azimuth: number | null, timestampMs = now - 100): WeaponPanelReading => ({
  schemaVersion: 1, status: 'ready', timestampMs, frameWidth: 1920, frameHeight: 1080, layout: null,
  weapon: { text: 'UB-32', catalogMatch: 'UB-32', confidence: 0.9, attempts: [] }, ammo: null, rangeMeters: null, rangeText: null,
  elevationDeg: elevation === null ? null : { value: elevation, text: String(elevation), confidence: 0.9, templateCost: 0 },
  azimuthDeg: azimuth === null ? null : { value: azimuth, text: String(azimuth), confidence: 0.9, templateCost: 0 }, warnings: [],
});
const required: RequiredAim = { elevationDeg: 12.3, azimuthDeg: 5, flightTime: 4, highArc: false, source: 'solution' };

describe('zawijanie azymutu', () => {
  it('daje przedział (−180°, 180°] i najkrótszy obrót', () => {
    expect(wrapDegrees(350 - 10)).toBe(-20);
    expect(wrapDegrees(10 - 350)).toBe(20);
    expect(wrapDegrees(180)).toBe(180);
    expect(wrapDegrees(-180)).toBe(180);
    expect(wrapDegrees(720 + 45)).toBe(45);
  });
});

describe('korekta celowania', () => {
  it('podnieś/opuść i w prawo/w lewo z polskim zapisem liczb', () => {
    const up = aimCorrection(required, panel(10, 1), now)!;
    expect(up.elevation!.label).toBe('PODNIEŚ +2,30°');
    expect(up.azimuth!.label).toBe('W PRAWO 4,00°');
    const down = aimCorrection(required, panel(13.4, 9.1), now)!;
    expect(down.elevation!.label).toBe('OPUŚĆ −1,10°');
    expect(down.azimuth!.label).toBe('W LEWO 4,10°');
  });
  it('przez północ wybiera krótszy obrót', () => {
    expect(aimCorrection({ ...required, azimuthDeg: 2 }, panel(12.3, 358), now)!.azimuth!.label).toBe('W PRAWO 4,00°');
    expect(aimCorrection({ ...required, azimuthDeg: 358 }, panel(12.3, 2), now)!.azimuth!.label).toBe('W LEWO 4,00°');
  });
  it('NA CELU dopiero, gdy obie osie mieszczą się w progu 0,1°', () => {
    expect(aimCorrection(required, panel(12.25, 5.05), now)!.onTarget).toBe(true);
    expect(aimCorrection(required, panel(12.15, 5.05), now)!.onTarget).toBe(false);
    // Brak odczytu jednej osi nigdy nie jest „na celu”.
    expect(aimCorrection(required, panel(12.3, null), now)!.onTarget).toBe(false);
  });
  it('odczyt starszy niż 2 s, brak rozwiązania lub kątów nie daje korekty', () => {
    expect(aimCorrection(required, panel(10, 1, now - 2001), now)).toBeNull();
    expect(panelFresh({ timestampMs: now - 2000 }, now)).toBe(true);
    expect(panelFresh({ timestampMs: now + 5000 }, now)).toBe(false);
    expect(aimCorrection(null, panel(10, 1), now)).toBeNull();
    expect(aimCorrection(required, panel(null, null), now)).toBeNull();
  });
});

describe('wymagany tor z kalkulatora', () => {
  const result = { bearing: { distance: 800, azimuthDeg: 45, uncertaintyUnits: 0 }, ballisticWarning: null, lead: null, leadWarning: null,
    solution: { arcs: [{ elevationDeg: 12, flightTime: 4, highArc: false }, { elevationDeg: 77, flightTime: 20, highArc: true }], dispersionRadius95: null, assumptions: [] } } as Result;
  it('wybiera tor niski albo wysoki i azymut z namiaru', () => {
    expect(requiredAimFrom(result, false)).toMatchObject({ elevationDeg: 12, azimuthDeg: 45, highArc: false });
    expect(requiredAimFrom(result, true)).toMatchObject({ elevationDeg: 77, highArc: true });
    expect(requiredAimFrom({ ...result, solution: null }, false)).toBeNull();
  });
});

describe('punkt z mapy gry → piksel tekstury katalogu', () => {
  it('ułamek od zachodu/północy skaluje się na całą teksturę', () => {
    expect(fractionToTexturePixel({ x: 0.25, y: 0.5 }, 1024, 1024)).toEqual({ x: 256, y: 512 });
    expect(fractionToTexturePixel({ x: 1, y: 1 }, 1024, 1024)).toEqual({ x: 1023, y: 1023 });
    expect(fractionToTexturePixel({ x: 1.01, y: 0.5 }, 1024, 1024)).toBeNull();
  });
});

describe('format jak w panelu gry', () => {
  it('kąty i korekty mają dwa miejsca po przecinku', () => {
    expect(formatAngle(3.12)).toBe('3,12°');
    expect(formatAngle(-0.5)).toBe('-0,50°');
    expect(aimCorrection({ ...required, elevationDeg: 3.12 }, panel(3.0, 5), now)!.elevation!.label).toBe('PODNIEŚ +0,12°');
  });
});

describe('profile równoważne z resolvera', () => {
  const profiles = [{ id: 'm109', name: 'M109 / M107 Medium Charge' }, { id: 'm109a6', name: 'M109A6 / M107 Medium Charge' }];
  it('pokazuje nazwy równoważnych profili bez wybranego i toleruje brak pola', () => {
    const resolution = { status: 'matched', profileId: 'm109', confidence: 0.9, candidates: [], evidence: [], equivalents: [{ profileId: 'm109', vehicle: 'M109' }, { profileId: 'm109a6', vehicle: 'M109A6' }] };
    expect(equivalentNames(resolution, 'm109', profiles)).toEqual(['M109A6']);
    expect(equivalentNames({ ...resolution, equivalents: ['m109a6'] }, 'm109', profiles)).toEqual(['M109A6 / M107 Medium Charge']);
    expect(equivalentNames({ status: 'matched', profileId: 'm109', confidence: 1, candidates: [], evidence: [] }, 'm109', profiles)).toEqual([]);
    expect(equivalentNames(null, null, profiles)).toEqual([]);
  });
});
