import type { Point } from './types';

/** Consecutive agreeing observations needed before automation may switch the map. */
export const AUTO_MAP_CONFIRMATIONS = 2;
/** Observations further apart than this do not count as consecutive. */
export const AUTO_MAP_MAX_GAP_MS = 3000;
/** A weapon-panel pause this long (death, respawn screen, loadout) makes the own position suspicious. */
export const RESPAWN_GAP_MS = 10_000;

export type AutoMapTracker = { candidateId: string | null; count: number; lastTimestampMs: number };
export const emptyTracker: AutoMapTracker = { candidateId: null, count: 0, lastTimestampMs: 0 };
export type AutoMapDecision = { tracker: AutoMapTracker; switchTo: string | null; status: 'holding' | 'same' | 'confirming' | 'switch' };

/**
 * The user's map is persistent. Automation may only SWITCH it after a stable,
 * positive recognition of ANOTHER map; a missing/lost observation (death screen,
 * loadout, closed map, no match) never clears it — the last map is held.
 */
export function trackAutoMap(tracker: AutoMapTracker, observation: { mapId: string | null; timestampMs: number }, currentMapId: string | null): AutoMapDecision {
  if (!observation.mapId) return { tracker: emptyTracker, switchTo: null, status: 'holding' };
  if (observation.mapId === currentMapId) return { tracker: emptyTracker, switchTo: null, status: 'same' };
  const consecutive = tracker.candidateId === observation.mapId
    && observation.timestampMs > tracker.lastTimestampMs
    && observation.timestampMs - tracker.lastTimestampMs <= AUTO_MAP_MAX_GAP_MS;
  if (tracker.candidateId === observation.mapId && observation.timestampMs <= tracker.lastTimestampMs) {
    return { tracker, switchTo: null, status: 'confirming' }; // the same frame twice is not a second observation
  }
  const count = consecutive ? tracker.count + 1 : 1;
  if (count >= AUTO_MAP_CONFIRMATIONS) return { tracker: emptyTracker, switchTo: observation.mapId, status: 'switch' };
  return { tracker: { candidateId: observation.mapId, count, lastTimestampMs: observation.timestampMs }, switchTo: null, status: 'confirming' };
}

/** True when the weapon panel was not readable for longer than `threshold` (possible death and respawn). */
export function respawnGap(previousFreshMs: number | null, nextFreshMs: number, threshold = RESPAWN_GAP_MS): boolean {
  return previousFreshMs !== null && nextFreshMs - previousFreshMs > threshold;
}

/** Automatic positions: the last positions seen are kept for display (not for calculation) when markers disappear. */
export type HeldPositions = { own: Point | null; target: Point | null; timestampMs: number };
export function holdPositions(previous: HeldPositions, own: Point | null, target: Point | null, timestampMs: number): HeldPositions {
  if (!own && !target) return previous;
  return { own: own ?? previous.own, target: target ?? previous.target, timestampMs };
}
