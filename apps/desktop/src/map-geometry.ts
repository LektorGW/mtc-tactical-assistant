import type { GridColumnStyle, MapGrid, Point } from './types';

/** Visible part of the map in image pixels (the SVG viewBox). */
export type View = { x: number; y: number; width: number; height: number };
/** Element box in CSS pixels, e.g. from getBoundingClientRect(). */
export type Box = { left: number; top: number; width: number; height: number };

export const MAX_GRID_DIVISIONS = 25;
const LETTERS = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ';
const LETTERS_NO_I = LETTERS.replace('I', '');

/**
 * Scale of a complete square community texture: the source calculator measures
 * distance as |Δ normalized XY| × full map size, i.e. size / texture width per pixel.
 * A non-square image would need two scales, so it is rejected explicitly.
 */
export function catalogScale(sizeGameMeters: number, width: number, height: number): number {
  if (!Number.isFinite(sizeGameMeters) || sizeGameMeters <= 0) throw new Error('Brak dodatniego wymiaru mapy w katalogu');
  if (!Number.isInteger(width) || !Number.isInteger(height) || width < 2 || height < 2) throw new Error('Niepoprawny rozmiar obrazu mapy');
  if (Math.abs(width / height - 1) > 0.01) throw new Error(`Obraz ${width}×${height} nie jest kwadratowy — wymiar katalogu dotyczy pełnej kwadratowej mapy`);
  return sizeGameMeters / width;
}

/** Same pixel→world convention as Rust `MapCalibration::transform` (north up = 0°, clockwise). */
/** Target minus own position in world [east, north] map units. */
export function worldDelta(own: Point, target: Point, unitsPerPixel: number, northRotationDeg: number): [number, number] | null {
  if (!Number.isFinite(unitsPerPixel) || unitsPerPixel <= 0 || !Number.isFinite(northRotationDeg)) return null;
  const east = (target.x - own.x) * unitsPerPixel;
  const north = -(target.y - own.y) * unitsPerPixel;
  const a = northRotationDeg * Math.PI / 180;
  return [east * Math.cos(a) + north * Math.sin(a), -east * Math.sin(a) + north * Math.cos(a)];
}

export function localBearing(own: Point, target: Point, unitsPerPixel: number, northRotationDeg: number): { distance: number; azimuthDeg: number } | null {
  const delta = worldDelta(own, target, unitsPerPixel, northRotationDeg);
  if (!delta) return null;
  const [x, y] = delta;
  const distance = Math.hypot(x, y);
  if (!Number.isFinite(distance) || distance < 1e-9) return null;
  const azimuthDeg = ((Math.atan2(x, y) * 180 / Math.PI) % 360 + 360) % 360;
  return { distance, azimuthDeg };
}

export function fitView(width: number, height: number): View {
  return { x: 0, y: 0, width, height };
}

/** Keeps the view inside the image; a view larger than the image is centered. */
export function clampView(view: View, width: number, height: number): View {
  const w = Math.min(view.width, width);
  const h = Math.min(view.height, height);
  const ratio = view.width / view.height;
  // Preserve the aspect ratio of the element (the view never exceeds the image).
  const fw = Math.min(w, h * ratio);
  const fh = fw / ratio;
  const x = Math.min(Math.max(view.x, 0), width - fw);
  const y = Math.min(Math.max(view.y, 0), height - fh);
  return { x, y, width: fw, height: fh };
}

/** Zooms by `factor` (>1 = closer) keeping the image point `anchor` under the cursor. */
export function zoomView(view: View, factor: number, anchor: Point, width: number, height: number, maxZoom = 16): View {
  if (!Number.isFinite(factor) || factor <= 0) return view;
  const minWidth = width / maxZoom;
  const nextWidth = Math.min(width, Math.max(minWidth, view.width / factor));
  const k = nextWidth / view.width;
  const next = {
    x: anchor.x - (anchor.x - view.x) * k,
    y: anchor.y - (anchor.y - view.y) * k,
    width: nextWidth,
    height: view.height * k,
  };
  return clampView(next, width, height);
}

export function panView(view: View, dx: number, dy: number, width: number, height: number): View {
  return clampView({ ...view, x: view.x + dx, y: view.y + dy }, width, height);
}

export function zoomLevel(view: View, width: number): number {
  return width / view.width;
}

/**
 * Converts a pointer position (CSS px, independent of DPI scaling) to image pixels
 * for an SVG with `viewBox = view` and the default `preserveAspectRatio="xMidYMid meet"`.
 * Uses the natural image size via the view, never the displayed size.
 */
export function clientToImage(clientX: number, clientY: number, box: Box, view: View): Point {
  const scale = Math.min(box.width / view.width, box.height / view.height);
  const offsetX = (box.width - view.width * scale) / 2;
  const offsetY = (box.height - view.height * scale) / 2;
  return {
    x: view.x + (clientX - box.left - offsetX) / scale,
    y: view.y + (clientY - box.top - offsetY) / scale,
  };
}

/** Pixel centers span 0..size−1; terrain sampling rejects points beyond the last pixel. */
export function clampToImage(p: Point, width: number, height: number): Point {
  return { x: Math.min(Math.max(p.x, 0), Math.max(0, width - 1)), y: Math.min(Math.max(p.y, 0), Math.max(0, height - 1)) };
}

export function columnLabels(count: number, style: GridColumnStyle): string[] {
  const n = Math.max(1, Math.min(MAX_GRID_DIVISIONS, Math.floor(count)));
  if (style === 'numbers') return Array.from({ length: n }, (_, i) => String(i + 1));
  const alphabet = style === 'letters-no-i' ? LETTERS_NO_I : LETTERS;
  return Array.from({ length: n }, (_, i) => alphabet[i]);
}

export function rowLabels(count: number, rowsFrom: MapGrid['rowsFrom']): string[] {
  const n = Math.max(1, Math.min(MAX_GRID_DIVISIONS, Math.floor(count)));
  return Array.from({ length: n }, (_, i) => String(rowsFrom === 'top' ? i + 1 : n - i));
}

export type Sector = { column: number; row: number; label: string };

/** Grid cell of an image point; the grid spans the whole (square) map image. */
export function sectorOf(p: Point, width: number, height: number, grid: MapGrid): Sector | null {
  if (!Number.isFinite(p.x) || !Number.isFinite(p.y) || p.x < 0 || p.y < 0 || p.x > width || p.y > height) return null;
  const n = Math.max(1, Math.min(MAX_GRID_DIVISIONS, Math.floor(grid.divisions)));
  const column = Math.min(n - 1, Math.floor(p.x / (width / n)));
  const row = Math.min(n - 1, Math.floor(p.y / (height / n)));
  return { column, row, label: `${columnLabels(n, grid.columns)[column]}${rowLabels(n, grid.rowsFrom)[row]}` };
}

/** Size of one grid cell in map units (e.g. game meters), for comparison with the in-game value. */
export function gridCellSize(unitsPerPixel: number, width: number, divisions: number): number {
  return unitsPerPixel * width / divisions;
}

export function formatDistance(value: number, unit: string): string {
  return `${value >= 1000 ? value.toFixed(0) : value.toFixed(1)} ${unit}`;
}

/** A point is created only when both coordinates are present and inside the image. */
export function pointFromFields(x: number | null, y: number | null, width: number, height: number): Point | null | 'incomplete' {
  if (x === null && y === null) return null;
  if (x === null || y === null || !Number.isFinite(x) || !Number.isFinite(y) || x < 0 || y < 0 || x > width || y > height) return 'incomplete';
  return { x, y };
}

