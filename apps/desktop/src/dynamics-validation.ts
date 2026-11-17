import type { Profile, ProjectileDynamics } from './types';

export type DynamicsDraft = {
  model: ProjectileDynamics['model'];
  linearDrag: number | null;
  quadraticDrag: number | null;
  rocketAcceleration: number | null;
  burnTime: number | null;
  thrustDirectionFixedLaunch: boolean;
  source: string;
  localCalibrationConfirmed: boolean;
};

const fields = new Set(['model', 'unit', 'linearDrag', 'quadraticDrag', 'rocketAcceleration', 'burnTime', 'thrustDirectionFixedLaunch', 'calibrated', 'status', 'source']);
function coefficient(value: unknown, label: string): number | null {
  if (value === null || value === undefined) return null;
  if (typeof value !== 'number' || !Number.isFinite(value) || value < 0) {
    throw new Error(`${label}: wymagana skończona liczba nieujemna`);
  }
  return value;
}
function requireParameters(d: ProjectileDynamics): void {
  if (d.model === 'vacuum') return;
  if (d.linearDrag === null || d.quadraticDrag === null) {
    throw new Error('Wpisz oba współczynniki oporu; puste pole oznacza brak pomiaru, potwierdzone zero wpisz ręcznie');
  }
  if (d.model === 'rocket' && (d.rocketAcceleration === null || d.burnTime === null || d.thrustDirectionFixedLaunch !== true)) {
    throw new Error('Napęd wymaga przyspieszenia, dodatniego czasu pracy i potwierdzenia stałego kierunku startowego');
  }
}

/** Structural validation mirrors the optional Rust dynamics schema. Missing
 * calibration stays unavailable; this function does not acknowledge a source. */
export function validateDynamics(value: unknown, profile: Profile): ProjectileDynamics | null {
  if (value === undefined || value === null) return null;
  if (typeof value !== 'object' || Array.isArray(value)) throw new Error('Niepoprawne dane oporu i napędu');
  const raw = value as Record<string, unknown>;
  if (Object.keys(raw).some(key => !fields.has(key))) throw new Error('Nieznane pole modelu oporu i napędu');
  if (!['vacuum', 'drag', 'rocket'].includes(raw.model as string)) throw new Error('Nieznany model lotu');
  if (!['game_units', 'game_meters'].includes(raw.unit as string) || raw.unit !== (profile.unit ?? 'game_units')) {
    throw new Error('Jednostka oporu i napędu musi odpowiadać jednostce profilu');
  }
  const status = raw.status === undefined ? 'no_data' : raw.status;
  const calibrated = raw.calibrated === undefined ? false : raw.calibrated;
  const source = raw.source === undefined ? '' : raw.source;
  if (typeof status !== 'string' || !['no_data', 'experimental', 'beta'].includes(status)
    || typeof calibrated !== 'boolean' || typeof source !== 'string' || new TextEncoder().encode(source).length > 2048) {
    throw new Error('Niepoprawny status, potwierdzenie lub źródło modelu oporu i napędu');
  }
  const fixed = raw.thrustDirectionFixedLaunch ?? null;
  if (fixed !== null && typeof fixed !== 'boolean') throw new Error('Niepoprawne potwierdzenie kierunku napędu');
  const d: ProjectileDynamics = {
    model: raw.model as ProjectileDynamics['model'],
    unit: raw.unit as ProjectileDynamics['unit'],
    linearDrag: coefficient(raw.linearDrag, 'Opór liniowy'),
    quadraticDrag: coefficient(raw.quadraticDrag, 'Opór kwadratowy'),
    rocketAcceleration: coefficient(raw.rocketAcceleration, 'Przyspieszenie rakiety'),
    burnTime: coefficient(raw.burnTime, 'Czas pracy napędu'),
    thrustDirectionFixedLaunch: fixed,
    calibrated, status: status as ProjectileDynamics['status'], source,
  };
  if (d.burnTime !== null && d.burnTime <= 0) throw new Error('Czas pracy napędu musi być dodatni');
  if (d.model === 'vacuum' && [d.linearDrag, d.quadraticDrag, d.rocketAcceleration, d.burnTime, d.thrustDirectionFixedLaunch].some(v => v !== null)) {
    throw new Error('Model bez oporu i napędu nie może ignorować wpisanych współczynników');
  }
  if (d.model === 'drag' && [d.rocketAcceleration, d.burnTime, d.thrustDirectionFixedLaunch].some(v => v !== null)) {
    throw new Error('Model oporu nie może ignorować parametrów napędu');
  }
  if (d.model === 'rocket' && d.thrustDirectionFixedLaunch === false) {
    throw new Error('Brak modelu zmiennego kierunku napędu; wymagane jawne założenie stałego kierunku startowego');
  }
  if (profile.category === 'guided_missile' && d.model !== 'vacuum') {
    throw new Error('Mechanika naprowadzania jest nieznana; model oporu i napędu dla tego profilu jest niedostępny');
  }
  if (d.calibrated) {
    if (d.status !== 'beta' || !d.source.trim()) throw new Error('Lokalna kalibracja wymaga statusu beta i źródła pomiarów');
    requireParameters(d);
  }
  return d;
}

/** Explicit user entry, never a fit or confirmation of live MTC mechanics. */
export function prepareLocalDynamics(profile: Profile | undefined, draft: DynamicsDraft): ProjectileDynamics | null {
  if (!profile) throw new Error('Wybierz profil pocisku');
  if (draft.model === 'vacuum') return null;
  if (!['drag', 'rocket'].includes(draft.model)) throw new Error('Nieznany model lotu');
  if (profile.category === 'guided_missile') throw new Error('Brak modelu mechaniki naprowadzania');
  if (profile.calibrated !== true || profile.status !== 'beta'
    || typeof profile.speed !== 'number' || !Number.isFinite(profile.speed) || profile.speed <= 0
    || typeof profile.gravity !== 'number' || !Number.isFinite(profile.gravity) || profile.gravity <= 0) {
    throw new Error('Najpierw przygotuj lokalną kalibrację beta prędkości i grawitacji dla wybranego pocisku');
  }
  if (draft.localCalibrationConfirmed !== true) throw new Error('Potwierdź lokalną kalibrację parametrów oporu i napędu');
  const d = validateDynamics({
    model: draft.model, unit: profile.unit ?? 'game_units',
    linearDrag: draft.linearDrag, quadraticDrag: draft.quadraticDrag,
    rocketAcceleration: draft.model === 'rocket' ? draft.rocketAcceleration : null,
    burnTime: draft.model === 'rocket' ? draft.burnTime : null,
    thrustDirectionFixedLaunch: draft.model === 'rocket' ? draft.thrustDirectionFixedLaunch : null,
    calibrated: true, status: 'beta', source: draft.source,
  }, profile);
  if (!d) throw new Error('Brak parametrów modelu');
  requireParameters(d);
  return { ...d, source: d.source.trim() };
}
