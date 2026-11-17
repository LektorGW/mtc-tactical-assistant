import { useEffect, useId, useRef, useState, type FormEvent } from 'react';
import { call } from './api';
import type { Profile } from './types';
import { outlinePath, prepareLocalSalvo, previewScale, type DynamicInterceptSolution, type ProfileWithSalvo, type SalvoDispersion, type SalvoDraft, type SalvoPattern } from './salvo';
import './ballistics.css';

export type SalvoArc = { elevationDeg: number; flightTime: number; highArc: boolean };
/** Static solution context from `calculate` (bearing + arcs + observed height). */
export type SalvoContext = { distance: number; azimuthDeg: number; heightDelta: number | null; arcs: SalvoArc[] };

function NumberInput({ id, label, value, onChange, min, max, step = 'any', required = false }: {
  id: string; label: string; value: number | null; onChange: (value: number | null) => void;
  min?: number; max?: number; step?: string; required?: boolean;
}) {
  return <div className="field">
    <label htmlFor={id}>{label}{required ? ' — wymagane' : ' — opcjonalne'}</label>
    <input id={id} type="number" inputMode="decimal" step={step} min={min} max={max} required={required} value={value ?? ''}
      onChange={event => onChange(event.target.value === '' ? null : event.target.valueAsNumber)} />
  </div>;
}
const fmt = (value: number, digits = 1) => Number.isFinite(value) ? value.toFixed(digits) : '—';
const statusLabel = (status: string) => status === 'beta' ? 'beta (lokalna kalibracja)' : 'eksperymentalny';

function draftFor(salvo: SalvoDispersion | null | undefined): SalvoDraft {
  return { angularSigmaMrad: salvo?.angularSigmaMrad ?? null, speedSigma: salvo?.speedSigma ?? null, rippleInterval: salvo?.rippleInterval ?? null, source: '', confirmed: false };
}

/** North-up SVG preview centred on the target; same data as the map contract. */
export function SalvoPreview({ pattern, unitLabel }: { pattern: SalvoPattern; unitLabel: string }) {
  const size = 240;
  const scale = previewScale(pattern, size);
  const c = size / 2;
  const [ce, cn] = pattern.centerOffset;
  const ring = (radius: number) => <circle className="area" cx={c} cy={c} r={radius * scale} />;
  const scaleBar = Math.pow(10, Math.floor(Math.log10((size * 0.3) / scale)));
  return <div className="salvo-preview">
    <svg viewBox={`0 0 ${size} ${size}`} role="img" aria-label={`Elipsy rozrzutu: pojedynczy pocisk ${fmt(pattern.singleProjectile.semiMajor)} na ${fmt(pattern.singleProjectile.semiMinor)} ${unitLabel}, salwa ${pattern.salvoSize}`}>
      <line className="grid-line" x1={c} y1={8} x2={c} y2={size - 8} />
      <line className="grid-line" x1={8} y1={c} x2={size - 8} y2={c} />
      <text className="north" x={c + 4} y={16}>N</text>
      {pattern.coverage?.areaRadius ? ring(pattern.coverage.areaRadius) : null}
      <path className="envelope" d={outlinePath(pattern.salvoEnvelope.outline, scale, size)} />
      <path className="single" d={outlinePath(pattern.singleProjectile.outline, scale, size)} />
      <path className="mpi" d={outlinePath(pattern.meanPointOfImpact.outline, scale, size)} />
      <path className="target" d={`M${c - 6} ${c} H${c + 6} M${c} ${c - 6} V${c + 6}`} />
      <circle className="center" cx={c + ce * scale} cy={c - cn * scale} r={2.5} />
      <line className="target" x1={12} y1={size - 12} x2={12 + scaleBar * scale} y2={size - 12} />
      <text className="north" x={12} y={size - 16}>{scaleBar} {unitLabel}</text>
    </svg>
    <ul className="salvo-legend">
      <li><span className="single" />Pojedynczy pocisk 95%: {fmt(pattern.singleProjectile.semiMajor)} × {fmt(pattern.singleProjectile.semiMinor)} {unitLabel}, oś {fmt(pattern.singleProjectile.majorAxisAzimuthDeg)}°</li>
      <li><span className="envelope" />Wszystkie {pattern.salvoSize} pociski (95%): {fmt(pattern.salvoEnvelope.semiMajor)} × {fmt(pattern.salvoEnvelope.semiMinor)} {unitLabel}</li>
      <li><span className="mpi" />Środek salwy 95%: {fmt(pattern.meanPointOfImpact.semiMajor)} × {fmt(pattern.meanPointOfImpact.semiMinor)} {unitLabel}</li>
      {pattern.coverage?.areaRadius ? <li><span className="area" />Obszar oceny pokrycia r = {fmt(pattern.coverage.areaRadius)} {unitLabel}</li> : null}
      <li>σ wzdłuż / w poprzek: {fmt(pattern.rangeSigma, 2)} / {fmt(pattern.crossRangeSigma, 2)} {unitLabel}</li>
      <li>Prognozowany środek względem celu: wschód {fmt(ce, 2)}, północ {fmt(cn, 2)} {unitLabel}</li>
    </ul>
  </div>;
}

type Props = {
  profile: Profile | undefined;
  context: SalvoContext | null;
  onApplySalvo: (salvo: SalvoDispersion | null) => void;
  /** Latest forecast for the map/overlay layer, or null when stale/cleared. */
  onPattern?: (pattern: SalvoPattern | null) => void;
  notice: (message: string) => void;
};

export function SalvoPanel({ profile, context, onApplySalvo, onPattern, notice }: Props) {
  const id = useId();
  const p = profile as ProfileWithSalvo | undefined;
  const [draft, setDraft] = useState<SalvoDraft>(() => draftFor(p?.salvo));
  const [arcIndex, setArcIndex] = useState(0);
  const [count, setCount] = useState<number | null>(null);
  const [effect, setEffect] = useState<number | null>(null);
  const [area, setArea] = useState<number | null>(null);
  const [result, setResult] = useState<{ scope: string; pattern: SalvoPattern } | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const version = useRef(0);
  const scope = JSON.stringify([p, context, arcIndex, count, effect, area]);
  const pattern = result?.scope === scope ? result.pattern : null;
  const onPatternRef = useRef(onPattern);
  onPatternRef.current = onPattern;
  useEffect(() => { onPatternRef.current?.(pattern); }, [pattern]);
  useEffect(() => { version.current++; setDraft(draftFor(p?.salvo)); setResult(null); setError(''); setBusy(false); }, [p?.id, p?.salvo, p?.unit, p?.calibrated]);
  useEffect(() => () => { version.current++; }, []);

  if (!p) return <section className="panel"><h2>Salwa i rozrzut</h2><p className="hint">Wybierz profil pocisku.</p></section>;
  if (p.category === 'guided_missile') return <section className="panel"><h2>Salwa i rozrzut</h2><p className="hint">Mechanika naprowadzania jest nieznana. Brak prognozy salwy.</p></section>;
  const unitLabel = (p.unit ?? 'game_units') === 'game_meters' ? 'm gry' : 'jedn. gry';
  const salvo = p.salvo && p.salvo.status !== 'no_data' ? p.salvo : null;
  const fail = (caught: unknown) => { const message = caught instanceof Error ? caught.message : String(caught); setError(message); notice(message); };
  const apply = (event: FormEvent) => {
    event.preventDefault();
    try { onApplySalvo(prepareLocalSalvo(p, draft)); setError(''); notice('Zapisano parametry rozrzutu salwy. Porównaj prognozę z obserwowanymi trafieniami.'); }
    catch (caught) { fail(caught); }
  };
  const forecast = async (event: FormEvent) => {
    event.preventDefault();
    const request = ++version.current;
    try {
      const arc = context?.arcs[arcIndex];
      if (!context || !arc || context.heightDelta === null) throw new Error('Najpierw oblicz tor do nieruchomego celu (wymagana różnica wysokości)');
      if (count !== null && (!Number.isInteger(count) || count < 1 || count > 1000)) throw new Error('Liczba pocisków: liczba całkowita 1–1000');
      if (area !== null && effect === null) throw new Error('Ocena pokrycia obszaru wymaga promienia efektu');
      setBusy(true); setError('');
      const next = await call<SalvoPattern>('salvo_pattern', { profile: p, input: {
        distance: context.distance, heightDelta: context.heightDelta, azimuthDeg: context.azimuthDeg,
        elevationDeg: arc.elevationDeg, flightTime: arc.flightTime, unit: p.unit ?? 'game_units',
        salvoSize: count, effectRadius: effect, areaRadius: area,
      } });
      if (request !== version.current) return;
      setResult({ scope, pattern: next });
    } catch (caught) { if (request === version.current) fail(caught); }
    finally { if (request === version.current) setBusy(false); }
  };

  return <section className="panel salvo-panel">
    <h2>Salwa i rozrzut</h2>
    <p className="hint">Pojedynczy pocisk i cała salwa są liczone osobno. Prognoza wymaga jawnego rozrzutu startowego (kątowego lub prędkości) albo σ trafień z kalibracji. Bez tych danych nie pokazujemy elipsy.</p>
    {error && <p className="notice" role="alert">{error}</p>}
    <div className="fit-result">
      {salvo ? <>
        <p>Rozrzut startowy: kątowy {salvo.angularSigmaMrad === null ? 'brak danych' : `${salvo.angularSigmaMrad} mrad (1σ)`} · prędkości {salvo.speedSigma === null ? 'brak danych' : `${salvo.speedSigma} ${unitLabel}/s (1σ)`} · odstęp startów {salvo.rippleInterval === null ? 'brak danych' : `${salvo.rippleInterval} s`} <span className={`status-badge ${salvo.status}`}>{statusLabel(salvo.status)}</span></p>
        <p className="hint">Źródło: {salvo.source}</p>
      </> : p.dispersionSigma !== null && p.dispersionSigma !== undefined
        ? <p>Rozrzut trafień z kalibracji: σ = {p.dispersionSigma} {unitLabel} (izotropowy).</p>
        : <p>Brak danych rozrzutu dla tego pocisku.</p>}
      <p>Liczba pocisków w profilu: {p.salvoSize}{p.sourceMetadata && p.salvoSize === 1 ? ' — snapshot społeczności nie podaje pojemności wyrzutni' : ''}.</p>
    </div>
    <form onSubmit={apply}>
      <fieldset disabled={busy}>
        <legend>Parametry rozrzutu z pomiarów</legend>
        <div className="form-grid">
          <NumberInput id={`${id}-angular`} label="Rozrzut kątowy 1σ (mrad)" value={draft.angularSigmaMrad} min={0} max={500} onChange={angularSigmaMrad => setDraft(d => ({ ...d, angularSigmaMrad, confirmed: false }))} />
          <NumberInput id={`${id}-speed`} label={`Rozrzut prędkości 1σ (${unitLabel}/s)`} value={draft.speedSigma} min={0} onChange={speedSigma => setDraft(d => ({ ...d, speedSigma, confirmed: false }))} />
          <NumberInput id={`${id}-ripple`} label="Odstęp startów w salwie (s)" value={draft.rippleInterval} min={0} max={600} onChange={rippleInterval => setDraft(d => ({ ...d, rippleInterval, confirmed: false }))} />
          <div className="field wide"><label htmlFor={`${id}-source`}>Źródło pomiaru rozrzutu — wymagane</label><input id={`${id}-source`} value={draft.source} required maxLength={2048} onChange={event => setDraft(d => ({ ...d, source: event.target.value, confirmed: false }))} /></div>
        </div>
        <label className="check-field" htmlFor={`${id}-confirm`}><input id={`${id}-confirm`} type="checkbox" checked={draft.confirmed} required onChange={event => setDraft(d => ({ ...d, confirmed: event.target.checked }))} />Parametry pochodzą z moich pomiarów lub wskazanego źródła — wymagane</label>
        <div className="button-group"><button className="primary" type="submit">Zapisz rozrzut salwy</button>{p.salvo && <button type="button" onClick={() => onApplySalvo(null)}>Usuń rozrzut salwy</button>}</div>
      </fieldset>
    </form>
    <form onSubmit={event => { void forecast(event); }}>
      <fieldset disabled={busy || !context?.arcs.length}>
        <legend>Prognoza salwy do nieruchomego celu</legend>
        {!context?.arcs.length && <p className="hint">Brak rozwiązania toru — oblicz najpierw elewację w panelu trajektorii.</p>}
        <div className="form-grid">
          <div className="field"><label htmlFor={`${id}-arc`}>Tor</label><select id={`${id}-arc`} value={arcIndex} onChange={event => setArcIndex(Number(event.target.value))}>{(context?.arcs ?? []).map((arc, index) => <option key={index} value={index}>{arc.highArc ? 'Wysoki' : 'Niski'} · {arc.elevationDeg.toFixed(2)}° · {arc.flightTime.toFixed(2)} s</option>)}</select></div>
          <NumberInput id={`${id}-count`} label="Zaobserwowana liczba pocisków (zastępuje profil)" value={count} min={1} max={1000} step="1" onChange={setCount} />
          <NumberInput id={`${id}-effect`} label={`Promień efektu jednego pocisku (${unitLabel})`} value={effect} min={0} onChange={setEffect} />
          <NumberInput id={`${id}-area`} label={`Promień obszaru do pokrycia (${unitLabel})`} value={area} min={0} onChange={setArea} />
        </div>
        <button type="submit">{busy ? 'Obliczanie salwy…' : 'Oblicz prognozę salwy'}</button>
      </fieldset>
    </form>
    {pattern && <div className="fit-result" aria-live="polite" data-testid="salvo-pattern">
      <p>Status prognozy: <span className={`status-badge ${pattern.status}`}>{statusLabel(pattern.status)}</span> · pocisków: {pattern.salvoSize} · źródło rozrzutu: {pattern.dispersionSource === 'launch_dispersion' ? 'rozrzut startowy' : 'σ trafień'}</p>
      <SalvoPreview pattern={pattern} unitLabel={unitLabel} />
      {pattern.coverage && <p>Prawdopodobieństwo, że co najmniej jeden pocisk spadnie w promieniu {fmt(pattern.coverage.effectRadius)} {unitLabel} od celu: {(100 * pattern.coverage.probabilityAnyWithinEffect).toFixed(1)}% · oczekiwana liczba: {fmt(pattern.coverage.expectedProjectilesWithinEffect, 2)}{pattern.coverage.expectedAreaFraction !== null ? ` · oczekiwane pokrycie obszaru: ${(100 * pattern.coverage.expectedAreaFraction).toFixed(1)}%` : ''}. To model geometryczny, nie model obrażeń.</p>}
      {pattern.timing && <p>Czas trafień: od {fmt(pattern.timing.firstImpact, 2)} s do {fmt(pattern.timing.lastImpact, 2)} s po pierwszym starcie.</p>}
      {pattern.missingComponents.length > 0 && <ul className="assumptions">{pattern.missingComponents.map(m => <li key={m}>{m}</li>)}</ul>}
      <ul className="assumptions">{pattern.assumptions.map((a, index) => <li key={index}>{a}</li>)}</ul>
    </div>}
  </section>;
}

type Vector = [number, number, number];
/** Suggested inputs from the map; `null` components stay empty until entered. `note` explains missing ones. */
export type InterceptDefaults = { relative?: Vector | null; targetVelocity?: Vector | null; ownVelocity?: Vector | null; maxTime?: number | null; note?: string | null };

/** Beta moving-target intercept for drag/rocket models (`dynamic_intercept`). */
export function DynamicInterceptPanel({ profile, defaults, notice }: { profile: Profile | undefined; defaults?: InterceptDefaults; notice: (message: string) => void }) {
  const id = useId();
  const [relative, setRelative] = useState<(number | null)[]>(defaults?.relative ?? [null, null, null]);
  const [target, setTarget] = useState<(number | null)[]>(defaults?.targetVelocity ?? [null, null, null]);
  // Unknown own motion is never assumed to be zero; it must be entered explicitly.
  const [own, setOwn] = useState<(number | null)[]>(defaults?.ownVelocity ?? [null, null, null]);
  const [inherit, setInherit] = useState<'' | 'yes' | 'no'>('');
  const [maxTime, setMaxTime] = useState<number | null>(defaults?.maxTime ?? null);
  const [result, setResult] = useState<{ scope: string; solution: DynamicInterceptSolution } | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const version = useRef(0);
  const scope = JSON.stringify([profile, relative, target, own, inherit, maxTime]);
  const solution = result?.scope === scope ? result.solution : null;
  useEffect(() => { version.current++; setResult(null); setError(''); setBusy(false); }, [profile?.id, profile?.dynamics, profile?.unit]);
  useEffect(() => () => { version.current++; }, []);
  if (!profile || profile.category === 'guided_missile') return <section className="panel"><h2>Przechwycenie z oporem i napędem</h2><p className="hint">{profile ? 'Mechanika naprowadzania jest nieznana — przechwycenie zablokowane.' : 'Wybierz profil pocisku.'}</p></section>;
  const unitLabel = (profile.unit ?? 'game_units') === 'game_meters' ? 'm gry' : 'jedn. gry';
  const vectorFields = (label: string, value: (number | null)[], set: (v: (number | null)[]) => void, suffix: string) =>
    (['wschód', 'północ', 'góra'] as const).map((axis, index) => <NumberInput key={`${label}-${axis}`} id={`${id}-${label}-${index}`} label={`${label}: ${axis} (${suffix})`} required value={value[index]} onChange={v => { const next = [...value]; next[index] = v; set(next); }} />);
  /** Copies the current map/tracker values; unknown components become empty, not 0. */
  const useCurrent = () => {
    setRelative(defaults?.relative ?? [null, null, null]);
    setTarget(defaults?.targetVelocity ?? [null, null, null]);
    setOwn(defaults?.ownVelocity ?? [null, null, null]);
    if (defaults?.maxTime !== undefined) setMaxTime(defaults.maxTime ?? null);
    if (defaults?.note) notice(defaults.note);
  };
  const run = async (event: FormEvent) => {
    event.preventDefault();
    const request = ++version.current;
    try {
      const all = [...relative, ...target, ...own, maxTime];
      if (all.some(v => v === null || !Number.isFinite(v))) throw new Error('Wymagane pełne 3D położenia celu, prędkości celu i stanowiska oraz horyzont');
      const ownMoving = own.some(v => Math.abs(v ?? 0) > 1e-9);
      if (ownMoving && inherit === '') throw new Error('Wybierz jawnie, czy pocisk dziedziczy prędkość stanowiska');
      setBusy(true); setError('');
      const next = await call<DynamicInterceptSolution>('dynamic_intercept', { profile, input: {
        relative, targetVelocity: target, ownVelocity: own,
        inheritOwnVelocity: inherit === '' ? null : inherit === 'yes', unit: profile.unit ?? 'game_units', maxTime,
      } });
      if (request !== version.current) return;
      setResult({ scope, solution: next });
    } catch (caught) {
      if (request !== version.current) return;
      const message = caught instanceof Error ? caught.message : String(caught);
      setError(message); notice(message);
    } finally { if (request === version.current) setBusy(false); }
  };
  return <section className="panel">
    <h2>Przechwycenie z oporem i napędem <span className="status-badge beta">beta</span></h2>
    <p className="hint">Iteracja Newtona na elewacji, azymucie i czasie lotu z trajektorią RK4 modelu profilu. Cel o stałej prędkości 3D; brak zbieżności oznacza brak wyniku, bez zastępowania modelem vacuum.</p>
    {error && <p className="notice" role="alert">{error}</p>}
    <form onSubmit={event => { void run(event); }}>
      <fieldset disabled={busy}>
        <legend>Ruch celu i stanowiska ({unitLabel})</legend>
        <div className="form-grid">
          {vectorFields('Cel względem wylotu', relative, setRelative, unitLabel)}
          {vectorFields('Prędkość celu', target, setTarget, `${unitLabel}/s`)}
          {vectorFields('Prędkość stanowiska', own, setOwn, `${unitLabel}/s`)}
          <NumberInput id={`${id}-horizon`} label="Horyzont ruchu i lotu (s)" required value={maxTime} min={0.000001} max={600} onChange={setMaxTime} />
          <div className="field"><label htmlFor={`${id}-inherit`}>Dziedziczenie prędkości stanowiska</label><select id={`${id}-inherit`} value={inherit} onChange={event => setInherit(event.target.value as typeof inherit)}><option value="">Nie wybrano</option><option value="yes">Tak (założenie do walidacji)</option><option value="no">Nie</option></select></div>
        </div>
        {defaults?.note && <p className="hint">{defaults.note}</p>}
        <div className="button-group">
          <button type="button" onClick={useCurrent} disabled={!defaults}>Użyj bieżących pozycji</button>
          <button type="submit">{busy ? 'Iteracja…' : 'Oblicz przechwycenie'}</button>
        </div>
      </fieldset>
    </form>
    {solution && <div className="fit-result" aria-live="polite" data-testid="dynamic-intercept">
      <p>Status: <span className={`status-badge ${solution.status}`}>{statusLabel(solution.status)}</span> · kroki RK4: {solution.stepsUsed}</p>
      <div className="arc-list">{solution.arcs.map(arc => <div key={`${arc.highArc}-${arc.elevationDeg}`}><b>{arc.highArc ? 'Wysoki tor' : 'Niski tor'}</b><strong>{arc.elevationDeg.toFixed(2)}° / az. {arc.azimuthDeg.toFixed(2)}°</strong><span>{arc.flightTime.toFixed(2)} s · błąd {arc.missDistance.toFixed(3)} {unitLabel} · {arc.iterations} iter.</span></div>)}</div>
      <ul className="convergence-list">{solution.branches.map((b, index) => <li key={index}>Start {b.seedElevationDeg.toFixed(2)}°: {b.converged ? 'zbieżny' : 'brak zbieżności'} po {b.iterations} iter.{b.finalMiss !== null ? `, błąd ${b.finalMiss.toFixed(3)} ${unitLabel}` : ''} — {b.message}</li>)}</ul>
      <ul className="assumptions">{solution.assumptions.map((a, index) => <li key={index}>{a}</li>)}</ul>
    </div>}
  </section>;
}
