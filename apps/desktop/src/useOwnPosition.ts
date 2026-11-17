import { useEffect, useRef, useState } from 'react';
import { call, native } from './api';
import { fractionToTexturePixel } from './aim-correction';
import type { Point, Settings } from './types';

export const OWN_POSITION_INTERVAL_MS = 700;
/** Applied only after two consistent frames with compass agreement (Rust heuristic 0.9). */
export const OWN_POSITION_MIN_CONFIDENCE = 0.85;

/** Mirrors `mtc_vision::OwnPositionReading` (`read_own_position`). */
export type OwnMarker = {
  pixel: Point; fraction: Point; sector: string; world: Point | null;
  headingDeg: number; shapeScore: number; compassDeg: number | null; compassAgrees: boolean | null; confidence: number;
};
export type OwnPositionReading = {
  schemaVersion: number;
  status: 'ready' | 'unconfirmed' | 'ambiguous' | 'notFound' | 'blocked';
  timestampMs: number; frameWidth: number; frameHeight: number;
  mapId: string | null; marker: OwnMarker | null;
  candidates: { pixel: Point; area: number; shapeScore: number; headingDeg: number }[];
  warnings: string[];
};

export type OwnPositionDecision = { apply: Point | null; reason: string };

/**
 * Same conversion as Ctrl+Shift+G: the fraction becomes a pixel of the loaded
 * catalog texture. Only a confirmed, stabilized reading for the loaded catalog
 * map is applied; tiny jitter (< 0.5 px) does not rewrite the settings.
 */
export function ownPositionDecision(reading: OwnPositionReading, s: Pick<Settings, 'mapSource' | 'communityMapId' | 'mapWidth' | 'mapHeight' | 'own'>): OwnPositionDecision {
  if (s.mapSource !== 'catalog' || !s.communityMapId) return { apply: null, reason: 'Wczytaj mapę z katalogu — minimapa nie podaje nazwy mapy.' };
  if (reading.mapId !== s.communityMapId) return { apply: null, reason: 'Odczyt dotyczy innej mapy niż wczytana.' };
  const marker = reading.marker;
  if (!marker) return { apply: null, reason: reading.warnings[0] ?? 'Brak strzałki gracza.' };
  if (reading.status !== 'ready') return { apply: null, reason: reading.warnings[0] ?? 'Pozycja niepotwierdzona kompasem.' };
  if (marker.confidence < OWN_POSITION_MIN_CONFIDENCE) return { apply: null, reason: 'Czekam na drugą zgodną klatkę.' };
  const texture = fractionToTexturePixel(marker.fraction, s.mapWidth, s.mapHeight);
  if (!texture) return { apply: null, reason: 'Punkt poza mapą.' };
  if (s.own && Math.hypot(s.own.x - texture.x, s.own.y - texture.y) < 0.5) return { apply: null, reason: `Pozycja aktualna (${marker.sector}).` };
  return { apply: texture, reason: `Ustawiono pozycję działa z minimapy: ${marker.sector}.` };
}

/**
 * Beta: periodically reads the player arrow and sets the own (gun) position.
 * Off by default; read-only — nothing is sent to the game.
 */
export function useOwnPosition(enabled: boolean, settings: Settings, update: (patch: Partial<Settings>) => void, handle: number | null, backend: 'wgc' | 'gdi') {
  const [reading, setReading] = useState<OwnPositionReading | null>(null);
  const [message, setMessage] = useState('');
  const latest = useRef({ settings, update });
  latest.current = { settings, update };
  const mapId = settings.mapSource === 'catalog' ? settings.communityMapId : null;
  useEffect(() => {
    setReading(null); setMessage('');
    if (!native || !enabled || handle === null) return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const tick = async () => {
      const started = performance.now();
      try {
        const next = await call<OwnPositionReading>('read_own_position', { handle, backend, mapId });
        if (cancelled) return;
        setReading(next);
        const { settings: s, update: set } = latest.current;
        const decision = ownPositionDecision(next, s);
        setMessage(decision.reason);
        if (decision.apply) set({ own: decision.apply, mode: s.mode === 'automatic' ? 'assisted' : s.mode });
      } catch (e) { if (!cancelled) setMessage(String(e)); }
      if (!cancelled) timer = setTimeout(() => void tick(), Math.max(0, OWN_POSITION_INTERVAL_MS - (performance.now() - started)));
    };
    void tick();
    return () => { cancelled = true; if (timer) clearTimeout(timer); };
  }, [enabled, handle, backend, mapId]);
  return { reading, message };
}
