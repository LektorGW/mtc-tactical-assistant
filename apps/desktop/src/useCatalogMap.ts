import { useState } from 'react';
import { call as nativeCall } from './api';
import { applyCatalogMap, isCatalogEntry, isCurrentTerrain } from './map-catalog';
import type { CatalogMapResult, CommunityMap, Settings, StoredMap, StoredTerrain } from './types';
import { validateSettings } from './validation';
import { endSimulation } from './simulation';

type Image = { dataUrl: string; width: number; height: number; sha256: string };
export type CatalogLoad = { id: string; phase: 'idle' | 'loading' | 'ready' | 'error'; message: string; warnings: string[]; origin: 'library' | 'cache' | 'downloaded' | null };
export type CatalogDeps = {
  settings: () => Settings;
  setSettings: (next: (current: Settings) => Settings) => void;
  showImage: (dataUrl: string, sha256: string) => void;
  log: (event: string) => void;
  call?: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
};

/**
 * App-wide map generation. Any other map change (own image import, simulation,
 * visible-view reference, source switch) bumps it, so a slower catalog download
 * started earlier can never overwrite that newer choice — also after the map
 * panel was unmounted and mounted again.
 */
let mapGeneration = 0;
export function invalidateCatalogLoads(): number { return ++mapGeneration; }

/** Loads one catalog map. Returns null when superseded by a newer map change. */
export async function loadCatalogMap(known: CommunityMap, deps: CatalogDeps, progress: (load: CatalogLoad) => void = () => {}): Promise<CatalogLoad | null> {
  const call = deps.call ?? nativeCall;
  const s = deps.settings();
  if (!s.catalogSourceAccepted) throw new Error('Najpierw potwierdź eksperymentalne źródło map społeczności');
  const token = invalidateCatalogLoads();
  const current = () => token === mapGeneration;
  progress({ id: known.id, phase: 'loading', message: `Wczytywanie mapy ${known.name}…`, warnings: [], origin: null });
  const warnings: string[] = [];
  try {
    let map: StoredMap | null = null, terrain: StoredTerrain | null = null, image: Image | null = null;
    let origin: CatalogLoad['origin'] = null;
    const existing = s.maps.find(m => isCatalogEntry(m, known));
    if (existing) {
      try {
        const img = await call<Image>('import_image', { path: existing.path });
        if (img.sha256 === existing.sha256 && img.width === existing.width && img.height === existing.height) { map = existing; image = img; origin = 'library'; }
      } catch { /* Missing library file: fall back to cache/download below. */ }
      const stored = s.terrains.find(t => t.mapId === known.id);
      if (map && isCurrentTerrain(stored, map)) terrain = stored;
    }
    if (!current()) return null;
    const needsTerrain = !!known.heightRangeStuds && !terrain;
    if (!map || needsTerrain) {
      try {
        const prepared = await call<CatalogMapResult>('prepare_catalog_map', { id: known.id, includeTerrain: true, allowOnline: true, sourceAcknowledged: true });
        if (!current()) return null;
        warnings.push(...prepared.warnings);
        if (!map || prepared.map.sha256 !== map.sha256) {
          const img = await call<Image>('import_image', { path: prepared.map.path });
          if (img.sha256 !== prepared.map.sha256 || img.width !== prepared.map.width || img.height !== prepared.map.height) throw new Error('Plik mapy w cache zmienił się podczas wczytywania — spróbuj ponownie');
          map = prepared.map; image = img; origin = prepared.origin;
        }
        terrain = prepared.terrain && prepared.terrain.mapWidth === map.width && prepared.terrain.mapHeight === map.height ? prepared.terrain : null;
      } catch (error) {
        if (!map) throw error;
        warnings.push(`Mapa wysokości niedostępna: ${String(error)}. Różnicę wysokości możesz wpisać ręcznie.`);
      }
    }
    if (!current() || !map || !image) return null;
    const resolvedMap = map, resolvedTerrain = terrain;
    // Validate before committing so an invalid library never silently stops autosave.
    // A real map ends the simulation (only its synthetic map/profile are removed).
    validateSettings(applyCatalogMap(endSimulation(deps.settings()), known, resolvedMap, resolvedTerrain));
    deps.setSettings(currentSettings => applyCatalogMap(endSimulation(currentSettings), known, resolvedMap, resolvedTerrain));
    deps.showImage(image.dataUrl, image.sha256);
    deps.log(`Mapa z katalogu: ${known.name}; ${origin === 'downloaded' ? 'pobrano' : origin === 'cache' ? 'lokalny cache' : 'biblioteka'}; skala ${resolvedMap.calibration.unitsPerPixel.toFixed(6)} m gry/px${resolvedTerrain ? '; teren dostępny' : ''}`);
    return { id: known.id, phase: 'ready', message: `${known.name}: ${origin === 'downloaded' ? 'pobrano do lokalnego cache' : origin === 'cache' ? 'wczytano z lokalnego cache' : 'wczytano z biblioteki'}.`, warnings, origin };
  } catch (error) {
    if (!current()) return null;
    return { id: known.id, phase: 'error', message: String(error).replace(/^Error: /, ''), warnings, origin: null };
  }
}

/** One click: verified library entry, else local cache, else pinned download. */
export function useCatalogMap(deps: CatalogDeps) {
  const [load, setLoad] = useState<CatalogLoad>({ id: '', phase: 'idle', message: '', warnings: [], origin: null });
  const open = async (known: CommunityMap) => {
    const result = await loadCatalogMap(known, deps, setLoad);
    // A superseded load clears its own spinner; the newer action owns the map now.
    setLoad(previous => result ?? (previous.phase === 'loading' && previous.id === known.id ? { ...previous, phase: 'idle', message: '' } : previous));
  };
  return { load, open };
}
