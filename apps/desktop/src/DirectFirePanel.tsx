import { useEffect, useId, useState, type FormEvent } from 'react';
import { call, profileFor } from './api';
import type { Profile } from './types';
import type { LoadoutSpecs } from './loadout-types';
import type { QuickAimState } from './useQuickAim';
import {
  CALCULATOR_GRAVITY, DIRECT_CATEGORIES, PGO7_ASSUMED, createDirectFireProfile, formatReticle, kmhToUnitsPerSecond, parseRanges, parseReticle,
  type AirToGroundImpact, type ControlShot, type ControlShotFit, type LeadEstimate, type RangeCard,
} from './direct-fire';
import './ballistics.css';

/** Compact data for the overlay card (≤ 8 rows, fits 340 px). */
export type OverlayDirectCard = { profile: string; status: string; zeroRange: number | null; rows: { range: number; holdMrad: number | null; drop: number | null; tof: number | null }[]; lead: string | null; heli: string | null };

type Props = {
  profiles: Profile[]; overrides: Record<string, Profile>; quick: QuickAimState;
  windowHandle: number | null; backend: 'wgc' | 'gdi';
  onSaveProfile: (profile: Profile) => void; log: (event: string) => void; notice: (message: string) => void;
  onCard: (card: OverlayDirectCard | null) => void;
};

function Num({ label, value, onChange, min, max, step = 'any' }: { label: string; value: number | null; onChange: (v: number | null) => void; min?: number; max?: number; step?: string }) {
  const id = useId();
  return <div className="field"><label htmlFor={id}>{label}</label><input id={id} type="number" inputMode="decimal" step={step} min={min} max={max} value={value ?? ''} onChange={e => onChange(e.target.value === '' ? null : e.target.valueAsNumber)} /></div>;
}
const f = (v: number | null | undefined, d = 1) => v === null || v === undefined || !Number.isFinite(v) ? '—' : v.toFixed(d).replace('.', ',');
const message = (e: unknown) => e instanceof Error ? e.message : String(e);
const statusLabel = (p: Profile) => p.calibrated ? 'beta (lokalna kalibracja)' : p.observedSpecs ? 'eksperymentalny (odczyt UI / wpis)' : p.sourceMetadata ? 'eksperymentalny (społeczność)' : 'brak danych';

export function DirectFirePanel({ profiles, overrides, quick, windowHandle, backend, onSaveProfile, log, notice, onCard }: Props) {
  const all = [...profiles.map(p => overrides[p.id] ?? p), ...Object.values(overrides).filter(o => !profiles.some(p => p.id === o.id))];
  const candidates = all.filter(p => DIRECT_CATEGORIES.has(p.category)).sort((a, b) => a.name.localeCompare(b.name, 'pl'));
  const { config, setConfig } = quick;
  const selected = config.profileId ? profileFor(profiles, config.profileId, overrides) : undefined;
  const [error, setError] = useState('');
  const fail = (e: unknown) => { setError(message(e)); notice(message(e)); };
  // Creation form
  const [name, setName] = useState(''); const [velocity, setVelocity] = useState<number | null>(null); const [gravity, setGravity] = useState<number | null>(CALCULATOR_GRAVITY);
  const [screen, setScreen] = useState<'loadout' | 'manual'>('manual'); const [observedAt, setObservedAt] = useState(''); const [ack, setAck] = useState(false);
  const [reticleText, setReticleText] = useState('');
  const [selectedReticle, setSelectedReticle] = useState('');
  const [readBusy, setReadBusy] = useState(false);
  // Range card, lead, control shots, helicopter
  const [rangesText, setRangesText] = useState('50-800/50');
  const [card, setCard] = useState<{ key: string; value: RangeCard } | null>(null);
  const [targetKmh, setTargetKmh] = useState<number | null>(null); const [course, setCourse] = useState<number | null>(90); const [leadRange, setLeadRange] = useState<number | null>(null);
  const [lead, setLead] = useState<LeadEstimate | null>(null);
  const [shotRange, setShotRange] = useState<number | null>(null); const [shotElevation, setShotElevation] = useState<number | null>(null);
  const [shotMiss, setShotMiss] = useState<number | null>(null); const [shotDirection, setShotDirection] = useState<'above' | 'below'>('below');
  const [shots, setShots] = useState<ControlShot[]>([]); const [fit, setFit] = useState<ControlShotFit | null>(null);
  const rocketProfiles = all.filter(p => p.category === 'helicopter_rocket' || (p.category === 'direct_fire' && p.dynamics?.model === 'rocket'));
  const [heliId, setHeliId] = useState(() => rocketProfiles.find(p => p.id === 'community-ub32-1')?.id ?? rocketProfiles[0]?.id ?? '');
  const heliProfile = profileFor(profiles, heliId, overrides);
  const [altitude, setAltitude] = useState<number | null>(null); const [airKmh, setAirKmh] = useState<number | null>(null);
  const [dive, setDive] = useState<number | null>(null); const [heliDistance, setHeliDistance] = useState<number | null>(null);
  const [impact, setImpact] = useState<AirToGroundImpact | null>(null); const [solved, setSolved] = useState<AirToGroundImpact | null>(null);
  const [busy, setBusy] = useState(false);
  const cardKey = JSON.stringify([selected, rangesText, config.sightHeight, config.zeroRange]);
  const activeCard = card?.key === cardKey ? card.value : null;
  useEffect(() => { setShots([]); setFit(null); setLead(null); setSelectedReticle(selected?.reticle ? formatReticle(selected.reticle.marks) : ''); }, [selected?.id, selected?.speed, selected?.reticle]);
  useEffect(() => {
    onCard(activeCard && selected ? {
      profile: selected.name, status: activeCard.status, zeroRange: activeCard.zeroRange,
      rows: activeCard.rows.filter((_, i, rows) => rows.length <= 8 || i % Math.ceil(rows.length / 8) === 0).slice(0, 8).map(r => ({ range: r.range, holdMrad: r.holdOverMrad, drop: r.dropBelowLineOfSight, tof: r.timeOfFlight })),
      lead: lead ? `wyprz. ${lead.leadMrad >= 0 ? 'P' : 'L'} ${f(Math.abs(lead.leadMrad))} mrad / ${f(Math.abs(lead.lateralOffset))} m` : null,
      heli: impact ? `nurk. ${f(impact.diveAngleDeg, 0)}° → ${f(impact.horizontalDistance, 0)} m, obniż ${f(impact.depressionBelowBoresightMrad)} mrad` : null,
    } : null);
  }, [activeCard, selected?.name, lead, impact]);
  useEffect(() => () => onCard(null), []);

  const run = async (task: () => Promise<void>) => { setBusy(true); setError(''); try { await task(); } catch (e) { fail(e); } finally { setBusy(false); } };
  const readLoadout = async () => {
    setReadBusy(true);
    try {
      if (windowHandle === null) throw new Error('Wybierz okno gry w zakładce „Analiza obrazu” lub „Celowanie”, otwórz ekran Loadout');
      const specs = await call<LoadoutSpecs>('read_loadout_specs', { handle: windowHandle, backend });
      if (!specs.muzzleVelocityMps) throw new Error(`Nie odczytano „Muzzle Velocity”${specs.warnings.length ? `: ${specs.warnings.join('; ')}` : ''}`);
      setVelocity(specs.muzzleVelocityMps.value); setScreen('loadout'); setObservedAt(new Date(specs.timestampMs).toISOString());
      notice(`Odczytano z UI gry: ${specs.muzzleVelocityMps.text} (pewność heurystyczna ${Math.round(specs.confidence * 100)}%). Sprawdź wartość i nazwę broni.`);
    } catch (e) {
      const text = message(e);
      fail(/not found|unknown command|nie znaleziono/i.test(text) ? 'Odczyt Loadout jeszcze niedostępny w tej wersji — wpisz prędkość ręcznie' : `Odczyt Loadout nieudany: ${text}`);
    } finally { setReadBusy(false); }
  };
  const create = (event: FormEvent) => {
    event.preventDefault();
    try {
      const reticle = reticleText.trim() ? parseReticle(reticleText) : null;
      const profile = createDirectFireProfile({ name, velocity, gravity, screen, observedAt: screen === 'loadout' && observedAt ? observedAt : new Date().toISOString(), weaponName: name, acknowledged: ack, reticle });
      onSaveProfile(profile); setConfig(c => ({ ...c, profileId: profile.id }));
      log(`Utworzono profil ognia bezpośredniego ${profile.name}: ${profile.source}`);
      notice('Profil utworzony (eksperymentalny). Wykonaj strzały kontrolne przed poleganiem na wynikach.');
      setName(''); setVelocity(null); setScreen('manual'); setObservedAt(''); setAck(false); setReticleText('');
    } catch (e) { fail(e); }
  };
  const saveReticle = () => {
    if (!selected) return;
    try {
      const marks = selectedReticle.trim() ? parseReticle(selectedReticle) : [];
      const updated: Profile = { ...selected, reticle: marks.length ? { ...(selected.reticle ?? PGO7_ASSUMED), marks, status: selected.reticle?.status ?? 'unverified' } : null };
      onSaveProfile(updated); log(`Mapa kresek ${selected.name}: ${marks.length ? formatReticle(marks) : 'usunięta'}`);
    } catch (e) { fail(e); }
  };
  const computeCard = () => run(async () => {
    if (!selected) throw new Error('Wybierz profil');
    const value = await call<RangeCard>('range_card', { profile: selected, input: { unit: selected.unit ?? 'game_units', ranges: parseRanges(rangesText), sightHeight: config.sightHeight, zeroRange: config.zeroRange } });
    setCard({ key: cardKey, value });
  });
  const computeLead = () => run(async () => {
    if (!selected || targetKmh === null || course === null || leadRange === null) throw new Error('Podaj profil, dystans, prędkość celu i kurs');
    setLead(await call<LeadEstimate>('lead_iterated', { profile: selected, input: { range: leadRange, targetSpeed: kmhToUnitsPerSecond(targetKmh), courseRelativeDeg: course, unit: selected.unit ?? 'game_units', sightHeight: config.sightHeight } }));
  });
  const addShot = () => {
    if (shotRange === null || shotElevation === null || shotMiss === null || shotRange <= 0) { fail('Strzał kontrolny: podaj dystans, użytą elewację i odchyłkę'); return; }
    const shot = { range: shotRange, elevationMrad: shotElevation, missVertical: shotDirection === 'above' ? Math.abs(shotMiss) : -Math.abs(shotMiss) };
    setShots(old => [...old, shot]); setFit(null);
    log(`Strzał kontrolny ${selected?.name ?? ''}: ${shot.range} m, elewacja ${shot.elevationMrad} mrad, ${shot.missVertical >= 0 ? 'wyżej' : 'niżej'} o ${Math.abs(shot.missVertical)} m`);
  };
  const checkShots = () => run(async () => {
    if (!selected) throw new Error('Wybierz profil');
    const result = await call<ControlShotFit>('fit_control_shots', { profile: selected, input: { unit: selected.unit ?? 'game_units', sightHeight: config.sightHeight, shots, maxResidual: 0.5 } });
    setFit(result); log(`Strzały kontrolne ${selected.name}: ${result.message}`);
  });
  const applyFit = () => {
    if (!selected || !fit?.fits || fit.fittedSpeed === null) return;
    const date = new Date().toISOString().slice(0, 10);
    onSaveProfile({ ...selected, speed: fit.fittedSpeed, calibrated: true, status: 'beta', source: `${selected.source}; strzały kontrolne ${date}: efektywna prędkość ${fit.fittedSpeed.toFixed(1)} (${shots.length} strzałów, model bez oporu, grawitacja bez zmian)` });
    log(`Lokalna kalibracja beta ${selected.name} ze strzałów kontrolnych: v=${fit.fittedSpeed.toFixed(2)}`); setFit(null); setShots([]);
  };
  const heliForward = () => run(async () => {
    if (!heliProfile || altitude === null || airKmh === null || dive === null) throw new Error('Podaj profil rakiety, wysokość, prędkość lotu i kąt nurkowania');
    setImpact(await call<AirToGroundImpact>('air_to_ground_impact', { profile: heliProfile, input: { altitudeAboveTarget: altitude, airspeed: kmhToUnitsPerSecond(airKmh), diveAngleDeg: dive, unit: heliProfile.unit ?? 'game_units' } }));
  });
  const heliInverse = () => run(async () => {
    if (!heliProfile || altitude === null || airKmh === null || heliDistance === null) throw new Error('Podaj profil rakiety, wysokość, prędkość lotu i dystans do celu');
    setSolved(await call<AirToGroundImpact>('air_to_ground_solve', { profile: heliProfile, input: { altitudeAboveTarget: altitude, airspeed: kmhToUnitsPerSecond(airKmh), horizontalDistance: heliDistance, unit: heliProfile.unit ?? 'game_units' } }));
  });
  const ackCommunity = (p: Profile) => { onSaveProfile({ ...p, sourceAcknowledged: true }); log(`Potwierdzono eksperymentalne dane ${p.name}`); };

  return <>
    <section className="panel">
      <h2>Broń bezpośrednia / śmigłowiec</h2>
      <p className="hint">Tylko liczby i tabele dla gracza. Aplikacja nie wykrywa ani nie śledzi przeciwników i nie rysuje znaczników wyprzedzenia na celach. Dystans podajesz sam (preset, dalmierz gry, dystans pokazany w grze).</p>
      {error && <p className="notice" role="alert">{error}</p>}
      <div className="form-grid">
        <div className="field wide"><label htmlFor="df-profile">Profil broni</label>
          <select id="df-profile" value={config.profileId ?? ''} onChange={e => setConfig(c => ({ ...c, profileId: e.target.value || null }))}>
            <option value="">— wybierz —</option>{candidates.map(p => <option key={p.id} value={p.id}>{p.name}{p.speed ? ` · ${p.speed} m/s` : ' · brak prędkości'}</option>)}
          </select></div>
      </div>
      {selected && <div className="fit-result">
        <p>Status: <b>{statusLabel(selected)}</b> · prędkość {selected.speed ?? 'brak danych'} · grawitacja {selected.gravity ?? 'brak danych'} · model {selected.dynamics?.model ?? 'vacuum'}</p>
        <p className="hint">Źródło: {selected.source}</p>
        {selected.observedSpecs && <ul className="assumptions"><li>{selected.observedSpecs.velocityUnitAssumption}</li><li>{selected.observedSpecs.gravitySource}</li></ul>}
        {selected.sourceMetadata && !selected.sourceAcknowledged && <button type="button" onClick={() => ackCommunity(selected)}>Używaj eksperymentalnych danych społeczności dla tego pocisku</button>}
      </div>}
    </section>

    <section className="panel">
      <h2>Tryb szybki — czołg / broń ręczna</h2>
      <p className="hint">Bez otwierania mapy: Numpad + / − zmienia zakładany dystans po liście presetów, Numpad 0 = bliski zasięg (celuj w środek). Skróty są rejestrowane tylko, gdy tryb jest włączony. Profil: z panelu broni w grze (gdy jednoznaczny) albo wybrany powyżej. Odczyt RANGE z panelu gry zastępuje preset.</p>
      <label className="check-field"><input type="checkbox" checked={config.enabled} onChange={e => setConfig(c => ({ ...c, enabled: e.target.checked }))} />Włącz tryb szybki i skróty Numpad</label>
      {config.enabled && quick.hotkeys && <p className={quick.hotkeys.errors.length ? 'notice' : 'hint'}>{quick.hotkeys.errors.length ? `Nie zarejestrowano: ${quick.hotkeys.errors.join('; ')}` : `Zarejestrowano: ${quick.hotkeys.registered.join(', ')}`}</p>}
      <div className="form-grid">
        <div className="field"><label htmlFor="df-presets">Presety dystansu (m)</label><input id="df-presets" defaultValue={config.presets.join(', ')} onBlur={e => { try { setConfig(c => ({ ...c, presets: parseRanges(e.target.value), index: -1 })); } catch (err) { fail(err); } }} /></div>
        <Num label="Próg „celuj w środek” — |opad| poniżej (m)" value={config.threshold} min={0.01} onChange={v => v !== null && v > 0 && setConfig(c => ({ ...c, threshold: v }))} />
        <Num label="Wysokość celownika nad osią lufy (m)" value={config.sightHeight} onChange={v => setConfig(c => ({ ...c, sightHeight: v ?? 0 }))} />
        <Num label="Dystans zerowania (m, puste = brak)" value={config.zeroRange} min={1} onChange={v => setConfig(c => ({ ...c, zeroRange: v }))} />
      </div>
      <div className="button-group">
        <button type="button" onClick={() => setConfig(c => ({ ...c, index: Math.max(-1, c.index - 1) }))}>− dystans</button>
        <button type="button" onClick={() => setConfig(c => ({ ...c, index: -1 }))}>bliski</button>
        <button type="button" onClick={() => setConfig(c => ({ ...c, index: Math.min(c.presets.length - 1, c.index + 1) }))}>+ dystans</button>
        <label className="check-field"><input type="checkbox" checked={config.showCard} onChange={e => setConfig(c => ({ ...c, showCard: e.target.checked }))} />Pokaż kartę nastaw na nakładce</label>
      </div>
      {config.enabled && <p className="quick-line" role="status" aria-live="polite">{quick.line}</p>}
      {config.enabled && <p className="hint">Profil: {quick.profile?.name ?? 'brak'}{quick.profileSource === 'panel' ? ' (z panelu broni w grze)' : quick.profileSource === 'selected' ? ' (wybrany ręcznie)' : ''}{quick.quick?.warnings.length ? ` · ${quick.quick.warnings[0]}` : ''}</p>}
    </section>

    <section className="panel">
      <h2>Nowy profil ognia bezpośredniego</h2>
      <form onSubmit={create}>
        <div className="form-grid">
          <div className="field wide"><label htmlFor="df-name">Broń i amunicja</label><input id="df-name" value={name} required maxLength={120} placeholder="np. RPG-7V2 / PG-7VR" onChange={e => setName(e.target.value)} /></div>
          <Num label="Prędkość wylotowa (m/s z UI gry)" value={velocity} min={0.1} onChange={v => { setVelocity(v); setScreen('manual'); }} />
          <Num label="Grawitacja (m gry/s²)" value={gravity} min={0.001} onChange={setGravity} />
          <div className="field wide"><label htmlFor="df-reticle">Kreski celownika (opcjonalnie): etykieta=dystans</label><input id="df-reticle" value={reticleText} placeholder="2=200, 3=300, 4=400, 5=500" onChange={e => setReticleText(e.target.value)} /></div>
        </div>
        <div className="button-group">
          <button type="button" disabled={readBusy} onClick={() => void readLoadout()}>{readBusy ? 'Odczyt…' : 'Odczytaj prędkość z ekranu Loadout'}</button>
          <button type="button" onClick={() => setReticleText(formatReticle(PGO7_ASSUMED.marks))}>Wstaw założenie PGO-7</button>
        </div>
        <p className="hint">{screen === 'loadout' ? `Wartość odczytana z UI gry ${observedAt.slice(0, 19).replace('T', ' ')}.` : 'Wartość wpisana ręcznie.'} Domyślna grawitacja to 17,64 m gry/s² z kalkulatora społeczności — nie wiadomo, czy dotyczy broni ręcznej; „m/s” z gry traktujemy jako metry gry/s. Profil będzie eksperymentalny.</p>
        <label className="check-field"><input type="checkbox" checked={ack} required onChange={e => setAck(e.target.checked)} />Rozumiem: jednostka i grawitacja to założenia do weryfikacji strzałami kontrolnymi</label>
        <button className="primary" type="submit">Utwórz profil</button>
      </form>
      {selected?.category === 'direct_fire' && <div className="button-group separated">
        <input aria-label="Kreski celownika wybranego profilu" value={selectedReticle} placeholder={selected.reticle ? formatReticle(selected.reticle.marks) : '2=200, 3=300'} onChange={e => setSelectedReticle(e.target.value)} />
        <button type="button" onClick={saveReticle}>Zapisz kreski dla „{selected.name}”</button>
        {selected.reticle && <span className="status-badge experimental">{selected.reticle.status}</span>}
      </div>}
    </section>

    <section className="panel">
      <h2>Karta nastaw</h2>
      <div className="form-grid"><div className="field"><label htmlFor="df-ranges">Dystanse (od-do/krok lub lista)</label><input id="df-ranges" value={rangesText} onChange={e => setRangesText(e.target.value)} /></div></div>
      <button type="button" disabled={busy || !selected} onClick={() => void computeCard()}>Oblicz kartę</button>
      {activeCard && <>
        <table className="range-card"><thead><tr><th>m</th><th>poprawka mrad</th><th>opad m</th><th>czas s</th><th>v m/s</th></tr></thead>
          <tbody>{activeCard.rows.map(r => <tr key={r.range} className={r.reachable ? '' : 'unreachable'}><td>{r.range}</td><td>{f(r.holdOverMrad)}</td><td>{f(r.dropBelowLineOfSight, 2)}</td><td>{f(r.timeOfFlight, 2)}</td><td>{f(r.velocityAtRange, 0)}</td></tr>)}</tbody></table>
        <p className="hint">Status {activeCard.status} · model {activeCard.model}. Poprawka dodatnia = celuj wyżej. 1 mrad ≈ 1 m na 1000 m.</p>
        <ul className="assumptions">{[...activeCard.warnings, ...activeCard.assumptions].map((a, i) => <li key={i}>{a}</li>)}</ul>
      </>}
    </section>

    <section className="panel">
      <h2>Wyprzedzenie (liczby, bez znaczników)</h2>
      <div className="form-grid">
        <Num label="Dystans do celu (m)" value={leadRange} min={1} onChange={setLeadRange} />
        <Num label="Prędkość celu (km/h)" value={targetKmh} min={0} onChange={setTargetKmh} />
        <Num label="Kurs względny (°): 0 oddala, 90 w prawo, 180 zbliża, 270 w lewo" value={course} onChange={setCourse} />
      </div>
      <button type="button" disabled={busy || !selected} onClick={() => void computeLead()}>Oblicz wyprzedzenie</button>
      {lead && <div className="fit-result"><p>Celuj {lead.leadMrad >= 0 ? 'w prawo' : 'w lewo'} o <b>{f(Math.abs(lead.leadMrad))} mrad</b> ({f(Math.abs(lead.lateralOffset))} m przy celu) · czas lotu {f(lead.timeOfFlight, 2)} s · przewidywany dystans {f(lead.futureRange, 0)} m{lead.elevationMrad !== null ? ` · elewacja ${f(lead.elevationMrad)} mrad` : ''}</p><p className="hint">km/h przeliczane na m gry/s (założenie). {lead.assumptions.join(' ')}</p></div>}
    </section>

    <section className="panel">
      <h2>Strzał kontrolny</h2>
      <p className="hint">Zapisz, gdzie upadł pocisk względem punktu celowania. Dla modelu bez oporu dopasowanie efektywnej prędkości (grawitacja bez zmian); w innym przypadku tylko log kalibracji.</p>
      <div className="form-grid">
        <Num label="Dystans (m)" value={shotRange} min={1} onChange={setShotRange} />
        <Num label="Użyta elewacja (mrad, z karty)" value={shotElevation} onChange={setShotElevation} />
        <Num label="Odchyłka (m)" value={shotMiss} min={0} onChange={setShotMiss} />
        <div className="field"><label htmlFor="df-miss">Kierunek</label><select id="df-miss" value={shotDirection} onChange={e => setShotDirection(e.target.value as 'above' | 'below')}><option value="below">niżej niż cel</option><option value="above">wyżej niż cel</option></select></div>
      </div>
      <div className="button-group">
        <button type="button" disabled={!selected} onClick={() => { const row = activeCard?.rows.find(r => r.range === shotRange); if (row?.elevationMrad != null) setShotElevation(Number(row.elevationMrad.toFixed(2))); }}>Elewacja z karty dla tego dystansu</button>
        <button type="button" disabled={!selected} onClick={addShot}>Dodaj strzał</button>
        <button type="button" disabled={busy || !shots.length} onClick={() => void checkShots()}>Sprawdź zgodność z modelem ({shots.length})</button>
      </div>
      {fit && <div className="fit-result"><p>{fit.message}</p>{fit.fittedSpeed !== null && <p>Efektywna prędkość: {f(fit.fittedSpeed, 2)} · maks. reszta {f(fit.maxResidual, 3)} m</p>}{fit.fits && <button className="primary" type="button" onClick={applyFit}>Zastosuj lokalną kalibrację beta</button>}</div>}
    </section>

    <section className="panel">
      <h2>Śmigłowiec — rakiety niekierowane</h2>
      <div className="form-grid">
        <div className="field wide"><label htmlFor="df-heli">Rakieta</label><select id="df-heli" value={heliId} onChange={e => setHeliId(e.target.value)}>{rocketProfiles.map(p => <option key={p.id} value={p.id}>{p.name} · {p.speed ?? '—'} m/s</option>)}</select></div>
        <Num label="Wysokość nad celem (m)" value={altitude} min={1} onChange={setAltitude} />
        <Num label="Prędkość lotu (km/h)" value={airKmh} min={0} onChange={setAirKmh} />
        <Num label="Kąt nurkowania (°)" value={dive} min={-30} max={89} onChange={setDive} />
        <Num label="Dystans poziomy do celu (m)" value={heliDistance} min={1} onChange={setHeliDistance} />
      </div>
      {heliProfile?.sourceMetadata && !heliProfile.sourceAcknowledged && <button type="button" onClick={() => ackCommunity(heliProfile)}>Używaj eksperymentalnych danych {heliProfile.name}</button>}
      <div className="button-group">
        <button type="button" disabled={busy} onClick={() => void heliForward()}>Gdzie trafią przy tym kącie</button>
        <button type="button" disabled={busy} onClick={() => void heliInverse()}>Jaki kąt dla tego dystansu</button>
      </div>
      {[impact && ['Przy zadanym kącie', impact], solved && ['Wymagany kąt', solved]].filter(Boolean).map(entry => { const [title, r] = entry as [string, AirToGroundImpact]; return <div className="fit-result" key={title}>
        <p><b>{title}:</b> nurkowanie {f(r.diveAngleDeg, 1)}° · trafienie {f(r.horizontalDistance, 0)} m poziomo ({f(r.slantRange, 0)} m skośnie) · czas {f(r.timeOfFlight, 2)} s</p>
        <p>Punkt trafienia {f(r.depressionBelowBoresightMrad)} mrad ({f(r.depressionBelowBoresightDeg, 2)}°) poniżej osi wyrzutni — obniż znak celowania o tyle i nałóż go na cel.{r.aimOffsetAlongGround !== null ? ` Względem przecięcia osi z ziemią: ${f(r.aimOffsetAlongGround, 0)} m.` : ''}</p>
        <ul className="assumptions">{[...r.warnings, ...r.assumptions].map((a, i) => <li key={i}>{a}</li>)}</ul></div>; })}
    </section>
  </>;
}
