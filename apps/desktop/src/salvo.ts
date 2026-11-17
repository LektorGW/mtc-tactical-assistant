import type { Calibration, Profile, Unit } from './types';

/** Mirrors `mtc_profiles::SalvoDispersion` (camelCase JSON). */
export type SalvoDispersion = {
  unit: Unit;
  angularSigmaMrad: number | null;
  speedSigma: number | null;
  rippleInterval: number | null;
  calibrated: boolean;
  status: 'no_data' | 'experimental' | 'beta';
  source: string;
};
/** @deprecated `Profile` now carries `salvo`; kept for existing imports. */
export type ProfileWithSalvo = Profile;

/** Target-centred east/north ellipse in the profile unit (Rust `DispersionEllipse`). */
export type DispersionEllipse = {
  probability: number; centerOffset: [number, number];
  semiMajor: number; semiMinor: number; majorAxisAzimuthDeg: number;
  outline: [number, number][];
};
export type SalvoPattern = {
  unit: Unit; status: 'experimental' | 'beta'; salvoSize: number;
  dispersionSource: 'launch_dispersion' | 'impact_sigma';
  targetPosition: [number, number]; centerOffset: [number, number];
  covariance: [[number, number], [number, number]];
  rangeSigma: number; crossRangeSigma: number;
  singleProjectile: DispersionEllipse; salvoEnvelope: DispersionEllipse; meanPointOfImpact: DispersionEllipse;
  nonlinearity: number;
  coverage: { effectRadius: number; probabilityAnyWithinEffect: number; expectedProjectilesWithinEffect: number; areaRadius: number | null; expectedAreaFraction: number | null; samples: number } | null;
  timing: { rippleInterval: number; firstImpact: number; lastImpact: number } | null;
  missingComponents: string[]; assumptions: string[];
};
export type DynamicInterceptSolution = {
  arcs: { elevationDeg: number; azimuthDeg: number; flightTime: number; launchVelocity: [number, number, number]; worldInitialVelocity: [number, number, number]; targetPositionAtImpact: [number, number, number]; projectilePositionAtImpact: [number, number, number]; lead: [number, number, number]; missDistance: number; iterations: number; highArc: boolean }[];
  branches: { seedElevationDeg: number; seedFlightTime: number; converged: boolean; iterations: number; finalMiss: number | null; message: string }[];
  status: 'experimental' | 'beta'; unit: Unit; stepsUsed: number; assumptions: string[];
};

const salvoFields = new Set(['unit', 'angularSigmaMrad', 'speedSigma', 'rippleInterval', 'calibrated', 'status', 'source']);
function optionalNumber(value: unknown, label: string, maximum: number): number | null {
  if (value === null || value === undefined) return null;
  if (typeof value !== 'number' || !Number.isFinite(value) || value < 0 || value > maximum) {
    throw new Error(`${label}: wymagana skończona liczba od 0 do ${maximum}`);
  }
  return value;
}

/** Structural validation identical to Rust `SalvoDispersion::validate`. */
export function validateSalvo(value: unknown, profile: Pick<Profile, 'unit'>): SalvoDispersion | null {
  if (value === undefined || value === null) return null;
  if (typeof value !== 'object' || Array.isArray(value)) throw new Error('Niepoprawne dane rozrzutu salwy');
  const raw = value as Record<string, unknown>;
  if (Object.keys(raw).some(key => !salvoFields.has(key))) throw new Error('Nieznane pole rozrzutu salwy');
  if (!['game_units', 'game_meters'].includes(raw.unit as string) || raw.unit !== (profile.unit ?? 'game_units')) {
    throw new Error('Jednostka rozrzutu salwy musi odpowiadać jednostce profilu');
  }
  const status = raw.status ?? 'no_data';
  const calibrated = raw.calibrated ?? false;
  const source = raw.source ?? '';
  if (typeof status !== 'string' || !['no_data', 'experimental', 'beta'].includes(status)
    || typeof calibrated !== 'boolean' || typeof source !== 'string' || new TextEncoder().encode(source).length > 2048) {
    throw new Error('Niepoprawny status lub źródło rozrzutu salwy');
  }
  const salvo: SalvoDispersion = {
    unit: raw.unit as Unit,
    angularSigmaMrad: optionalNumber(raw.angularSigmaMrad, 'Rozrzut kątowy (mrad)', 500),
    speedSigma: optionalNumber(raw.speedSigma, 'Rozrzut prędkości', 1e9),
    rippleInterval: optionalNumber(raw.rippleInterval, 'Odstęp startów (s)', 600),
    calibrated, status: status as SalvoDispersion['status'], source,
  };
  if (salvo.calibrated && salvo.status !== 'beta') throw new Error('Lokalna kalibracja rozrzutu salwy wymaga statusu beta');
  if (salvo.status !== 'no_data') requireSalvoParameters(salvo);
  return salvo;
}

export function requireSalvoParameters(salvo: SalvoDispersion): void {
  if (salvo.status === 'no_data' || !salvo.source.trim()) throw new Error('Brak danych rozrzutu salwy: wymagane źródło i status experimental/beta');
  if (salvo.angularSigmaMrad === null && salvo.speedSigma === null) throw new Error('Brak danych rozrzutu salwy: wymagany rozrzut kątowy lub prędkości');
}

export type SalvoDraft = { angularSigmaMrad: number | null; speedSigma: number | null; rippleInterval: number | null; source: string; confirmed: boolean };

/** Explicit local entry from observations; result is beta only for a locally calibrated profile. */
export function prepareLocalSalvo(profile: Profile | undefined, draft: SalvoDraft): SalvoDispersion {
  if (!profile) throw new Error('Wybierz profil pocisku');
  if (profile.category === 'guided_missile') throw new Error('Mechanika naprowadzania jest nieznana; brak prognozy salwy');
  if (!draft.confirmed) throw new Error('Potwierdź, że parametry rozrzutu pochodzą z pomiarów lub wskazanego źródła');
  const local = profile.calibrated === true && profile.status === 'beta';
  const salvo = validateSalvo({
    unit: profile.unit ?? 'game_units', angularSigmaMrad: draft.angularSigmaMrad, speedSigma: draft.speedSigma,
    rippleInterval: draft.rippleInterval, calibrated: local, status: local ? 'beta' : 'experimental', source: draft.source.trim(),
  }, profile);
  if (!salvo) throw new Error('Brak parametrów rozrzutu');
  return salvo;
}

/**
 * Convert a target-centred east/north offset (profile unit) to a pixel delta on
 * a map image calibrated with `MapCalibration` (inverse of `transform`).
 * dx grows to the right, dy grows downwards (SVG/canvas convention).
 */
export function offsetToPixels(offset: [number, number], calibration: Pick<Calibration, 'unitsPerPixel' | 'northRotationDeg'>): [number, number] {
  const a = calibration.northRotationDeg * Math.PI / 180;
  const [east, north] = offset;
  const imageEast = east * Math.cos(a) - north * Math.sin(a);
  const imageNorth = east * Math.sin(a) + north * Math.cos(a);
  return [imageEast / calibration.unitsPerPixel, -imageNorth / calibration.unitsPerPixel];
}

/** SVG `<ellipse>` attributes around the target pixel for a map overlay. */
export function ellipseOnMap(ellipse: DispersionEllipse, targetPixel: { x: number; y: number }, calibration: Pick<Calibration, 'unitsPerPixel' | 'northRotationDeg'>) {
  const [dx, dy] = offsetToPixels(ellipse.centerOffset, calibration);
  return {
    cx: targetPixel.x + dx, cy: targetPixel.y + dy,
    rx: ellipse.semiMajor / calibration.unitsPerPixel, ry: ellipse.semiMinor / calibration.unitsPerPixel,
    rotationDeg: ellipse.majorAxisAzimuthDeg - calibration.northRotationDeg - 90,
  };
}

/** North-up preview frame: SVG x = east, y = −north, scaled to fit `size`. */
export function previewScale(pattern: SalvoPattern, size: number, margin = 0.12): number {
  const extent = [pattern.salvoEnvelope, pattern.singleProjectile]
    .flatMap(e => e.outline).reduce((max, [e, n]) => Math.max(max, Math.abs(e), Math.abs(n)), 0);
  const radius = Math.max(extent, pattern.coverage?.areaRadius ?? 0, pattern.coverage?.effectRadius ?? 0, 1e-9);
  return (size * (0.5 - margin)) / radius;
}
export function outlinePath(outline: [number, number][], scale: number, size: number): string {
  if (!outline.length) return '';
  return outline.map(([e, n], i) => `${i ? 'L' : 'M'}${(size / 2 + e * scale).toFixed(2)} ${(size / 2 - n * scale).toFixed(2)}`).join(' ') + ' Z';
}
