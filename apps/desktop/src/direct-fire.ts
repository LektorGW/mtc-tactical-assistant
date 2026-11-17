import type { Profile, Unit } from './types';

/** Mirrors `mtc_profiles::ObservedSpecs`. */
export type ObservedSpecs = {
  screen: 'loadout' | 'manual'; observedAt: string; weaponName?: string | null;
  muzzleVelocity: number; velocityUnitAssumption: string; gravitySource: string;
};
/** Mirrors `mtc_profiles::ReticleMap`. */
export type ReticleMark = { mark: string; range: number };
export type ReticleMap = { marks: ReticleMark[]; status: 'unverified' | 'experimental' | 'beta'; source: string };

export type RangeCardRow = { range: number; reachable: boolean; timeOfFlight: number | null; dropBelowLineOfSight: number | null; velocityAtRange: number | null; elevationMrad: number | null; elevationDeg: number | null; holdOverMrad: number | null; holdOverDeg: number | null; note: string | null };
export type RangeCard = { rows: RangeCardRow[]; unit: Unit; model: string; status: string; sightHeight: number; zeroRange: number | null; zeroElevationMrad: number; warnings: string[]; assumptions: string[]; stepsUsed: number };
export type LeadEstimate = { lateralOffset: number; radialOffset: number; futureRange: number; leadMrad: number; leadDeg: number; timeOfFlight: number; elevationMrad: number | null; iterations: number; converged: boolean; assumptions: string[] };
export type QuickAim = { range: number | null; pointBlankRange: number | null; centerAim: boolean; reachable: boolean; dropBelowLineOfSight: number | null; holdOverMrad: number | null; elevationMrad: number | null; timeOfFlight: number | null; reticle: { mark: string; markRange: number; residualDrop: number; residualMrad: number; status: string } | null; model: string; status: string; warnings: string[] };
export type AirToGroundImpact = { diveAngleDeg: number; horizontalDistance: number; slantRange: number; timeOfFlight: number; impactSpeed: number; impactAngleDeg: number; boresightGroundDistance: number | null; aimOffsetAlongGround: number | null; depressionBelowBoresightMrad: number; depressionBelowBoresightDeg: number; model: string; status: string; warnings: string[]; assumptions: string[] };
export type ControlShot = { range: number; elevationMrad: number; missVertical: number };
export type ControlShotFit = { fittedSpeed: number | null; residuals: number[]; maxResidual: number | null; fits: boolean; modelApplicable: boolean; message: string; assumptions: string[] };

/** Gravity of the community calculator (9.8 × 1.8 m gry/s²) — an assumption for hand-held weapons. */
export const CALCULATOR_GRAVITY = 9.8 * 1.8;
export const DEFAULT_PRESETS = [200, 400, 600, 800, 1000, 1200, 1500, 2000];
export const DIRECT_CATEGORIES = new Set(['direct_fire', 'tank', 'helicopter_gun', 'anti_aircraft', 'helicopter_rocket']);
/** Assumed PGO-7 style mapping, editable by the player (status unverified). */
export const PGO7_ASSUMED: ReticleMap = {
  marks: [2, 3, 4, 5].map(n => ({ mark: String(n), range: n * 100 })),
  status: 'unverified',
  source: 'Założenie: kreska N celownika PGO-7 ≈ N×100 m; niezweryfikowane w MTC',
};

const kmh = 1 / 3.6;
export const kmhToUnitsPerSecond = (value: number) => value * kmh;

export function validateObservedSpecs(value: unknown, speed: number | null | undefined, calibrated: boolean): ObservedSpecs | null {
  if (value === undefined || value === null) return null;
  if (typeof value !== 'object' || Array.isArray(value)) throw new Error('Niepoprawny odczyt z interfejsu gry');
  const raw = value as Record<string, unknown>;
  const allowed = new Set(['screen', 'observedAt', 'weaponName', 'muzzleVelocity', 'velocityUnitAssumption', 'gravitySource']);
  if (Object.keys(raw).some(key => !allowed.has(key))) throw new Error('Nieznane pole odczytu z interfejsu gry');
  const text = (v: unknown, max: number) => typeof v === 'string' && !!v.trim() && v.length <= max;
  if (!['loadout', 'manual'].includes(raw.screen as string) || !text(raw.observedAt, 64) || !text(raw.velocityUnitAssumption, 512) || !text(raw.gravitySource, 512)
    || (raw.weaponName !== undefined && raw.weaponName !== null && !text(raw.weaponName, 200))) {
    throw new Error('Niekompletne pochodzenie odczytu z interfejsu gry');
  }
  const v = raw.muzzleVelocity;
  if (typeof v !== 'number' || !Number.isFinite(v) || v <= 0) throw new Error('Niepoprawna odczytana prędkość wylotowa');
  if (!calibrated && (typeof speed !== 'number' || Math.abs(speed - v) > 1e-9 * Math.max(1, Math.abs(speed)))) {
    throw new Error('Prędkość profilu musi być równa odczytanej prędkości wylotowej');
  }
  return raw as unknown as ObservedSpecs;
}

export function validateReticle(value: unknown): ReticleMap | null {
  if (value === undefined || value === null) return null;
  const r = value as ReticleMap;
  if (typeof r !== 'object' || Array.isArray(r) || Object.keys(r).some(k => !['marks', 'status', 'source'].includes(k))
    || !Array.isArray(r.marks) || !r.marks.length || r.marks.length > 32
    || !['unverified', 'experimental', 'beta'].includes(r.status) || typeof r.source !== 'string' || !r.source.trim() || r.source.length > 512) {
    throw new Error('Mapa kresek celownika wymaga 1–32 kresek, statusu i źródła');
  }
  let previous = 0;
  for (const m of r.marks) {
    if (!m || Object.keys(m).some(k => !['mark', 'range'].includes(k)) || typeof m.mark !== 'string' || !m.mark.trim() || [...m.mark].length > 16
      || typeof m.range !== 'number' || !Number.isFinite(m.range) || m.range <= previous || m.range > 1e5) {
      throw new Error('Kreski celownika: niepuste etykiety i rosnące dodatnie dystanse');
    }
    previous = m.range;
  }
  return r;
}

/** "2=200, 3=300" ↔ marks. */
export function parseReticle(text: string): ReticleMark[] {
  return text.split(/[,;\n]/).map(part => part.trim()).filter(Boolean).map(part => {
    const match = /^(.{1,16}?)\s*[=:→]\s*([0-9]+(?:[.,][0-9]+)?)$/.exec(part);
    if (!match) throw new Error(`Niepoprawna kreska „${part}” — użyj formatu etykieta=dystans`);
    return { mark: match[1].trim(), range: Number(match[2].replace(',', '.')) };
  });
}
export const formatReticle = (marks: ReticleMark[]) => marks.map(m => `${m.mark}=${m.range}`).join(', ');

/** "50-800/50" or "100, 200, 400" → sorted unique positive ranges (max 64). */
export function parseRanges(text: string): number[] {
  const trimmed = text.trim();
  const span = /^([0-9.]+)\s*-\s*([0-9.]+)\s*\/\s*([0-9.]+)$/.exec(trimmed);
  let values: number[];
  if (span) {
    const [from, to, step] = span.slice(1).map(Number);
    if (!(from > 0 && to >= from && step > 0) || (to - from) / step > 63) throw new Error('Zakres: od-do/krok, maks. 64 wiersze');
    values = Array.from({ length: Math.floor((to - from) / step + 1e-9) + 1 }, (_, i) => from + i * step);
  } else {
    values = trimmed.split(/[,;\s]+/).filter(Boolean).map(v => Number(v.replace(',', '.')));
  }
  const unique = [...new Set(values)].sort((a, b) => a - b);
  if (!unique.length || unique.length > 64 || unique.some(v => !Number.isFinite(v) || v <= 0 || v > 1e5)) throw new Error('Podaj 1–64 dodatnich dystansów');
  return unique;
}

/** Index −1 = close mode (point-blank, aim centre). */
export function stepPreset(index: number, action: string, count: number): number {
  if (action === 'close') return -1;
  if (action === 'next') return Math.min(count - 1, index + 1);
  if (action === 'previous') return Math.max(-1, index - 1);
  return index;
}

export type DirectFireDraft = { name: string; velocity: number | null; gravity: number | null; screen: 'loadout' | 'manual'; observedAt: string; weaponName: string; acknowledged: boolean; reticle: ReticleMark[] | null };

/** Local direct-fire profile: status experimental, explicit unit/gravity assumptions. */
export function createDirectFireProfile(draft: DirectFireDraft, id = `direct-${Date.now().toString(36)}`): Profile {
  if (!draft.name.trim()) throw new Error('Podaj nazwę broni i amunicji');
  if (draft.velocity === null || !Number.isFinite(draft.velocity) || draft.velocity <= 0) throw new Error('Podaj dodatnią prędkość wylotową');
  if (draft.gravity === null || !Number.isFinite(draft.gravity) || draft.gravity <= 0) throw new Error('Podaj dodatnią grawitację');
  if (!draft.acknowledged) throw new Error('Potwierdź założenia jednostki i grawitacji');
  const date = draft.observedAt.slice(0, 10);
  const loadout = draft.screen === 'loadout';
  const observedSpecs: ObservedSpecs = {
    screen: draft.screen, observedAt: draft.observedAt, weaponName: draft.weaponName.trim() || null, muzzleVelocity: draft.velocity,
    velocityUnitAssumption: 'Założenie: „m/s” z interfejsu gry = metry gry na sekundę (konwencja kalkulatora społeczności); do weryfikacji',
    gravitySource: Math.abs(draft.gravity - CALCULATOR_GRAVITY) < 1e-12
      ? 'Założenie: 9.8 × 1.8 = 17.64 m gry/s² z kalkulatora grand-hawk; nie wiadomo, czy dotyczy broni ręcznej'
      : `Wpisana przez gracza: ${draft.gravity} m gry/s²; do weryfikacji`,
  };
  const profile: Profile & { observedSpecs: ObservedSpecs; reticle?: ReticleMap } = {
    schemaVersion: 1, id, name: draft.name.trim(), category: 'direct_fire',
    gameVersion: `${loadout ? 'odczyt UI gry' : 'wpis ręczny'} ${date}; wersja gry niepotwierdzona`,
    status: 'experimental',
    source: loadout ? `odczytane z UI gry ${date} (Loadout: Muzzle Velocity ${draft.velocity} m/s)` : `wpisane ręcznie ${date} (prędkość ${draft.velocity} m/s)`,
    calibrated: false, speed: draft.velocity, gravity: draft.gravity, minElevationDeg: -89, maxElevationDeg: 89,
    dispersionSigma: null, salvoSize: 1,
    notes: 'Profil ognia bezpośredniego. Model bez oporu, jeśli nie dodano współczynników; wymaga strzałów kontrolnych.',
    unit: 'game_meters', sourceAcknowledged: true, observedSpecs,
  };
  if (draft.reticle?.length) profile.reticle = validateReticle({ ...PGO7_ASSUMED, marks: draft.reticle })!;
  return profile;
}

const pl = (value: number, digits = 1) => value.toFixed(digits).replace('.', ',');

/** Single overlay line for the quick mode. Never mentions or locates targets. */
export function quickLine(quick: QuickAim | null, opts: { rangefinder: boolean; missingSpeed: boolean; error?: string | null }): string {
  if (opts.missingSpeed) return 'brak danych pocisku — odczytaj z Loadout';
  if (opts.error) return opts.error;
  if (!quick) return 'obliczanie…';
  const suffix = opts.rangefinder ? ' · z dalmierza gry' : '';
  if (quick.range === null) {
    return quick.pointBlankRange === null ? 'bliski zasięg: opad przekracza próg już przy 5 m' : `< ${Math.round(quick.pointBlankRange)} m · CELUJ W ŚRODEK`;
  }
  const range = `${Math.round(quick.range)} m`;
  if (!quick.reachable || quick.dropBelowLineOfSight === null) return `${range} · poza zasięgiem modelu${suffix}`;
  const time = quick.timeOfFlight === null ? '' : ` · t ${pl(quick.timeOfFlight)} s`;
  if (quick.reticle) {
    const r = quick.reticle.residualDrop;
    return `${range} → kreska ${quick.reticle.mark} (${r >= 0 ? '+' : '−'}${pl(Math.abs(r))} m)${time}${suffix}`;
  }
  if (quick.centerAim) return `${range} · CELUJ W ŚRODEK${time}${suffix}`;
  const drop = quick.dropBelowLineOfSight;
  const mrad = quick.holdOverMrad === null ? '' : ` (≈${pl(Math.abs(quick.holdOverMrad))} mrad)`;
  return `${range} · ${drop > 0 ? 'PODNIEŚ' : 'OBNIŻ'} ${pl(Math.abs(drop))} m${mrad}${time}${suffix}`;
}
