import type { WeaponPanelReading } from './mtc-hud-types';
import type { Point, Profile, Result, WeaponResolution } from './types';

/** Readings older than this are hidden; the game state may have changed. */
export const PANEL_MAX_AGE_MS = 2000;
/** Below this angle difference the axis counts as aligned. */
export const ON_TARGET_DEG = 0.1;

export type RequiredAim = { elevationDeg: number; azimuthDeg: number; flightTime: number; highArc: boolean; source: 'intercept3d' | 'solution' };
export type AxisCorrection = { delta: number; aligned: boolean; label: string };
export type AimCorrection = {
  elevation: AxisCorrection | null; azimuth: AxisCorrection | null;
  onTarget: boolean; panelTimestampMs: number;
};

/** Angle difference wrapped to (−180°, 180°]; positive = clockwise (to the right). */
export function wrapDegrees(delta: number): number {
  const wrapped = ((delta % 360) + 540) % 360 - 180;
  return wrapped === -180 ? 180 : wrapped;
}

/** Two decimals like the in-game panel (3.12°), Polish decimal comma. */
const pl = (value: number) => Math.abs(value).toFixed(2).replace('.', ',');
export function formatAngle(value: number): string { return `${value.toFixed(2).replace('.', ',')}°`; }

export function panelFresh(panel: Pick<WeaponPanelReading, 'timestampMs'> | null, now: number, maxAge = PANEL_MAX_AGE_MS): boolean {
  return !!panel && Number.isFinite(panel.timestampMs) && panel.timestampMs <= now + 250 && now - panel.timestampMs <= maxAge;
}

/** Required launch angles of the chosen arc (3D intercept first, then the static solution). */
export function requiredAimFrom(result: Result | null, preferHigh: boolean): RequiredAim | null {
  if (!result) return null;
  const intercept = result.intercept3d?.arcs ?? [];
  if (intercept.length) {
    const arc = intercept.length > 1 && preferHigh ? intercept[intercept.length - 1] : intercept[0];
    return { elevationDeg: arc.elevationDeg, azimuthDeg: arc.azimuthDeg, flightTime: arc.flightTime, highArc: preferHigh && intercept.length > 1, source: 'intercept3d' };
  }
  const arcs = result.solution?.arcs ?? [];
  const arc = arcs.find(a => a.highArc === preferHigh) ?? arcs[0];
  if (!arc) return null;
  return { elevationDeg: arc.elevationDeg, azimuthDeg: result.bearing.azimuthDeg, flightTime: arc.flightTime, highArc: arc.highArc, source: 'solution' };
}

/**
 * Advice only: required − current, for the player to apply manually.
 * Elevation: positive = raise. Azimuth: wrapped, positive = turn right (clockwise).
 */
export function aimCorrection(required: RequiredAim | null, panel: WeaponPanelReading | null, now: number, tolerance = ON_TARGET_DEG): AimCorrection | null {
  if (!required || !panel || !panelFresh(panel, now)) return null;
  const elevation = panel.elevationDeg && Number.isFinite(panel.elevationDeg.value) ? (() => {
    const delta = required.elevationDeg - panel.elevationDeg!.value;
    const aligned = Math.abs(delta) < tolerance;
    return { delta, aligned, label: aligned ? '✓ ELEWACJA' : delta > 0 ? `PODNIEŚ +${pl(delta)}°` : `OPUŚĆ −${pl(delta)}°` };
  })() : null;
  const azimuth = panel.azimuthDeg && Number.isFinite(panel.azimuthDeg.value) ? (() => {
    const delta = wrapDegrees(required.azimuthDeg - panel.azimuthDeg!.value);
    const aligned = Math.abs(delta) < tolerance;
    return { delta, aligned, label: aligned ? '✓ AZYMUT' : delta > 0 ? `W PRAWO ${pl(delta)}°` : `W LEWO ${pl(delta)}°` };
  })() : null;
  if (!elevation && !azimuth) return null;
  return { elevation, azimuth, onTarget: !!elevation?.aligned && !!azimuth?.aligned, panelTimestampMs: panel.timestampMs };
}

/** In-game grid fraction (x from west, y from north) → pixel of the catalog texture covering the same square. */
/** Display names of profiles the resolver marks as ballistically equivalent (optional `equivalents`). */
export function equivalentNames(resolution: WeaponResolution | null, chosenId: string | null, profiles: Pick<Profile, 'id' | 'name'>[]): string[] {
  const raw = (resolution as (WeaponResolution & { equivalents?: unknown }) | null)?.equivalents;
  if (!Array.isArray(raw)) return [];
  const names = raw.map(item => {
    if (typeof item === 'string') return profiles.find(p => p.id === item)?.name ?? item;
    if (item && typeof item === 'object') {
      const entry = item as { profileId?: unknown; vehicle?: unknown; name?: unknown };
      if (entry.profileId === chosenId) return null;
      if (typeof entry.vehicle === 'string') return entry.vehicle;
      if (typeof entry.name === 'string') return entry.name;
      if (typeof entry.profileId === 'string') return profiles.find(p => p.id === entry.profileId)?.name ?? entry.profileId;
    }
    return null;
  }).filter((name): name is string => !!name && name !== profiles.find(p => p.id === chosenId)?.name);
  return [...new Set(names)];
}

export function fractionToTexturePixel(fraction: Point, width: number, height: number): Point | null {
  if (![fraction.x, fraction.y].every(Number.isFinite) || fraction.x < 0 || fraction.y < 0 || fraction.x > 1 || fraction.y > 1) return null;
  return { x: Math.min(fraction.x * width, width - 1), y: Math.min(fraction.y * height, height - 1) };
}
