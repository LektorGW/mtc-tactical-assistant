import { useEffect, useId, useRef, useState, type FormEvent } from 'react';
import { call } from './api';
import type { Profile, ProjectileDynamics, Unit } from './types';
import { prepareLocalDynamics, type DynamicsDraft } from './dynamics-validation';

type Props = {
  profile: Profile | undefined;
  onApply: (dynamics: ProjectileDynamics | null) => void;
  notice: (message: string) => void;
};
type Simulation = {
  points: { time: number; position: [number, number, number]; velocity: [number, number, number] }[];
  unit: Unit;
  assumptions: string[];
};
function draftFor(profile: Profile | undefined): DynamicsDraft {
  const d = profile?.dynamics;
  return {
    model: d?.model ?? 'vacuum', linearDrag: d?.linearDrag ?? null,
    quadraticDrag: d?.quadraticDrag ?? null, rocketAcceleration: d?.rocketAcceleration ?? null,
    burnTime: d?.burnTime ?? null, thrustDirectionFixedLaunch: d?.thrustDirectionFixedLaunch === true,
    source: d?.source ?? '', localCalibrationConfirmed: false,
  };
}
function NumberControl({ id, name, label, value, onChange, min, max }: {
  id: string; name: string; label: string; value: number | null;
  onChange: (value: number | null) => void; min?: number; max?: number;
}) {
  return <div className="field">
    <label htmlFor={id}>{label} — wymagane</label>
    <input id={id} name={name} type="number" inputMode="decimal" step="any" required
      min={min} max={max} value={value ?? ''} onChange={event => onChange(event.target.value === '' ? null : event.target.valueAsNumber)} />
  </div>;
}

export function DynamicsPanel({ profile, onApply, notice }: Props) {
  const id = useId();
  const [draft, setDraft] = useState<DynamicsDraft>(() => draftFor(profile));
  const [elevation, setElevation] = useState<number | null>(null);
  const [azimuth, setAzimuth] = useState<number | null>(null);
  const [duration, setDuration] = useState<number | null>(null);
  const [preview, setPreview] = useState<{ scope: string; simulation: Simulation } | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const errorSummary = useRef<HTMLParagraphElement>(null);
  const requestVersion = useRef(0);
  const currentScope = JSON.stringify([profile?.id, profile?.category, profile?.unit, profile?.gameVersion, profile?.status, profile?.source, profile?.calibrated, profile?.speed, profile?.gravity, profile?.dynamics, draft, elevation, azimuth, duration]);
  // Render-time scope validation prevents even a one-render display of a result
  // belonging to a previous profile, unit, coefficients or preview request.
  const activePreview = preview?.scope === currentScope ? preview.simulation : null;
  useEffect(() => {
    requestVersion.current++;
    setDraft(draftFor(profile));
    setElevation(null); setAzimuth(null); setDuration(null);
    setPreview(null); setError(''); setBusy(false);
  }, [profile?.id, profile?.unit, profile?.dynamics, profile?.calibrated, profile?.status, profile?.speed, profile?.gravity, profile?.source]);
  useEffect(() => () => { requestVersion.current++; }, []);
  const clearFeedback = () => { requestVersion.current++; setError(''); setPreview(null); };
  const edit = (patch: Partial<DynamicsDraft>) => {
    clearFeedback();
    setDraft(previous => ({ ...previous, ...patch, localCalibrationConfirmed: patch.localCalibrationConfirmed ?? false }));
  };
  const fail = (caught: unknown) => {
    const message = caught instanceof Error ? caught.message : String(caught);
    setError(message); notice(message);
    requestAnimationFrame(() => errorSummary.current?.focus());
  };
  const apply = (event: FormEvent) => {
    event.preventDefault();
    try {
      const dynamics = prepareLocalDynamics(profile, draft);
      onApply(dynamics);
      setError(''); setPreview(null);
      notice(dynamics ? 'Zastosowano lokalny model beta. Porównaj wynik z niezależnymi strzałami w grze.' : 'Włączono model bez oporu i napędu.');
    } catch (caught) { fail(caught); }
  };
  const simulate = async (event: FormEvent) => {
    event.preventDefault();
    const version = requestVersion.current;
    try {
      if (!profile) throw new Error('Wybierz profil pocisku');
      const dynamics = prepareLocalDynamics(profile, draft);
      if (elevation === null || !Number.isFinite(elevation) || elevation < profile.minElevationDeg || elevation > profile.maxElevationDeg
        || azimuth === null || !Number.isFinite(azimuth) || azimuth < 0 || azimuth > 360
        || duration === null || !Number.isFinite(duration) || duration < 0.000001 || duration > 600) {
        throw new Error('Podaj elewację w limitach profilu, azymut od 0° do 360° i czas od 0.000001 do 600 s');
      }
      setBusy(true); setError(''); setPreview(null);
      const result = await call<Simulation>('projectile_simulation', {
        profile: { ...profile, dynamics },
        input: { elevationDeg: elevation, azimuthDeg: azimuth, unit: profile.unit ?? 'game_units', duration },
      });
      if (version !== requestVersion.current) return;
      const end = result.points?.at(-1);
      if (!end || result.unit !== (profile.unit ?? 'game_units') || !Number.isFinite(end.time)
        || end.position.length !== 3 || end.position.some(value => !Number.isFinite(value))) {
        throw new Error('Symulacja nie zwróciła kompletnego położenia w jednostkach profilu');
      }
      setPreview({ scope: currentScope, simulation: result });
    } catch (caught) { if (version === requestVersion.current) fail(caught); }
    finally { if (version === requestVersion.current) setBusy(false); }
  };

  if (!profile) return <section className="panel"><h2>Opór i napęd pocisku</h2><p className="hint">Wybierz profil pocisku.</p></section>;
  if (profile.category === 'guided_missile') return <section className="panel"><h2>Opór i napęd pocisku</h2><p className="hint">Mechanika naprowadzania jest nieznana. Ten profil nie ma dostępnej predykcji lotu.</p></section>;
  const unit = profile.unit ?? 'game_units';
  const unitLabel = unit === 'game_meters' ? 'm gry' : 'jedn. gry';
  const eligible = profile.calibrated && profile.status === 'beta';
  const last = activePreview?.points.at(-1);
  const first = activePreview?.points[0];
  const coordinate = (position: [number, number, number]) => `wschód ${position[0].toFixed(3)}, północ ${position[1].toFixed(3)}, góra ${position[2].toFixed(3)} ${unitLabel}`;
  return <section className="panel dynamics-panel">
    <h2>Opór i napęd pocisku</h2>
    <p className="hint">Jednostka wybranego profilu: {unitLabel} ({unit}). Współczynniki pochodzą z lokalnych pomiarów w grze. Puste pole oznacza brak danych; wpisz 0 wyłącznie dla potwierdzonego zera.</p>
    <p className="hint">Model beta wymaga porównania z obserwowanymi strzałami. Dla oporu lub napędu dostępny jest tor do nieruchomego celu; wyprzedzenie ruchomego celu wymaga odrębnego modelu.</p>
    {error && <p className="notice" ref={errorSummary} tabIndex={-1} role="alert">{error}</p>}
    <form onSubmit={apply}>
      <fieldset disabled={busy}>
        <legend>Model lotu</legend>
        {([['vacuum', 'Bez oporu i napędu'], ['drag', 'Opór powietrza'], ['rocket', 'Rakieta z napędem']] as const).map(([model, label]) =>
          <label className="check-field" key={model} htmlFor={`${id}-${model}`}><input id={`${id}-${model}`} name="dynamicsModel" type="radio" value={model} checked={draft.model === model} onChange={() => edit({ model })} />{label}</label>)}
        {draft.model !== 'vacuum' && <>
          {!eligible && <p className="hint">Przed zapisem przygotuj lokalną kalibrację beta prędkości i grawitacji wybranego pocisku.</p>}
          <div className="form-grid">
            <NumberControl id={`${id}-linear`} name="linearDrag" label="Opór liniowy (1/s)" value={draft.linearDrag} onChange={linearDrag => edit({ linearDrag })} min={0} />
            <NumberControl id={`${id}-quadratic`} name="quadraticDrag" label={`Opór kwadratowy (1/${unitLabel})`} value={draft.quadraticDrag} onChange={quadraticDrag => edit({ quadraticDrag })} min={0} />
            {draft.model === 'rocket' && <>
              <NumberControl id={`${id}-acceleration`} name="rocketAcceleration" label={`Przyspieszenie rakiety (${unitLabel}/s²)`} value={draft.rocketAcceleration} onChange={rocketAcceleration => edit({ rocketAcceleration })} min={0} />
              <NumberControl id={`${id}-burn`} name="burnTime" label="Czas pracy napędu (s)" value={draft.burnTime} onChange={burnTime => edit({ burnTime })} min={Number.MIN_VALUE} />
            </>}
            <div className="field wide"><label htmlFor={`${id}-source`}>Źródło pomiarów oporu i napędu — wymagane</label><input id={`${id}-source`} name="dynamicsSource" value={draft.source} required maxLength={2048} onChange={event => edit({ source: event.target.value })} /></div>
          </div>
          {draft.model === 'rocket' && <label className="check-field" htmlFor={`${id}-fixed`}><input id={`${id}-fixed`} name="fixedLaunchDirection" type="checkbox" checked={draft.thrustDirectionFixedLaunch} required onChange={event => edit({ thrustDirectionFixedLaunch: event.target.checked })} />Przyjmuję stały kierunek napędu wyznaczony w chwili startu — wymagane</label>}
          <label className="check-field" htmlFor={`${id}-confirmed`}><input id={`${id}-confirmed`} name="localDynamicsConfirmed" type="checkbox" checked={draft.localCalibrationConfirmed} required onChange={event => edit({ localCalibrationConfirmed: event.target.checked })} />Potwierdzam lokalną kalibrację parametrów tego modelu na podstawie pomiarów w grze — wymagane</label>
        </>}
        <button className="primary" type="submit">Zastosuj model lotu</button>
      </fieldset>
    </form>
    <form onSubmit={event => { void simulate(event); }}>
      <fieldset disabled={busy}>
        <legend>Podgląd trajektorii</legend>
        <p className="hint">Początek w nieruchomym stanowisku. Wysokość jest liczona względem miejsca startu; podgląd nie uwzględnia zderzeń z terenem.</p>
        <div className="form-grid">
          <NumberControl id={`${id}-elevation`} name="previewElevation" label="Elewacja podglądu (°)" value={elevation} onChange={value => { clearFeedback(); setElevation(value); }} min={profile.minElevationDeg} max={profile.maxElevationDeg} />
          <NumberControl id={`${id}-azimuth`} name="previewAzimuth" label="Azymut podglądu (°)" value={azimuth} onChange={value => { clearFeedback(); setAzimuth(value); }} min={0} max={360} />
          <NumberControl id={`${id}-duration`} name="previewDuration" label="Czas podglądu (s)" value={duration} onChange={value => { clearFeedback(); setDuration(value); }} min={0.000001} max={600} />
        </div>
        <button type="submit">{busy ? 'Obliczanie trajektorii…' : 'Oblicz podgląd trajektorii'}</button>
      </fieldset>
    </form>
    {activePreview && first && last && <div className="fit-result" aria-live="polite" data-testid="dynamics-preview">
      <p>Początek: {coordinate(first.position)}.</p>
      <p>Po {last.time.toFixed(3)} s: {coordinate(last.position)}.</p>
      <ul className="assumptions">{activePreview.assumptions.map((assumption, index) => <li key={index}>{assumption}</li>)}</ul>
    </div>}
  </section>;
}
