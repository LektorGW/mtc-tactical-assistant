import { useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { call, native } from './api';
import type { Result, Observation } from './types';
import { fresh } from './validation';
import type { OverlayDirectCard } from './DirectFirePanel';
import './ballistics.css';
/** Quick tank / hand-held line: numbers for the player only, no target markers. */
export type OverlayQuick = { line: string; profile: string | null; source: 'panel' | 'selected' | null; rangefinder: boolean };
export type OverlayAxis = { delta: number; aligned: boolean; label: string } | null;
export type OverlayAim = { elevation: OverlayAxis; azimuth: OverlayAxis; onTarget: boolean; panelTimestampMs: number; highArc: boolean };
export type OverlaySalvo = { status: 'experimental' | 'beta'; salvoSize: number; semiMajor: number; semiMinor: number; unit: string; incomplete: boolean };
export type OverlayMap = { name: string; ownSector: string | null; targetSector: string | null; heightDelta: number | null; heightSource: 'terrain' | 'manual' | null };
export type OverlayData = { result: Result | null; unit: string; profile: string; observation: Observation | null; observations?: Observation[]; mode: string; warning: string; training: boolean; positionsTimestamp?: number|null;experimental?:boolean; map?: OverlayMap; salvo?: OverlaySalvo | null; aim?: OverlayAim | null; quick?: OverlayQuick | null; direct?: OverlayDirectCard | null };
export function Overlay() {
  const [data, setData] = useState<OverlayData | null>(null);
  const [click, setClick] = useState(false);
  const [, setClock] = useState(Date.now());
  const now=Date.now();
  useEffect(() => {
    if (!native) return;
    const off = listen<OverlayData>('tactical-update', e => setData(e.payload));
    const offClick = listen<{clickThrough: boolean; visible: boolean}>('overlay-state', e => setClick(e.payload.clickThrough));
    call<OverlayData>('latest').then(setData).catch(() => {});
    const timer = setInterval(() => setClock(Date.now()), 500);
    return () => { void off.then(f => f()); void offClick.then(f => f()); clearInterval(timer); };
  }, []);
  const o = data?.observation ?? null;
  const resultFresh = !data?.positionsTimestamp || data.positionsTimestamp<=now && now-data.positionsTimestamp<=2000;
  const result = resultFresh?data?.result:null;
  const arc=result?.intercept3d?.arcs[0]??result?.solution?.arcs[0];
  const azimuth=result?.intercept3d?.arcs[0]?.azimuthDeg??result?.bearing.azimuthDeg;
  const heading=data?.observations?.find(o=>o.kind==='heading'&&fresh(o,now)&&(data.mode!=='automatic'||o.confidence>=0.85));
  const turn=heading&&azimuth!==undefined?((azimuth-heading.value+540)%360)-180:null;
  // Aim advice from the in-game panel; hidden when its reading is older than 2 s.
  const aim=result&&data?.aim&&data.aim.panelTimestampMs<=now+250&&now-data.aim.panelTimestampMs<=2000?data.aim:null;
  return <div className="overlay-card">
    <div className="overlay-drag" onPointerDown={() => native && getCurrentWindow().startDragging()}><span className="brand-mark">⌖</span><b>MTC / TACTICAL</b><span className="status-tag">{click ? 'PRZELOT KLIKÓW' : 'EDYCJA'}</span></div>
    <div className="overlay-profile">{data?.profile || 'Oczekiwanie na konfigurację'}{data?.training && <b className="training-label"> • SYMULACJA</b>}{data?.experimental && <b className="training-label"> • COMMUNITY / BETA</b>}</div>
    {data?.quick && <div className="overlay-quick" role="status"><strong>{data.quick.line}</strong><small>{data.quick.profile ?? 'brak profilu'}{data.quick.source === 'panel' ? ' · z panelu broni' : ''} · Numpad +/−/0 · model uproszczony</small></div>}
    {data?.direct && <table className="overlay-direct"><caption>{data.direct.profile} · {data.direct.status}{data.direct.zeroRange ? ` · zero ${data.direct.zeroRange} m` : ''}</caption><tbody>{data.direct.rows.map(r => <tr key={r.range}><td>{r.range} m</td><td>{r.holdMrad === null ? '—' : `${r.holdMrad >= 0 ? '+' : '−'}${Math.abs(r.holdMrad).toFixed(1)} mrad`}</td><td>{r.drop === null ? '—' : `${r.drop.toFixed(2)} m`}</td><td>{r.tof === null ? '—' : `${r.tof.toFixed(2)} s`}</td></tr>)}</tbody>{(data.direct.lead || data.direct.heli) && <tfoot><tr><td colSpan={4}>{[data.direct.lead, data.direct.heli].filter(Boolean).join(' · ')}</td></tr></tfoot>}</table>}
    <div className="overlay-metrics"><div><small>DYSTANS</small><strong>{result?.bearing.distance.toFixed(1) ?? '—'}<em>{data?.unit}</em></strong></div><div><small>AZYMUT</small><strong>{azimuth?.toFixed(1) ?? '—'}<em>°</em></strong></div></div>
    {data?.map && (data.map.ownSector || data.map.targetSector) && <div className="overlay-detail overlay-map">{data.map.ownSector ?? '—'} → {data.map.targetSector ?? '—'} · Δh {result && data.map.heightDelta !== null ? `${data.map.heightDelta >= 0 ? '+' : ''}${data.map.heightDelta.toFixed(1)} ${data.unit}${data.map.heightSource === 'terrain' ? ' (teren)' : ' (ręcznie)'}` : 'brak danych'}</div>}
    {aim && <div className={`overlay-aim ${aim.onTarget ? 'on-target' : ''}`} role="status">{aim.onTarget ? <strong>✓ NA CELU</strong> : <>{[aim.elevation, aim.azimuth].map((axis, i) => <strong key={i} className={axis?.aligned ? 'aligned' : ''}>{axis?.label ?? (i ? 'AZYMUT: brak odczytu' : 'ELEWACJA: brak odczytu')}</strong>)}</>}<small>{aim.highArc ? 'tor wysoki' : 'tor niski'} · tylko podpowiedź</small></div>}
    {!aim && result && data?.salvo && <div className="overlay-detail overlay-salvo">Salwa {data.salvo.status === 'beta' ? 'beta' : 'eksp.'} ×{data.salvo.salvoSize}: 95% {data.salvo.semiMajor.toFixed(0)}×{data.salvo.semiMinor.toFixed(0)} {data.unit}{data.salvo.incomplete ? ' · model niepełny' : ''}</div>}
    <div className="overlay-detail">Elewacja: <b>{arc?.elevationDeg.toFixed(2) ?? 'brak danych'}{arc && '°'}</b> · Lot: <b>{arc?.flightTime.toFixed(2) ?? '—'} s</b>{result?.intercept3d&&' · 3D'}</div>
    {!aim && <div className="overlay-hud">HUD: {fresh(o, now) ? `${o?.kind}: ${o?.kind === 'timer' ? `${Math.floor(o.value / 60)}:${String(o.value % 60).padStart(2, '0')}` : o?.value} · ${Math.round((o?.confidence ?? 0) * 100)}% heur.` : 'brak świeżego odczytu'}</div>}
    {!aim && turn!==null && <div className="overlay-detail">Kompas {heading?.value}° · korekta {turn<0?'w lewo':'w prawo'} {Math.abs(turn).toFixed(1)}°</div>}
    <p className="overlay-warning">{!resultFresh?'Pozycje z obrazu utraciły ważność':result?.ballisticWarning || data?.warning || (data?.experimental?'Model społeczności; zgodność aktualnej gry niepotwierdzona':'Dane ręczne; sprawdź kalibrację')}</p>
    <footer>Ctrl+Shift+O ukryj · I kliknięcia · M panel</footer>
  </div>;
}
