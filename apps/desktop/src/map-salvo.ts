import type { MapSalvoLayer } from './MapCanvas';
import { ellipseOnMap, offsetToPixels, type SalvoPattern } from './salvo';
import type { Point, Settings } from './types';

/** Converts the target-centred salvo forecast to image pixels; refuses unit mismatches. */
export function salvoLayerFor(pattern: SalvoPattern | null, target: Point | null, scale: number | null, rotation: number, mapUnit: Settings['mapUnit']): { layer: MapSalvoLayer | null; reason: string } {
  if (!pattern) return { layer: null, reason: '' };
  if (!target || !scale) return { layer: null, reason: 'Prognoza salwy wymaga celu i skali mapy.' };
  if (pattern.unit !== mapUnit) return { layer: null, reason: 'Prognoza salwy ma inne jednostki niż mapa — elipsy nie są rysowane.' };
  const calibration = { unitsPerPixel: scale, northRotationDeg: rotation };
  const ellipse = (key: string, e: SalvoPattern['salvoEnvelope'], color: string, dashed = false) => ({ key, ...ellipseOnMap(e, target, calibration), color, dashed });
  const [dx, dy] = offsetToPixels(pattern.centerOffset, calibration);
  return {
    layer: {
      ellipses: [ellipse('salvo', pattern.salvoEnvelope, '#ff7f6e'), ellipse('single', pattern.singleProjectile, '#ffb3a8', true), ellipse('mpi', pattern.meanPointOfImpact, '#f6d58c')],
      center: { x: target.x + dx, y: target.y + dy },
      label: `SALWA ${pattern.status === 'beta' ? 'BETA' : 'EKSP.'}${pattern.missingComponents.length ? ' · niepełny model' : ''}`,
    },
    reason: '',
  };
}

/** A forecast is valid only for the exact inputs it was computed from. */
export type SalvoForecast = { pattern: SalvoPattern; scope: string };
export function salvoScopeKey(input: {
  profileId: string | null; profileSalvo: unknown; profileSpeed: number | null; profileGravity: number | null; salvoSize: number | null;
  calibration: { unitsPerPixel: number; northRotationDeg: number } | null; mapPath: string | null;
  own: Point | null; target: Point | null; heightDelta: number | null; arcs: { elevationDeg: number; flightTime: number }[] | null;
}): string {
  return JSON.stringify([input.profileId, input.profileSalvo ?? null, input.profileSpeed, input.profileGravity, input.salvoSize,
    input.calibration ? [input.calibration.unitsPerPixel, input.calibration.northRotationDeg] : null, input.mapPath,
    input.own, input.target, input.heightDelta, input.arcs?.map(a => [a.elevationDeg, a.flightTime]) ?? null]);
}
/** Stale forecasts (other profile, target, map, height or arcs) are never drawn. */
export function currentSalvo(forecast: SalvoForecast | null, scope: string): SalvoPattern | null {
  return forecast && forecast.scope === scope ? forecast.pattern : null;
}
