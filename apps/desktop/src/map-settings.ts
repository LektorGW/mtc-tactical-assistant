import { MAX_GRID_DIVISIONS } from './map-geometry';
import { defaultSettings, type MapGrid, type Settings } from './types';

/**
 * Fills and validates the map-workflow fields added to settings-v4 in 0.5.
 * `validateSettings` already merges defaults; this function decides migration
 * values from the raw file (absent ≠ default) and repairs malformed values,
 * so an older or hand-edited file can never block the rest of the settings.
 */
export function normalizeMapSettings(raw: unknown, settings: Settings): Settings {
  const r = (raw && typeof raw === 'object' ? raw : {}) as Record<string, unknown>;
  const has = (key: keyof Settings) => Object.prototype.hasOwnProperty.call(r, key);
  const catalogActive = !!settings.communityMapId && settings.maps.some(m => m.id === settings.communityMapId && m.path === settings.mapPath);
  const mapSource: Settings['mapSource'] = has('mapSource') && (r.mapSource === 'catalog' || r.mapSource === 'custom')
    ? r.mapSource
    : catalogActive ? 'catalog' : settings.mapPath || settings.markerConfig.viewSignature ? 'custom' : 'catalog';
  // Before 0.5 the source was accepted per library import; confirmed community
  // entries already in the library imply that one-time acceptance.
  const catalogSourceAccepted = has('catalogSourceAccepted') && typeof r.catalogSourceAccepted === 'boolean'
    ? r.catalogSourceAccepted
    : settings.maps.some(m => m.confirmed && m.source.startsWith('Community '));
  return {
    ...settings,
    mapSource,
    catalogSourceAccepted,
    mapGrid: normalizeGrid(has('mapGrid') ? r.mapGrid : undefined),
    useTerrainHeight: typeof r.useTerrainHeight === 'boolean' ? r.useTerrainHeight : true,
    gameGridSize: typeof r.gameGridSize === 'number' && Number.isFinite(r.gameGridSize) && r.gameGridSize > 0 ? r.gameGridSize : null,
  };
}

export function normalizeGrid(value: unknown): MapGrid {
  const fallback = defaultSettings.mapGrid;
  const g = (value && typeof value === 'object' ? value : {}) as Partial<MapGrid>;
  return {
    visible: typeof g.visible === 'boolean' ? g.visible : fallback.visible,
    divisions: Number.isInteger(g.divisions) && g.divisions! >= 1 && g.divisions! <= MAX_GRID_DIVISIONS ? g.divisions! : fallback.divisions,
    columns: g.columns === 'letters' || g.columns === 'numbers' || g.columns === 'letters-no-i' ? g.columns : fallback.columns,
    rowsFrom: g.rowsFrom === 'bottom' || g.rowsFrom === 'top' ? g.rowsFrom : fallback.rowsFrom,
  };
}
