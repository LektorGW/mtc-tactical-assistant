import type { RecordingBackend, RecordingOptions, RecordingRequest, RecordingStatus } from './recording-types';
import type { VisionConfig } from './types';

/** Defaults are conservative; recording itself stays off until consent is ticked. */
export const defaultRecordingOptions: RecordingOptions = {
  format: 'png', jpegQuality: 90, maxFps: 1, maxFrames: 300, maxMegabytes: 500, maxSeconds: 300,
};

const between = (value: number, min: number, max: number) => Number.isFinite(value) && value >= min && value <= max;

/** Mirrors the native limits: 0.1–5 FPS, 1–10000 frames, 1–8192 MB, 1–3600 s. */
export function recordingOptionsError(options: RecordingOptions): string | null {
  if (options.format !== 'png' && options.format !== 'jpeg') return 'Wybierz format PNG lub JPEG.';
  if (!between(options.maxFps, 0.1, 5)) return 'Limit FPS nagrania musi wynosić 0,1–5.';
  if (!Number.isInteger(options.maxFrames) || !between(options.maxFrames, 1, 10000)) return 'Limit klatek musi wynosić 1–10000.';
  if (!Number.isInteger(options.maxMegabytes) || !between(options.maxMegabytes, 1, 8192)) return 'Limit rozmiaru musi wynosić 1–8192 MB.';
  if (!Number.isInteger(options.maxSeconds) || !between(options.maxSeconds, 1, 3600)) return 'Limit czasu musi wynosić 1–3600 s.';
  if (options.format === 'jpeg' && (!Number.isInteger(options.jpegQuality) || !between(options.jpegQuality, 40, 100))) return 'Jakość JPEG musi wynosić 40–100.';
  return null;
}

/**
 * Builds the native request only with explicit consent from the current UI
 * session, a chosen directory and a selected window. Throws a Polish message otherwise.
 */
export function buildRecordingRequest(options: RecordingOptions, consent: boolean, directory: string | null,
  handle: number | null, backend: RecordingBackend, config: VisionConfig | null): RecordingRequest {
  if (consent !== true) throw new Error('Zaznacz zgodę na zapis klatek przed rozpoczęciem nagrywania.');
  if (!directory || !directory.trim()) throw new Error('Wybierz lokalny katalog na nagranie.');
  if (handle === null || !Number.isFinite(handle)) throw new Error('Wybierz widoczne okno do nagrania.');
  const error = recordingOptionsError(options);
  if (error) throw new Error(error);
  return { ...options, consent: true, directory, handle, backend, config };
}

export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return '—';
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

export function formatDuration(ms: number): string {
  if (!Number.isFinite(ms) || ms < 0) return '—';
  const seconds = Math.floor(ms / 1000);
  return `${String(Math.floor(seconds / 60)).padStart(2, '0')}:${String(seconds % 60).padStart(2, '0')}`;
}

/** Largest fraction of any limit already used, 0–1, for the indicator bar. */
export function recordingProgress(status: Pick<RecordingStatus, 'frames' | 'bytes' | 'elapsedMs' | 'maxFrames' | 'maxBytes' | 'maxDurationMs'>): number {
  const parts = [
    status.maxFrames > 0 ? status.frames / status.maxFrames : 0,
    status.maxBytes > 0 ? status.bytes / status.maxBytes : 0,
    status.maxDurationMs > 0 ? status.elapsedMs / status.maxDurationMs : 0,
  ].filter(Number.isFinite);
  return Math.min(1, Math.max(0, ...parts));
}
