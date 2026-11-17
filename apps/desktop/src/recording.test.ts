import { describe, expect, it } from 'vitest';
import { buildRecordingRequest, defaultRecordingOptions, formatBytes, formatDuration, recordingOptionsError, recordingProgress } from './recording';
import { buildVisionConfig, updateRoiField } from './vision-utils';
import { defaultSettings } from './types';

describe('nagrywanie testowe za zgodą', () => {
  it('nie buduje żądania bez jawnej zgody, katalogu lub okna', () => {
    expect(() => buildRecordingRequest(defaultRecordingOptions, false, 'C:\\nagrania', 1, 'wgc', null)).toThrow(/zgod/);
    expect(() => buildRecordingRequest(defaultRecordingOptions, true, null, 1, 'wgc', null)).toThrow(/katalog/);
    expect(() => buildRecordingRequest(defaultRecordingOptions, true, '  ', 1, 'wgc', null)).toThrow(/katalog/);
    expect(() => buildRecordingRequest(defaultRecordingOptions, true, 'C:\\nagrania', null, 'wgc', null)).toThrow(/okno/);
    const request = buildRecordingRequest(defaultRecordingOptions, true, 'C:\\nagrania', 42, 'gdi', null);
    expect(request).toMatchObject({ consent: true, directory: 'C:\\nagrania', handle: 42, backend: 'gdi', format: 'png', maxFps: 1 });
  });

  it('odrzuca limity spoza zakresu obsługiwanego natywnie', () => {
    expect(recordingOptionsError(defaultRecordingOptions)).toBeNull();
    for (const patch of [{ maxFps: 0 }, { maxFps: 6 }, { maxFps: NaN }, { maxFrames: 0 }, { maxFrames: 1.5 },
      { maxFrames: 10001 }, { maxMegabytes: 0 }, { maxMegabytes: 8193 }, { maxSeconds: 3601 },
      { format: 'jpeg' as const, jpegQuality: 20 }]) {
      expect(recordingOptionsError({ ...defaultRecordingOptions, ...patch })).not.toBeNull();
    }
    expect(recordingOptionsError({ ...defaultRecordingOptions, format: 'png', jpegQuality: 20 })).toBeNull();
  });

  it('formatuje postęp według najbardziej wykorzystanego limitu', () => {
    expect(formatBytes(1536)).toBe('2 KB');
    expect(formatBytes(5 * 1024 * 1024)).toBe('5.0 MB');
    expect(formatDuration(75_400)).toBe('01:15');
    expect(formatDuration(-1)).toBe('—');
    expect(recordingProgress({ frames: 10, maxFrames: 100, bytes: 50, maxBytes: 100, elapsedMs: 0, maxDurationMs: 1000 })).toBe(0.5);
    expect(recordingProgress({ frames: 200, maxFrames: 100, bytes: 0, maxBytes: 0, elapsedMs: 0, maxDurationMs: 0 })).toBe(1);
  });
});

describe('pola liczbowe ROI', () => {
  const roi = { x: 0.5, y: 0.5, width: 0.4, height: 0.4 };
  it('ignoruje puste pole zamiast wysyłać NaN', () => {
    expect(updateRoiField(roi, 'x', NaN)).toBeNull();
  });
  it('utrzymuje prostokąt w granicach klatki', () => {
    expect(updateRoiField(roi, 'x', 0.8)).toEqual({ ...roi, x: 0.8, width: 1 - 0.8 });
    expect(updateRoiField(roi, 'width', 2)).toEqual({ ...roi, width: 0.5 });
    expect(updateRoiField(roi, 'y', -1)).toEqual({ ...roi, y: 0 });
    const edge = updateRoiField(roi, 'x', 1)!;
    expect(edge.x).toBeLessThan(1);
    expect(edge.x + edge.width).toBeLessThanOrEqual(1);
    expect(edge.width).toBeGreaterThan(0);
  });
  it('buduje tę samą konfigurację analizy co pętla OCR', () => {
    const config = buildVisionConfig(defaultSettings);
    expect(config.ocrRegions.every(r => !('enabled' in r))).toBe(true);
    expect(config.markerConfig?.mapWidth).toBe(defaultSettings.mapWidth);
  });
});
