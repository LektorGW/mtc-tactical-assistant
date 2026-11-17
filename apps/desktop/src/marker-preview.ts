import type { MarkerSetupPreview } from './marker-types';
import type { Point } from './types';

type PreviewGeometry = Pick<MarkerSetupPreview, 'width' | 'height' | 'previewWidth' | 'previewHeight'>;
type ImageBounds = { left: number; top: number; width: number; height: number };

export const markerPreviewMaxAgeMs = 2000;

export function validMarkerPreviewGeometry(data: PreviewGeometry): boolean {
  const dimensions = [data.width, data.height, data.previewWidth, data.previewHeight];
  if (!dimensions.every(value => Number.isInteger(value) && value > 0)
    || data.previewWidth > 1440 || data.previewHeight > 900) return false;
  // Native thumbnailing rounds to whole pixels. The image still spans the
  // entire source frame, including when a small source is enlarged to fit.
  // Accept at most one pixel of aspect-ratio rounding.
  return Math.abs(data.previewWidth * data.height - data.previewHeight * data.width)
    <= Math.max(data.width, data.height);
}

export function markerPreviewMatchesImage(data: PreviewGeometry, naturalWidth: number, naturalHeight: number): boolean {
  return validMarkerPreviewGeometry(data) && naturalWidth === data.previewWidth && naturalHeight === data.previewHeight;
}

export function markerPreviewIsFresh(timestampMs: number, nowMs: number): boolean {
  return Number.isFinite(timestampMs) && Number.isFinite(nowMs)
    && timestampMs <= nowMs && nowMs - timestampMs <= markerPreviewMaxAgeMs;
}

export function normalizedPreviewPoint(clientX: number, clientY: number, bounds: ImageBounds): Point | null {
  if (![clientX, clientY, bounds.left, bounds.top, bounds.width, bounds.height].every(Number.isFinite)
    || bounds.width <= 0 || bounds.height <= 0) return null;
  const point = { x: (clientX - bounds.left) / bounds.width, y: (clientY - bounds.top) / bounds.height };
  if (!(point.x >= 0 && point.x <= 1 && point.y >= 0 && point.y <= 1)) return null;
  // The native calibration accepts the half-open range [0, 1). A click on the
  // right/bottom border belongs to the last source pixel, not outside the frame.
  return { x: Math.min(point.x, maxNormalizedCoordinate), y: Math.min(point.y, maxNormalizedCoordinate) };
}

export const maxNormalizedCoordinate = 1 - 2 ** -20;
