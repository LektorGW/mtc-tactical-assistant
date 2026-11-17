import { useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { call, native, profileFor } from './api';
import type { Profile } from './types';
import { DEFAULT_PRESETS, quickLine, stepPreset, type QuickAim } from './direct-fire';

export type QuickConfig = {
  enabled: boolean; presets: number[]; index: number; threshold: number;
  sightHeight: number; zeroRange: number | null; profileId: string | null; showCard: boolean;
};
export type HotkeyStatus = { enabled: boolean; registered: string[]; errors: string[] };
const STORAGE_KEY = 'mtc-quick-aim-v1';
export const defaultQuickConfig: QuickConfig = { enabled: false, presets: DEFAULT_PRESETS, index: -1, threshold: 1, sightHeight: 0, zeroRange: null, profileId: null, showCard: false };

function load(): QuickConfig {
  try {
    const raw = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? 'null') as Partial<QuickConfig> | null;
    if (!raw || typeof raw !== 'object') return defaultQuickConfig;
    const presets = Array.isArray(raw.presets) && raw.presets.length && raw.presets.every(v => typeof v === 'number' && v > 0) ? raw.presets : DEFAULT_PRESETS;
    // Hotkeys never start registered: the quick mode is always off after a restart.
    return { ...defaultQuickConfig, ...raw, presets, enabled: false, index: -1 };
  } catch { return defaultQuickConfig; }
}

/**
 * "Czołg / broń ręczna — szybki": assumed range from presets (Numpad + / − / 0)
 * or the game's rangefinder, profile from the live weapon panel or the last
 * chosen direct-fire profile. Produces numbers only; no target detection.
 */
export function useQuickAim({ profiles, overrides, panelProfileId, rangefinderMeters }: {
  profiles: Profile[]; overrides: Record<string, Profile>; panelProfileId: string | null; rangefinderMeters: number | null;
}) {
  const [config, setConfig] = useState<QuickConfig>(load);
  const [hotkeys, setHotkeys] = useState<HotkeyStatus | null>(null);
  const [quick, setQuick] = useState<{ key: string; value: QuickAim | null; error: string | null } | null>(null);
  useEffect(() => { try { localStorage.setItem(STORAGE_KEY, JSON.stringify(config)); } catch { /* per-viewer convenience only */ } }, [config]);
  useEffect(() => {
    if (!native) { setHotkeys(config.enabled ? { enabled: true, registered: [], errors: ['Skróty globalne wymagają aplikacji Windows'] } : null); return; }
    let alive = true;
    void call<HotkeyStatus>('quick_range_hotkeys', { enabled: config.enabled }).then(s => { if (alive) setHotkeys(s); }).catch(e => { if (alive) setHotkeys({ enabled: config.enabled, registered: [], errors: [String(e)] }); });
    return () => { alive = false; };
  }, [config.enabled]);
  useEffect(() => () => { if (native) void call('quick_range_hotkeys', { enabled: false }).catch(() => {}); }, []);
  useEffect(() => {
    if (!native || !config.enabled) return;
    const off = listen<string>('quick-range', event => setConfig(c => ({ ...c, index: stepPreset(c.index, event.payload, c.presets.length) })));
    return () => { void off.then(f => f()); };
  }, [config.enabled]);

  const panelProfile = panelProfileId ? profileFor(profiles, panelProfileId, overrides) : undefined;
  const chosen = config.profileId ? profileFor(profiles, config.profileId, overrides) : undefined;
  const profile = panelProfile ?? chosen;
  const profileSource: 'panel' | 'selected' | null = panelProfile ? 'panel' : chosen ? 'selected' : null;
  const rangefinder = rangefinderMeters !== null && Number.isFinite(rangefinderMeters) && rangefinderMeters > 0;
  const range = rangefinder ? rangefinderMeters : config.index >= 0 ? config.presets[Math.min(config.index, config.presets.length - 1)] ?? null : null;
  const missingSpeed = !!profile && !(typeof profile.speed === 'number' && profile.speed > 0);
  const key = JSON.stringify([profile, range, config.threshold, config.sightHeight, config.zeroRange]);
  useEffect(() => {
    if (!config.enabled || !profile || missingSpeed) return;
    let alive = true;
    void call<QuickAim>('quick_aim', { profile, input: { unit: profile.unit ?? 'game_units', range, sightHeight: config.sightHeight, zeroRange: config.zeroRange, centerThreshold: config.threshold } })
      .then(value => { if (alive) setQuick({ key, value, error: null }); })
      .catch(e => { if (alive) setQuick({ key, value: null, error: String(e) }); });
    return () => { alive = false; };
  }, [key, config.enabled]);
  const current = quick?.key === key ? quick : null;
  const line = !profile ? 'brak profilu — wybierz broń w zakładce „Broń bezpośrednia”'
    : quickLine(current?.value ?? null, { rangefinder, missingSpeed: !profile.speed, error: current?.error ?? null });
  return { config, setConfig, hotkeys, profile, profileSource, quick: current?.value ?? null, line, range, rangefinder };
}
export type QuickAimState = ReturnType<typeof useQuickAim>;
