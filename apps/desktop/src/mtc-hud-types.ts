import type { Calibration, Point, WeaponResolution } from './types';

/** Mirrors `mtc_vision::PanelText` (camelCase JSON). */
export type PanelText = { text: string; catalogMatch: string | null; confidence: number; attempts: string[] };
/** Mirrors `mtc_vision::PanelValue`. */
export type PanelValue = { value: number; text: string; confidence: number; templateCost: number };
/** Mirrors `mtc_vision::PixelBox`: half-open client-pixel rectangle [x0, x1) × [y0, y1). */
export type PixelBox = { x0: number; y0: number; x1: number; y1: number };
export type WeaponPanelLayout = { nameBox: PixelBox; rangeBox: PixelBox | null; angleBox: PixelBox | null; scale: number };
export type WeaponPanelReading = {
  schemaVersion: number; status: 'ready' | 'partial' | 'blocked'; timestampMs: number;
  frameWidth: number; frameHeight: number; layout: WeaponPanelLayout | null;
  weapon: PanelText | null; ammo: PanelText | null;
  rangeMeters: number | null; rangeText: string | null;
  elevationDeg: PanelValue | null; azimuthDeg: PanelValue | null;
  warnings: string[];
};
/** `src-tauri/src/mtc_hud.rs` `WeaponPanelAnalysis`. */
export type WeaponPanelAnalysis = { panel: WeaponPanelReading; profile: WeaponResolution | null; backend: string };

/** Grid line rectangle in client pixels of the analysed frame. */
export type MtcMapGrid = { left: number; top: number; right: number; bottom: number; cellWidth: number; cellHeight: number; columns: number; rows: number; lineStrength: number; frameWidth: number; frameHeight: number };
export type MtcMapCandidate = { id: string; name: string; sizeGameMeters: number; heightGameMeters: number; cellMeters: number; deltaMeters: number };
export type MtcMapReading = {
  schemaVersion: number; status: 'ready' | 'ambiguous' | 'unidentified' | 'blocked'; timestampMs: number;
  frameWidth: number; frameHeight: number; grid: MtcMapGrid | null;
  gridSizeMeters: number | null; gridSizeOcr: string[]; mapId: string | null; candidates: MtcMapCandidate[];
  mapWidthMeters: number | null; mapHeightMeters: number | null;
  /** Client pixels → game meters; origin at the SW grid corner, x east, y north. */
  calibration: Calibration | null; warnings: string[];
};
/** `mtc_map_point`: fraction 0–1 from the west (x) and north (y) grid edge. */
export type MtcMapPoint = { fraction: Point; world: Point | null; sector: string };
