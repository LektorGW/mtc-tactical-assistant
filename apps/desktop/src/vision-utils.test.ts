import { describe, expect, it } from 'vitest';
import { isCaptureBusy, MIN_SCAN_GAP_MS, nextScanDelay } from './vision-utils';

describe('live scan scheduling', () => {
  it('keeps the remaining frame interval when analysis was fast', () => {
    expect(nextScanDelay(3, 100)).toBeCloseTo(1000 / 3 - 100);
  });
  it('leaves_a_gap_for_other_capture_consumers_after_a_slow_scan', () => {
    expect(nextScanDelay(3, 340)).toBe(MIN_SCAN_GAP_MS);
    expect(nextScanDelay(0.25, 10_000)).toBe(MIN_SCAN_GAP_MS);
    expect(MIN_SCAN_GAP_MS).toBeGreaterThan(10);
  });
  it('recognises only the native busy-slot rejection as transient', () => {
    expect(isCaptureBusy('Poprzednia klatka jest przetwarzana')).toBe(true);
    expect(isCaptureBusy(new Error('Poprzednia klatka jest przetwarzana'))).toBe(true);
    expect(isCaptureBusy('Okno źródłowe zostało zamknięte')).toBe(false);
  });
});
