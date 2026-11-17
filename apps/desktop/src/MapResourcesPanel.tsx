import { useEffect, useId, useRef, useState, type FormEvent } from 'react';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { call } from './api';
import {
  acceptResourceProgress, buildResourceRequest, expectedResourceItems,
  resourceKindLabel, resourceStatusLabel, validateResourceCatalog,
  type KnownMapResource, type ResourceLibrary, type ResourceProgress,
} from './resource-types';

export type { ResourceLibrary } from './resource-types';
type Props = {
  onInstall: (library: ResourceLibrary) => Promise<void>;
  notice: (message: string) => void;
  sourceAccepted: boolean;
};

export function MapResourcesPanel({ onInstall, notice, sourceAccepted }: Props) {
  const id = useId();
  const [catalog, setCatalog] = useState<KnownMapResource[]>([]);
  const [loading, setLoading] = useState(true);
  const [selected, setSelected] = useState<string[]>([]);
  const [includeTerrain, setIncludeTerrain] = useState(false);
  const [online, setOnline] = useState(false);
  const [accepted, setAccepted] = useState(false);
  const [phase, setPhase] = useState<'idle' | 'sync' | 'install'>('idle');
  const [progress, setProgress] = useState<ResourceProgress | null>(null);
  const [total, setTotal] = useState(0);
  const [library, setLibrary] = useState<ResourceLibrary | null>(null);
  const [installed, setInstalled] = useState(false);
  const [error, setError] = useState('');
  const [progressWarning, setProgressWarning] = useState('');
  const mounted = useRef(false);
  const operationPending = useRef(false);
  const catalogVersion = useRef(0);
  const progressListener = useRef<UnlistenFn | null>(null);
  const errorSummary = useRef<HTMLParagraphElement>(null);
  const busy = phase !== 'idle';
  const sourceAcknowledged = sourceAccepted || accepted;

  const fail = (caught: unknown) => {
    const message = caught instanceof Error ? caught.message : String(caught);
    if (mounted.current) {
      setError(message);
      requestAnimationFrame(() => errorSummary.current?.focus());
    }
    notice(message);
  };
  const loadCatalog = async () => {
    const version = ++catalogVersion.current;
    if (mounted.current) { setLoading(true); setError(''); }
    try {
      // This command returns bundled metadata only; it never downloads rasters.
      const next = validateResourceCatalog(await call<unknown>('resource_catalog'));
      if (mounted.current && version === catalogVersion.current) {
        setCatalog(next);
        setSelected(previous => previous.filter(key => next.some(map => map.id === key)));
      }
    } catch (caught) { if (mounted.current && version === catalogVersion.current) fail(caught); }
    finally { if (mounted.current && version === catalogVersion.current) setLoading(false); }
  };
  useEffect(() => {
    mounted.current = true;
    void loadCatalog();
    return () => {
      mounted.current = false;
      catalogVersion.current++;
      progressListener.current?.(); progressListener.current = null;
    };
  }, []);

  const changeSelection = (key: string, checked: boolean) => {
    setError('');
    setSelected(previous => checked ? [...new Set([...previous, key])] : previous.filter(value => value !== key));
  };
  const install = async (ready: ResourceLibrary) => {
    if (!ready.maps.length) throw new Error('Brak gotowych map do importu. Sprawdź ostrzeżenia i dostępność lokalnego cache.');
    if (mounted.current) setPhase('install');
    // A user-started import continues through parent installation even if the
    // panel is hidden. Local component state updates stop after unmount.
    await onInstall(ready);
    if (mounted.current) setInstalled(true);
    notice(`Dodano ${ready.maps.length} map oraz ${ready.terrains.length} map wysokości do lokalnej biblioteki.`);
  };
  const synchronize = async (event: FormEvent) => {
    event.preventDefault();
    if (operationPending.current) return;
    try {
      const request = buildResourceRequest(catalog, selected, includeTerrain, online, sourceAcknowledged);
      operationPending.current = true;
      if (mounted.current) {
        setPhase('sync'); setError(''); setProgressWarning(''); setProgress(null);
        setTotal(expectedResourceItems(catalog, request)); setLibrary(null); setInstalled(false);
      }
      // Subscribe before invoking sync so the first native completion cannot
      // race past the listener. No percentage is used to establish success.
      try {
        const stop = await listen<unknown>('map-resource-progress', event => {
          if (mounted.current) setProgress(previous => acceptResourceProgress(previous, event.payload, catalog, request));
        });
        if (mounted.current) progressListener.current = stop;
        else stop();
      } catch {
        if (mounted.current) setProgressWarning('Podgląd postępu jest niedostępny. Wynik pojawi się po zakończeniu operacji.');
      }
      const ready = await call<ResourceLibrary>('sync_resources', { ...request });
      if (mounted.current) setLibrary(ready);
      await install(ready);
    } catch (caught) { fail(caught); }
    finally {
      progressListener.current?.(); progressListener.current = null;
      operationPending.current = false;
      if (mounted.current) setPhase('idle');
    }
  };
  const retryInstallation = async () => {
    if (operationPending.current || !library) return;
    try {
      if (!sourceAcknowledged) throw new Error('Potwierdź użycie eksperymentalnego źródła map');
      operationPending.current = true;
      if (mounted.current) setError('');
      await install(library);
    } catch (caught) { fail(caught); }
    finally { operationPending.current = false; if (mounted.current) setPhase('idle'); }
  };
  const sources = catalog.filter((map, index, all) => all.findIndex(other => other.sourceMetadata.repository === map.sourceMetadata.repository && other.sourceMetadata.commit === map.sourceMetadata.commit) === index);
  const progressMap = catalog.find(map => map.id === progress?.id);
  const warnings = library ? [...new Set([...library.warnings, ...library.report.results.flatMap(result => result.error ? [`${catalog.find(map => map.id === result.id)?.name ?? result.id} — ${resourceKindLabel[result.kind]}: ${result.error.message}`] : [])])] : [];
  const allSelected = catalog.length > 0 && selected.length === catalog.length;
  return <section className="panel map-resources-panel">
    <h2>Mapy z katalogu społeczności</h2>
    <p className="hint">Wybierz mapy do lokalnej biblioteki. Katalog opisuje eksperymentalne źródło; skala, orientacja i zgodność z aktualną grą wymagają sprawdzenia.</p>
    {sources.map(map => <p className="source" key={`${map.sourceMetadata.repository}@${map.sourceMetadata.commit}`}>Źródło: {map.sourceMetadata.repository}<br />Commit: {map.sourceMetadata.commit}<br />Kod źródła: {map.sourceMetadata.codeLicense}{map.sourceMetadata.rasterLicense === 'unconfirmed' && <><br />Licencja obrazów: niepotwierdzona. Import służy lokalnemu cache; obrazy nie są przeznaczone do dołączania do paczek aplikacji.</>}</p>)}
    {error && <p className="notice" role="alert" tabIndex={-1} ref={errorSummary}>{error}</p>}
    <div className="button-group"><button type="button" disabled={busy || loading} onClick={() => { void loadCatalog(); }}>Odśwież katalog map</button></div>
    <form aria-busy={busy || loading} onSubmit={event => { void synchronize(event); }}>
      <fieldset disabled={busy || loading}>
        <legend>Wybór map ({selected.length}/{catalog.length})</legend>
        <div className="button-group">
          <button type="button" disabled={!catalog.length || allSelected} onClick={() => { setSelected(catalog.map(map => map.id)); setError(''); }}>Zaznacz wszystkie mapy</button>
          <button type="button" disabled={!selected.length} onClick={() => { setSelected([]); setError(''); }}>Odznacz wszystkie mapy</button>
        </div>
        {loading && <p role="status">Odczytywanie lokalnego katalogu…</p>}
        <div className="form-grid">
          {catalog.map(map => <label className="check-field" htmlFor={`${id}-${map.id}`} key={map.id}>
            <input id={`${id}-${map.id}`} name="resourceMap" value={map.id} type="checkbox" checked={selected.includes(map.id)} onChange={event => changeSelection(map.id, event.target.checked)} />
            <span>{map.name}<br /><small>{map.heightmapUrl ? 'Mapa i teren w źródle' : 'Mapa; brak terenu w źródle'}</small></span>
          </label>)}
        </div>
        <label className="check-field" htmlFor={`${id}-terrain`}><input id={`${id}-terrain`} name="resourceTerrain" type="checkbox" checked={includeTerrain} onChange={event => { setIncludeTerrain(event.target.checked); setError(''); }} />Dołącz dostępne mapy wysokości terenu</label>
        <label className="check-field" htmlFor={`${id}-online`}><input id={`${id}-online`} name="resourceOnline" type="checkbox" checked={online} onChange={event => { setOnline(event.target.checked); setError(''); }} />Pobierz z internetu (online)</label>
        <p className="hint">{online ? 'Wybrane pliki zostaną pobrane do lokalnego cache.' : 'Tryb offline sprawdza lokalny cache. Brakujące pliki zostaną zgłoszone.'}</p>
        {!sourceAccepted && <label className="check-field" htmlFor={`${id}-accepted`}><input id={`${id}-accepted`} name="resourceSourceAcknowledged" type="checkbox" checked={accepted} required onChange={event => { setAccepted(event.target.checked); setError(''); }} />Potwierdzam użycie eksperymentalnych map i terenu z opisanego źródła w lokalnym cache</label>}
        <button className="primary" type="submit">{online ? 'Pobierz i dodaj wybrane mapy' : 'Dodaj wybrane mapy z cache'}</button>
      </fieldset>
    </form>
    <div role="status" aria-live="polite" aria-atomic="true">
      {phase === 'sync' && <><p>Przetwarzanie plików: {progress?.completed ?? 0}/{total}{progress && ` — ${progressMap?.name ?? progress.id}, ${resourceKindLabel[progress.kind]}: ${resourceStatusLabel[progress.status]}`}</p><progress aria-label="Postęp przetwarzania plików map" max={total || 1} value={progress?.completed} /></>}
      {phase === 'install' && <p>Dodawanie gotowych map do biblioteki…</p>}
      {progressWarning && <p className="hint">{progressWarning}</p>}
      {library && <p>{installed ? 'Dodano do biblioteki' : 'Gotowe pliki'}: {library.maps.length} map, {library.terrains.length} map wysokości.</p>}
    </div>
    {warnings.length > 0 && <div className="resource-warnings"><h3>Ostrzeżenia importu</h3><ul className="assumptions">{warnings.map(warning => <li key={warning}>{warning}</li>)}</ul></div>}
    {library && <details><summary>Wyniki sprawdzania plików ({library.report.results.length})</summary><ul className="assumptions">{library.report.results.map(result => <li key={`${result.id}-${result.kind}`}>{catalog.find(map => map.id === result.id)?.name ?? result.id} — {resourceKindLabel[result.kind]}: {resourceStatusLabel[result.status]}{result.changed && '; źródłowy plik zmienił się'}</li>)}</ul></details>}
    {library && library.maps.length > 0 && !installed && <button type="button" disabled={busy} onClick={() => { void retryInstallation(); }}>Ponów dodanie gotowych map do biblioteki</button>}
  </section>;
}
