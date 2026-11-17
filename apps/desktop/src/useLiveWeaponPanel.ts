import { useEffect, useState } from 'react';
import { call } from './api';
import type { WeaponPanelAnalysis } from './mtc-hud-types';

export const LIVE_PANEL_INTERVAL_MS = 500;

/**
 * Sequential ~2 Hz reading of the in-game weapon panel. The next request starts
 * only after the previous one finished, so calls never overlap. Lives in App so
 * the overlay keeps its correction while other tabs are open. Read-only: it never
 * sends input to the game.
 */
export function useLiveWeaponPanel(handle: number | null, backend: 'wgc' | 'gdi') {
  const [running, setRunning] = useState(false);
  const [analysis, setAnalysis] = useState<WeaponPanelAnalysis | null>(null);
  const [error, setError] = useState('');
  useEffect(() => { setRunning(false); setAnalysis(null); setError(''); }, [handle, backend]);
  useEffect(() => {
    if (!running || handle === null) return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const tick = async () => {
      const started = performance.now();
      try {
        const next = await call<WeaponPanelAnalysis>('read_weapon_panel', { handle, backend });
        if (!cancelled) { setAnalysis(next); setError(''); }
      } catch (e) { if (!cancelled) setError(String(e)); }
      if (!cancelled) timer = setTimeout(() => void tick(), Math.max(0, LIVE_PANEL_INTERVAL_MS - (performance.now() - started)));
    };
    void tick();
    return () => { cancelled = true; if (timer) clearTimeout(timer); };
  }, [running, handle, backend]);
  return { running, setRunning, analysis, error };
}
