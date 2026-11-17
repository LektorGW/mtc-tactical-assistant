import { describe, expect, it } from 'vitest';
import { markerPreviewIsFresh, markerPreviewMatchesImage, maxNormalizedCoordinate, normalizedPreviewPoint, validMarkerPreviewGeometry } from './marker-preview';

describe('marker preview geometry', () => {
  it('accepts a native thumbnail of a 1920×1080 frame without changing full-frame coordinates', () => {
    const source = { width: 1920, height: 1080, previewWidth: 1440, previewHeight: 810 };
    expect(markerPreviewMatchesImage(source, 1440, 810)).toBe(true);
    expect(markerPreviewMatchesImage(source, 1920, 1080)).toBe(false);
    const point = normalizedPreviewPoint(460, 355, { left: 100, top: 50, width: 720, height: 405 });
    expect(point).toEqual({ x: 0.5, y: 305 / 405 });
    expect(point!.x * source.width).toBe(960);
    expect(point!.y * source.height).toBeCloseTo(813.3333333333);
  });

  it('supports full-size previews and one-pixel thumbnail rounding', () => {
    expect(markerPreviewMatchesImage({ width: 982, height: 653, previewWidth: 982, previewHeight: 653 }, 982, 653)).toBe(true);
    expect(markerPreviewMatchesImage({ width: 982, height: 653, previewWidth: 1353, previewHeight: 900 }, 1353, 900)).toBe(true);
    expect(validMarkerPreviewGeometry({ width: 1921, height: 1081, previewWidth: 1440, previewHeight: 810 })).toBe(true);
  });

  it('rejects missing, stretched, zero-size and oversized preview metadata', () => {
    const source = { width: 1920, height: 1080, previewWidth: 1440, previewHeight: 810 };
    for (const patch of [{ previewWidth: undefined }, { previewWidth: NaN }, { previewWidth: 0 },
      { previewWidth: 1921 }, { previewHeight: 900 }, { height: 1080.5 }]) {
      expect(validMarkerPreviewGeometry({ ...source, ...patch } as typeof source)).toBe(false);
    }
  });

  it('rejects clicks outside the visible image and invalid layout dimensions', () => {
    const bounds = { left: 10, top: 20, width: 720, height: 405 };
    expect(normalizedPreviewPoint(9, 20, bounds)).toBeNull();
    expect(normalizedPreviewPoint(730, 426, bounds)).toBeNull();
    expect(normalizedPreviewPoint(10, 20, { ...bounds, width: 0 })).toBeNull();
    expect(normalizedPreviewPoint(NaN, 20, bounds)).toBeNull();
    const edge = normalizedPreviewPoint(730, 425, bounds)!;
    // The border maps to the last source pixel, inside the native [0, 1) range.
    expect(edge).toEqual({ x: maxNormalizedCoordinate, y: maxNormalizedCoordinate });
    expect(edge.x).toBeLessThan(1);
    expect(Math.floor(edge.x * 7680)).toBe(7679);
  });
});

describe('marker preview source age', () => {
  it('accepts at most two seconds of source age without refreshing stale or future timestamps', () => {
    expect(markerPreviewIsFresh(10_000, 12_000)).toBe(true);
    expect(markerPreviewIsFresh(10_000, 12_001)).toBe(false);
    expect(markerPreviewIsFresh(12_001, 12_000)).toBe(false);
    expect(markerPreviewIsFresh(NaN, 12_000)).toBe(false);
  });
});
