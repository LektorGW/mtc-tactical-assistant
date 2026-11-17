import type { Roi, Settings, VisionConfig } from './types';

const MIN_SIZE = 0.001;

/**
 * Applies one numeric ROI field from the panel. Non-finite input (an emptied
 * field) is ignored instead of sending NaN to the native validator, and the
 * rectangle is clamped so x + width and y + height never exceed the frame.
 */
export function updateRoiField(roi: Roi, key: keyof Roi, value: number): Roi | null {
  if (!Number.isFinite(value)) return null;
  const next = { ...roi, [key]: Math.min(1, Math.max(0, value)) };
  next.x = Math.min(next.x, 1 - MIN_SIZE);
  next.y = Math.min(next.y, 1 - MIN_SIZE);
  next.width = Math.min(Math.max(next.width, MIN_SIZE), 1 - next.x);
  next.height = Math.min(Math.max(next.height, MIN_SIZE), 1 - next.y);
  return next;
}

/** Same analysis configuration as the live OCR loop builds from the settings. */
export function buildVisionConfig(s: Settings): VisionConfig {
  return {
    schemaVersion: 1,
    referenceWidth: s.captureReferenceWidth,
    referenceHeight: s.captureReferenceHeight,
    aspectTolerance: 0.03,
    maxAgeMs: 2000,
    ocrRegions: s.hudRegions.filter(r => r.enabled).map(({ enabled: _, ...r }) => r),
    markerConfig: { ...s.markerConfig, mapWidth: s.mapWidth, mapHeight: s.mapHeight },
  };
}

/** Native rejection while another capture consumer holds the single frame slot. */
export const CAPTURE_BUSY_MESSAGE = 'Poprzednia klatka jest przetwarzana';

/**
 * Minimum pause between live scans. Waiting for the next WGC frame keeps the native
 * frame slot for almost the whole interval, so without a gap a marker-preview refresh
 * (it waits up to 800 ms, polling every 10 ms) could practically never acquire it.
 */
export const MIN_SCAN_GAP_MS = 30;

export function nextScanDelay(fps: number, elapsedMs: number): number {
  return Math.max(MIN_SCAN_GAP_MS, 1000 / fps - elapsedMs);
}

/** A busy slot is a transient collision, not a lost observation. */
export function isCaptureBusy(error: unknown): boolean {
  return String(error).includes(CAPTURE_BUSY_MESSAGE);
}
