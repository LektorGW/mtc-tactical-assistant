import { useEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { call, native } from './api';
import { fractionToTexturePixel } from './aim-correction';
import type { CatalogLoad } from './useCatalogMap';
import type { CommunityMap, Point, Settings } from './types';

export const MAP_POINT_EVENT = 'mtc-map-point-picked';
export const DEFAULT_TARGET_SHORTCUT = 'Ctrl+Shift+T';
export const DEFAULT_OWN_SHORTCUT = 'Ctrl+Shift+G';

export type MapPointRole = 'target' | 'own';
/** Payload of the native `mtc-map-point-picked` event (`map_hotkeys.rs`). */
export type MapPointPicked = {
  role: MapPointRole; ok: boolean; error: string | null;
  fraction: Point | null; world: Point | null; sector: string | null;
  mapId: string | null; mapStatus: 'ready' | 'ambiguous' | 'unidentified' | 'blocked' | null;
  candidates: string[]; gridSizeMeters: number | null; timestampMs: number; cursor: Point | null;
};
export type MapHotkeyStatus = { enabled: boolean; targetShortcut: string; ownShortcut: string; targetRegistered: boolean; ownRegistered: boolean; errors: string[] };

export type MapPointPlan =
  | { kind: 'apply'; role: MapPointRole; texture: Point; sector: string; note: string | null }
  | { kind: 'load'; role: MapPointRole; mapId: string; fraction: Point; sector: string }
  | { kind: 'error'; message: string };

const roleName = (role: MapPointRole) => role === 'own' ? 'działa' : 'celu';

/**
 * Decides what a picked point means for the current settings. The same texture
 * conversion as a click in the live map view is used; a different identified
 * map is loaded first; an unlabeled grid (minimap) is accepted only for the map
 * that is already loaded, with an explicit note.
 */
export function planMapPoint(event: MapPointPicked, s: Pick<Settings, 'communityMapId' | 'mapSource' | 'mapWidth' | 'mapHeight'>, maps: Pick<CommunityMap, 'id' | 'name'>[]): MapPointPlan {
  if (!event.ok || !event.fraction || !event.sector) return { kind: 'error', message: event.error ?? 'Brak punktu mapy' };
  const current = s.mapSource === 'catalog' ? s.communityMapId : null;
  let mapId = event.mapId;
  let note: string | null = null;
  if (!mapId && event.mapStatus === 'ambiguous' && current && event.candidates.includes(current)) mapId = current;
  if (!mapId && event.mapStatus === 'unidentified' && current) {
    mapId = current;
    note = `Mapa w grze nierozpoznana (brak podpisu „Grid size”) — punkt przyjęto dla bieżącej mapy ${maps.find(m => m.id === current)?.name ?? current}.`;
  }
  if (!mapId) return { kind: 'error', message: event.mapStatus === 'ambiguous' ? 'Kilka map pasuje do rozmiaru pola — wybierz mapę ręcznie i spróbuj ponownie.' : 'Nie rozpoznano mapy w grze — wczytaj mapę ręcznie i spróbuj ponownie.' };
  if (!maps.some(m => m.id === mapId)) return { kind: 'error', message: `Mapa ${mapId} nie występuje w katalogu.` };
  if (mapId !== current) return { kind: 'load', role: event.role, mapId, fraction: event.fraction, sector: event.sector };
  const texture = fractionToTexturePixel(event.fraction, s.mapWidth, s.mapHeight);
  if (!texture) return { kind: 'error', message: 'Punkt poza siatką mapy' };
  return { kind: 'apply', role: event.role, texture, sector: event.sector, note };
}

type Options = {
  enabled: boolean;
  settings: Settings;
  update: (patch: Partial<Settings>) => void;
  handle: number | null;
  backend: 'wgc' | 'gdi';
  communityMaps: CommunityMap[];
  openCatalog: (known: CommunityMap) => Promise<CatalogLoad | null>;
  notice: (message: string) => void;
  targetShortcut?: string;
  ownShortcut?: string;
};

/**
 * Global hotkeys: the mouse cursor over the open in-game map (key M) sets the
 * target (Ctrl+Shift+T) or own position (Ctrl+Shift+G). Read-only — only the
 * cursor position and the visible window are read; nothing is sent to the game.
 */
export function useMapPointHotkeys(options: Options) {
  const { enabled, settings, update, handle, backend, communityMaps, openCatalog, notice } = options;
  const targetShortcut = options.targetShortcut ?? DEFAULT_TARGET_SHORTCUT;
  const ownShortcut = options.ownShortcut ?? DEFAULT_OWN_SHORTCUT;
  const [status, setStatus] = useState<MapHotkeyStatus | null>(null);
  const [last, setLast] = useState<MapPointPicked | null>(null);
  const [pending, setPending] = useState<Extract<MapPointPlan, { kind: 'load' }> | null>(null);
  const latest = useRef({ settings, communityMaps, update, openCatalog, notice });
  latest.current = { settings, communityMaps, update, openCatalog, notice };

  useEffect(() => {
    if (!native) return;
    let cancelled = false;
    void call<MapHotkeyStatus>('configure_map_point_hotkeys', { request: { enabled: enabled && handle !== null, handle, backend, targetShortcut, ownShortcut } })
      .then(next => { if (!cancelled) setStatus(next); })
      .catch(error => { if (!cancelled) latest.current.notice(String(error)); });
    return () => {
      cancelled = true;
      void call('configure_map_point_hotkeys', { request: { enabled: false, handle: null, backend, targetShortcut, ownShortcut } }).catch(() => {});
    };
  }, [enabled, handle, backend, targetShortcut, ownShortcut]);

  const apply = (plan: Extract<MapPointPlan, { kind: 'apply' }>) => {
    const { settings: s, update: set, notice: say } = latest.current;
    set({ [plan.role]: plan.texture, mode: s.mode === 'automatic' ? 'assisted' : s.mode });
    say(`${plan.note ? `${plan.note} ` : ''}Ustawiono pozycję ${roleName(plan.role)} z mapy w grze: ${plan.sector}.`);
  };

  useEffect(() => {
    if (!native) return;
    let unlisten: (() => void) | undefined;
    let disposed = false;
    void listen<MapPointPicked>(MAP_POINT_EVENT, ({ payload }) => {
      setLast(payload);
      const { settings: s, communityMaps: maps, openCatalog: open, notice: say } = latest.current;
      const plan = planMapPoint(payload, s, maps);
      if (plan.kind === 'error') { say(`Wskazanie ${roleName(payload.role)}: ${plan.message}`); return; }
      if (plan.kind === 'apply') { apply(plan); return; }
      const known = maps.find(m => m.id === plan.mapId)!;
      if (!s.catalogSourceAccepted) { say(`Rozpoznano mapę ${known.name}. Zaakceptuj źródło map społeczności, aby ją wczytać.`); return; }
      setPending(plan);
      void open(known).then(load => {
        if (load?.phase === 'error') { setPending(null); say(load.message); }
      });
    }).then(stop => { if (disposed) stop(); else unlisten = stop; });
    return () => { disposed = true; unlisten?.(); };
  }, []);

  // Apply a point picked on another map once that catalog map is loaded.
  useEffect(() => {
    if (!pending || settings.mapSource !== 'catalog' || settings.communityMapId !== pending.mapId) return;
    const texture = fractionToTexturePixel(pending.fraction, settings.mapWidth, settings.mapHeight);
    setPending(null);
    if (texture) apply({ kind: 'apply', role: pending.role, texture, sector: pending.sector, note: null });
  }, [pending, settings.mapSource, settings.communityMapId, settings.mapWidth, settings.mapHeight]);

  return { status, last, pending: pending !== null };
}
