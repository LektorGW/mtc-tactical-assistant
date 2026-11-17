import { useEffect, useMemo, useState } from 'react';
import { AlertTriangle, Check, Download, Navigation, Play, RotateCcw, Save, Search, Settings2, Target, Upload } from 'lucide-react';
import { call, pickImage, pickJson, saveJson } from './api';
import { MapCanvas, type MapMarker } from './MapCanvas';
import type { SalvoPattern } from './salvo';
import { salvoLayerFor } from './map-salvo';
import { filterCatalog, inWikiRotation, WIKI_ROTATION } from './map-catalog';
import { MAX_GRID_DIVISIONS, gridCellSize, pointFromFields, sectorOf } from './map-geometry';
import { invalidateCatalogLoads, useCatalogMap } from './useCatalogMap';
import type { AssetsStatus, Calibration, CommunityMap, GridColumnStyle, Point, Profile, Result, Settings, Unit } from './types';
import { validateSettings } from './validation';
import { endSimulation } from './simulation';

type Tool = 'own' | 'target' | 'a' | 'b' | 'impact';
const labels: Record<Tool, string> = { own: 'Działo', target: 'Cel', a: 'Punkt A', b: 'Punkt B', impact: 'Trafienie' };

export type TerrainState = { available: boolean; active: boolean; loading: boolean };
type Props = {
  settings: Settings;
  update: (patch: Partial<Settings>) => void;
  setSettings: (next: (current: Settings) => Settings) => void;
  getSettings: () => Settings;
  log: (event: string) => void;
  notice: (message: string) => void;
  communityMaps: CommunityMap[];
  mapImage: string | null;
  setMapImage: (url: string | null) => void;
  setMapFingerprint: (sha: string | null) => void;
  result: Result | null;
  warning: string;
  profile: Profile | undefined;
  training: boolean;
  effectiveOwn: Point | null;
  effectiveTarget: Point | null;
  automaticPositions: boolean;
  terrain: TerrainState;
  assetsStatus: AssetsStatus | null;
  height: number | null;
  setHeight: (value: number | null) => void;
  /** Latest salvo forecast from the weapons tab (target-centred, profile unit). */
  salvoPattern: SalvoPattern | null;
  onDemo: () => void;
  native: boolean;
  /** Calibration and unit actually used by `calculate` (automatic map match included). */
  calibration: Calibration | null;
  calculationUnit: Unit;
  /** Resets height inputs and the salvo forecast after any map change. */
  onMapChanged: () => void;
  /** A stored terrain of an older (stretched) preparation exists for this map. */
  staleTerrain: boolean;
  /** Possible respawn: own position may be outdated (informational, never auto-cleared). */
  ownStale: boolean;
  onClearOwn: () => void;
  onDismissOwnStale: () => void;
  /** Automatic mode: last seen positions shown (not calculated) while markers are missing. */
  heldPositions: { own: Point | null; target: Point | null } | null;
  /** Automatic map recognition status (holding the last map etc.). */
  autoMapNote: string | null;
};

function NumberField({ label, value, onChange, min, max, step = 'any' }: { label: string; value: number | null; onChange: (n: number | null) => void; min?: number; max?: number; step?: string }) {
  return <label className="field"><span>{label}</span><input type="number" value={value ?? ''} min={min} max={max} step={step} onChange={e => onChange(e.target.value === '' || !Number.isFinite(e.target.valueAsNumber) ? null : e.target.valueAsNumber)} /></label>;
}
const fmt = (n: number | null | undefined, digits = 1) => n === undefined || n === null ? '—' : n.toFixed(digits);
const iso = () => new Date().toISOString();

export function MapPanel(props: Props) {
  const { settings: s, update, setSettings, getSettings, log, notice, communityMaps, mapImage, setMapImage, setMapFingerprint, result, warning, profile, training, effectiveOwn, effectiveTarget, automaticPositions, terrain, assetsStatus, height, setHeight, onDemo, native, salvoPattern, calibration, calculationUnit, onMapChanged, staleTerrain, ownStale, onClearOwn, onDismissOwnStale, heldPositions, autoMapNote } = props;
  const [tool, setTool] = useState<Tool>('own');
  const [a, setA] = useState<Point | null>(null);
  const [b, setB] = useState<Point | null>(null);
  const [impact, setImpact] = useState<Point | null>(null);
  const [referenceDistance, setReferenceDistance] = useState<number | null>(null);
  const [search, setSearch] = useState('');
  const [ownImageCovered, setOwnImageCovered] = useState(false);
  const catalog = useCatalogMap({ settings: getSettings, setSettings, showImage: (url, sha) => { onMapChanged(); setMapImage(url); setMapFingerprint(sha); setA(null); setB(null); setImpact(null); }, log });
  const listed = useMemo(() => filterCatalog(communityMaps, search), [communityMaps, search]);
  const advanced = s.mapSource === 'custom';
  const activeKnown = communityMaps.find(m => m.id === s.communityMapId);
  const catalogActive = !advanced && !!activeKnown && s.maps.some(m => m.id === activeKnown.id && m.path === s.mapPath);
  const attempt = async (fn: () => Promise<void>) => { try { await fn(); } catch (e) { notice(String(e).replace(/^Error: /, '')); } };

  const place = (p: Point, button: 'primary' | 'secondary') => {
    const active = button === 'secondary' ? 'target' : tool;
    const mode = automaticPositions ? 'assisted' : s.mode;
    if (active === 'own') { update({ own: p, mode }); setTool('target'); }
    else if (active === 'target') update({ target: p, mode });
    else if (active === 'a') { setA(p); setTool('b'); }
    else if (active === 'b') setB(p);
    else setImpact(p);
  };
  const moveMarker = (key: string, p: Point) => {
    const mode = automaticPositions ? 'assisted' : s.mode;
    if (key === 'own' || key === 'target') update({ [key]: p, mode });
    else if (key === 'a') setA(p); else if (key === 'b') setB(p); else if (key === 'impact') setImpact(p);
  };
  const ownSector = effectiveOwn ? sectorOf(effectiveOwn, s.mapWidth, s.mapHeight, s.mapGrid) : null;
  const targetSector = effectiveTarget ? sectorOf(effectiveTarget, s.mapWidth, s.mapHeight, s.mapGrid) : null;
  const markers: MapMarker[] = [
    ...(effectiveOwn ? [{ key: 'own', point: effectiveOwn, label: `DZIAŁO${ownSector ? ` ${ownSector.label}` : ''}`, color: '#67c7c1', draggable: true }] : []),
    ...(effectiveTarget ? [{ key: 'target', point: effectiveTarget, label: `CEL${targetSector ? ` ${targetSector.label}` : ''}`, color: '#e6af46', draggable: true }] : []),
    ...(!effectiveOwn && heldPositions?.own ? [{ key: 'held-own', point: heldPositions.own, label: 'DZIAŁO (ostatnie)', color: '#5f8f8c' }] : []),
    ...(!effectiveTarget && heldPositions?.target ? [{ key: 'held-target', point: heldPositions.target, label: 'CEL (ostatni)', color: '#9c824f' }] : []),
    ...(advanced && a ? [{ key: 'a', point: a, label: 'A', color: '#b798f4', draggable: true }] : []),
    ...(advanced && b ? [{ key: 'b', point: b, label: 'B', color: '#b798f4', draggable: true }] : []),
    ...(impact ? [{ key: 'impact', point: impact, label: 'TRAFIENIE', color: '#ef897e', draggable: true }] : []),
  ];
  const usedScale = calibration?.unitsPerPixel ?? null;
  const usedRotation = calibration?.northRotationDeg ?? s.rotation;
  const salvoLayer = salvoLayerFor(salvoPattern, effectiveTarget, usedScale, usedRotation, calculationUnit);
  const arc = result?.intercept3d?.arcs[0] ?? result?.solution?.arcs[0];
  const azimuth = result?.intercept3d?.arcs[0]?.azimuthDeg ?? result?.bearing.azimuthDeg;
  const cell = calibration ? gridCellSize(calibration.unitsPerPixel, s.mapWidth, s.mapGrid.divisions) : null;
  const cellDifference = cell && s.gameGridSize ? (s.gameGridSize - cell) / cell * 100 : null;
  const tools: Tool[] = advanced ? ['own', 'target', 'a', 'b', 'impact'] : ['own', 'target', 'impact'];

  const switchSource = (mapSource: Settings['mapSource']) => { invalidateCatalogLoads(); update({ mapSource }); };
  const importMap = () => attempt(async () => {
    const path = await pickImage(); if (!path) return;
    invalidateCatalogLoads(); onMapChanged();
    const img = await call<{ dataUrl: string; width: number; height: number; sha256: string }>('import_image', { path });
    setMapImage(img.dataUrl); setMapFingerprint(img.sha256); setA(null); setB(null); setImpact(null);
    setSettings(current => ({ ...endSimulation(current), mapSource: 'custom', mapPath: path, mapName: path.split(/[\\/]/).pop()!, mapWidth: img.width, mapHeight: img.height, scale: null, rotation: 0, own: null, target: null, communityMapId: null, markerConfig: { ...current.markerConfig, mapWidth: img.width, mapHeight: img.height, coordinateCalibrationConfirmed: false, viewSignature: null } }));
    log('Import mapy; poprzednia skala i oznaczenia usunięte');
  });
  const calibrate = () => attempt(async () => {
    if (!a || !b || !referenceDistance) throw new Error('Wskaż punkty A i B oraz znany dystans między nimi');
    const scale = await call<number>('calibrate_map', { a, b, distance: referenceDistance });
    update({ scale }); log(`Skala: ${scale.toFixed(6)} ${s.unit}/px; dystans referencyjny: ${referenceDistance}`); setTool('own');
  });
  const useCommunityScale = () => attempt(async () => {
    if (!s.communityMapId || !ownImageCovered) throw new Error('Wybierz mapę i potwierdź, że obraz obejmuje całą kwadratową mapę');
    if (Math.abs(s.mapWidth / s.mapHeight - 1) > 0.01) throw new Error('Wymiar źródłowy dotyczy pełnej kwadratowej mapy; obecny obraz ma inne proporcje');
    const scale = await call<number>('community_map_scale', { id: s.communityMapId, width: s.mapWidth, sourceAcknowledged: true });
    update({ scale, rotation: 0, mapUnit: 'game_meters', unit: 'm gry' }); log(`Użyto wymiaru mapy ze źródła community: ${s.communityMapId}`);
  });
  const recordShot = () => {
    if (!result) return;
    update({ history: [...s.history, { at: iso(), distance: result.bearing.distance, azimuth: result.bearing.azimuthDeg, unit: s.unit, mapUnit: s.mapUnit, note: `${profile?.name}; ${training ? 'symulacja' : `${s.mapName}; ${ownSector?.label ?? '?'} → ${targetSector?.label ?? '?'}`}` }].slice(-50) });
  };
  const correctAim = () => {
    if (!impact || !effectiveTarget || !effectiveOwn) return;
    const corrected = { x: 2 * effectiveTarget.x - impact.x, y: 2 * effectiveTarget.y - impact.y };
    if (corrected.x < 0 || corrected.y < 0 || corrected.x > s.mapWidth - 1 || corrected.y > s.mapHeight - 1) { notice('Skorygowany punkt wychodzi poza mapę'); return; }
    update({ own: effectiveOwn, target: corrected, mode: automaticPositions ? 'assisted' : s.mode }); log(`Korekta punktu celowania o wektor (${(effectiveTarget.x - impact.x).toFixed(1)}, ${(effectiveTarget.y - impact.y).toFixed(1)}) px`); setImpact(null);
  };
  const setGrid = (patch: Partial<Settings['mapGrid']>) => update({ mapGrid: { ...s.mapGrid, ...patch } });
  const heightLine = result?.heightSource === 'terrain' ? `${fmt(result.heightDelta)} ${s.unit} (mapa wysokości)` : result?.heightSource === 'manual' ? `${fmt(result.heightDelta)} ${s.unit} (wpisana ręcznie)` : 'brak danych';

  return <>
    <section className="panel">
      <div className="panel-heading"><div><h2>Mapa operacyjna</h2><p>{s.mapName}{catalogActive && ' · skala z katalogu, północ u góry'}</p></div>
        <div className="button-group"><button onClick={onDemo}><Play size={15} />Symulacja</button></div></div>
      <div className="mode-switch" role="tablist" aria-label="Źródło mapy">
        <button role="tab" aria-selected={!advanced} className={!advanced ? 'active' : ''} onClick={() => switchSource('catalog')}><Download size={14} />Mapa z katalogu</button>
        <button role="tab" aria-selected={advanced} className={advanced ? 'active' : ''} onClick={() => switchSource('custom')}><Settings2 size={14} />Własny obraz (zaawansowane)</button>
      </div>

      {!advanced && <div className="catalog-picker">
        {!s.catalogSourceAccepted ? <div className="consent-box">
          <p><b>Mapy społeczności — dane eksperymentalne.</b> Tekstury, mapy wysokości i wymiary pochodzą z publicznego kalkulatora {activeKnown?.sourceMetadata.repository ?? 'grand-hawk/artillery-calculator'} (przypięty commit, licencja kodu MIT; licencja obrazów niepotwierdzona). Pliki są pobierane jednorazowo do lokalnego cache i nie trafiają do paczek aplikacji. Zgodność z aktualną wersją MTC nie jest potwierdzona.</p>
          <button className="primary" onClick={() => { update({ catalogSourceAccepted: true }); log('Zaakceptowano eksperymentalne źródło map społeczności (pobieranie do lokalnego cache)'); }}><Check size={15} />Akceptuję źródło i pobieranie map</button>
        </div> : <>
          <label className="field search-field"><span><Search size={12} /> Szukaj mapy ({communityMaps.length})</span><input value={search} onChange={e => setSearch(e.target.value)} placeholder="Np. Normandy, Zone 11, Roinburg" /></label>
          <div className="catalog-list" role="listbox" aria-label="Mapy z katalogu">
            {listed.map(m => <button key={m.id} role="option" aria-selected={s.communityMapId === m.id} className={s.communityMapId === m.id && catalogActive ? 'active' : ''} disabled={catalog.load.phase === 'loading'} onClick={() => void attempt(() => catalog.open(m))}>
              <b>{m.name}</b><small>{m.sizeGameMeters.toFixed(0)} m gry · pole {(m.sizeGameMeters / 9).toFixed(0)} m{m.heightRangeStuds ? ' · teren' : ' · bez terenu'}{inWikiRotation(m.id) ? ' · w rotacji' : ''}</small>
            </button>)}
            {!listed.length && <p className="hint">Brak mapy o tej nazwie.</p>}
          </div>
          <p className="hint">Najpierw mapy w rotacji wg multicrew.wiki ({WIKI_ROTATION.checkedAt}). Skala = pełny wymiar mapy / szerokość tekstury; północ = góra. {native ? '' : 'Pobieranie wymaga wersji .exe.'}</p>
        </>}
        {catalog.load.phase !== 'idle' && <div className={`catalog-status ${catalog.load.phase}`} role="status" aria-live="polite">
          {catalog.load.phase === 'error' && <AlertTriangle size={14} />}{catalog.load.message}
          {catalog.load.phase === 'error' && <div className="button-group"><button onClick={() => switchSource('custom')}><Upload size={14} />Przejdź do ręcznego importu</button></div>}
          {catalog.load.warnings.length > 0 && <ul className="assumptions">{catalog.load.warnings.map(w => <li key={w}>{w}</li>)}</ul>}
        </div>}
      </div>}

      {advanced && <div className="advanced-tools">
        <div className="button-group"><button onClick={importMap}><Upload size={15} />Importuj obraz mapy</button></div>
        <p className="hint">Własny obraz wymaga skali: wskaż A i B o znanym dystansie albo — dla pełnej kwadratowej mapy — użyj wymiaru z katalogu.</p>
      </div>}

      <div className="map-tools">{tools.map(t => <button key={t} className={tool === t ? 'active' : ''} onClick={() => setTool(t)}>{t === 'own' ? <Navigation size={14} /> : t === 'target' ? <Target size={14} /> : null}{labels[t]}</button>)}
        <span>LPM: {labels[tool].toLowerCase()} · PPM: cel · przeciągnij punkt lub mapę · kółko: zoom</span></div>
      <div className="map-wrap">
        <MapCanvas image={mapImage} width={s.mapWidth} height={s.mapHeight} grid={s.mapGrid} markers={markers}
          line={effectiveOwn && effectiveTarget ? { from: effectiveOwn, to: effectiveTarget, color: '#e6af46' } : null}
          secondaryLine={advanced && a && b ? { from: a, to: b, color: '#a88bee' } : null}
          scale={usedScale} rotation={usedRotation} unit={s.unit} dispersionRadius={result?.solution?.dispersionRadius95 ?? null} salvo={salvoLayer.layer}
          placeholder={advanced ? 'Zaimportuj obraz mapy lub uruchom symulację' : 'Wybierz mapę z katalogu'}
          onPlace={place} onMove={moveMarker} />
        <div className="map-footer"><span>{usedScale ? `${usedScale.toFixed(4)} ${s.unit}/px` : 'SKALA NIEUSTAWIONA'}{cell ? ` · pole siatki ${cell.toFixed(1)} ${s.unit}` : ''}</span><span>Góra mapy: azymut {usedRotation}° · siatka {s.mapGrid.divisions}×{s.mapGrid.divisions}</span></div>
      </div>

      {ownStale && <div className="catalog-status loading respawn-note" role="status">Własna pozycja sprzed odrodzenia? Odczyt panelu broni był przerwany na dłużej niż 10 s.<div className="button-group"><button onClick={onClearOwn}>Wyczyść pozycję działa</button><button onClick={onDismissOwnStale}>Pozycja aktualna</button></div></div>}
      {autoMapNote && <p className="hint">{autoMapNote}</p>}
      <div className="map-readout" aria-live="polite">
        <div><small>DZIAŁO</small><b>{ownSector?.label ?? '—'}</b><span>{effectiveOwn ? `${effectiveOwn.x.toFixed(0)}, ${effectiveOwn.y.toFixed(0)} px` : 'kliknij mapę'}</span></div>
        <div><small>CEL</small><b>{targetSector?.label ?? '—'}</b><span>{effectiveTarget ? `${effectiveTarget.x.toFixed(0)}, ${effectiveTarget.y.toFixed(0)} px` : 'PPM lub narzędzie Cel'}</span></div>
        <div><small>DYSTANS / AZYMUT</small><b>{fmt(result?.bearing.distance)} {s.unit}</b><span>{fmt(azimuth)}° (N = 0°, E = 90°)</span></div>
        <div><small>RÓŻNICA WYSOKOŚCI</small><b>{fmt(result?.heightDelta)}{result?.heightDelta !== undefined && result?.heightDelta !== null ? ` ${s.unit}` : ''}</b><span>{heightLine}</span></div>
        <div><small>ELEWACJA / CZAS LOTU</small><b>{fmt(arc?.elevationDeg, 2)}° / {fmt(arc?.flightTime, 2)} s</b><span>{arc ? (result?.solution?.arcs.length ?? 0) > 1 ? 'niski tor; wysoki w zakładce Profile' : 'rozwiązanie profilu' : result?.ballisticWarning || warning || 'wymaga profilu pocisku'}</span></div>
      </div>
      {(result?.ballisticWarning || warning) && <p className="hint warning-inline"><AlertTriangle size={13} /> {warning || result?.ballisticWarning}</p>}
      {salvoPattern && <p className={`hint salvo-map-note ${salvoLayer.layer ? '' : 'warning-inline'}`}>{salvoLayer.layer ? <>Salwa ({salvoPattern.status === 'beta' ? 'beta' : 'eksperymentalna'}, {salvoPattern.salvoSize} pocisków): elipsa salwy 95% {salvoPattern.salvoEnvelope.semiMajor.toFixed(0)}×{salvoPattern.salvoEnvelope.semiMinor.toFixed(0)} {s.unit} (półosie); linia ciągła — salwa, przerywana — pojedynczy pocisk, wewnętrzna — średni punkt trafienia, krzyżyk — prognozowany środek.</> : salvoLayer.reason}{salvoPattern.missingComponents.length > 0 && <> Brak składników modelu: {salvoPattern.missingComponents.join(', ')} — rzeczywisty rozrzut może być większy.</>}</p>}
      {result?.ballisticWarning?.includes('Jednostki') && <p className="hint">Wybierz profil w {s.mapUnit === 'game_meters' ? 'metrach gry' : 'jednostkach gry'} (np. z katalogu społeczności, „BM-21 Grad / 9M22…”) lub przelicz mapę. Profil „{profile?.name}” liczy w innych jednostkach. Dane społeczności potwierdź w zakładce <b>Profile i balistyka</b>; profil rozpoznany z gry możesz przyjąć w zakładce <b>Celowanie</b>.</p>}

      <div className="height-row">
        <label className="check-field"><input type="checkbox" checked={s.useTerrainHeight} onChange={e => update({ useTerrainHeight: e.target.checked })} />Różnica wysokości z mapy wysokości (automatycznie, gdy jest dostępna)</label>
        <span className={`terrain-tag ${terrain.active ? 'ok' : ''}`}>{terrain.loading ? 'Wczytywanie terenu…' : terrain.active ? 'Teren aktywny (eksperymentalny)' : terrain.available ? 'Teren wyłączony' : staleTerrain ? 'Teren w starym formacie — wybierz mapę ponownie z katalogu' : 'Brak mapy wysokości dla tej mapy'}</span>
        <NumberField label={`Ręczna różnica wysokości: cel wyżej o (${s.unit})`} value={height} onChange={setHeight} />
      </div>
      {terrain.active && catalogActive && <p className="hint">Założenie (status eksperymentalny): mapa wysokości i tekstura pochodzą z tego samego źródła i obejmują ten sam kwadrat mapy; niekwadratowy teren jest wyśrodkowany. Wyrównanie nie zostało zmierzone w grze — porównaj Δh z obserwacją.</p>}
      {assetsStatus && assetsStatus.warnings.length > 0 && <details className="resource-warnings"><summary>Pominięte pliki biblioteki ({assetsStatus.warnings.length})</summary><ul className="assumptions">{assetsStatus.warnings.map(w => <li key={w}>{w}</li>)}</ul></details>}

      <details className="grid-settings">
        <summary>Siatka sektorów i kontrola skali</summary>
        <div className="form-grid">
          <label className="check-field"><input type="checkbox" checked={s.mapGrid.visible} onChange={e => setGrid({ visible: e.target.checked })} />Pokaż siatkę</label>
          <NumberField label={`Liczba pól na bok (1–${MAX_GRID_DIVISIONS})`} value={s.mapGrid.divisions} min={1} max={MAX_GRID_DIVISIONS} step="1" onChange={n => { if (n !== null && Number.isInteger(n) && n >= 1 && n <= MAX_GRID_DIVISIONS) setGrid({ divisions: n }); }} />
          <label className="field"><span>Kolumny</span><select value={s.mapGrid.columns} onChange={e => setGrid({ columns: e.target.value as GridColumnStyle })}><option value="letters-no-i">Litery bez I (A…H, J…)</option><option value="letters">Litery A–Z</option><option value="numbers">Liczby</option></select></label>
          <label className="field"><span>Numeracja wierszy</span><select value={s.mapGrid.rowsFrom} onChange={e => setGrid({ rowsFrom: e.target.value as 'top' | 'bottom' })}><option value="top">Od góry (1 u góry)</option><option value="bottom">Od dołu (1 na dole)</option></select></label>
          <NumberField label={`Rozmiar pola z gry (${s.unit}) — lewy dolny róg mapy, klawisz M`} value={s.gameGridSize} min={0} onChange={n => update({ gameGridSize: n !== null && n > 0 ? n : null })} />
        </div>
        <p className="hint">Siatka {s.mapGrid.divisions}×{s.mapGrid.divisions} obejmuje cały obraz (kalkulator źródłowy: mapa = 9 pól). Domyślne etykiety (kolumny A–H, J bez I; wiersze 1–9 od góry; sektor np. „C7”) są zgodne ze zrzutem gry z 2026-10-15 (minimapa); mogą się zmienić z aktualizacją MTC. {cell ? `Pole według bieżącej skali: ${cell.toFixed(1)} ${s.unit}.` : ''} {cellDifference !== null ? Math.abs(cellDifference) <= 2 ? `Zgodne z wartością z gry (${cellDifference.toFixed(1)}%).` : `Różnica ${cellDifference.toFixed(1)}% względem gry — katalog może dotyczyć innej wersji mapy; rozważ własną kalibrację w trybie zaawansowanym.` : ''}</p>
      </details>

      <div className="coordinates">{(['own', 'target'] as const).map(key => <PointFields key={key} legend={key === 'own' ? 'Działo (px obrazu)' : 'Cel / punkt celowania (px)'} value={s[key]} width={s.mapWidth} height={s.mapHeight} onChange={point => update({ [key]: point })} />)}
        <div className="coordinate-actions"><button onClick={() => { update({ own: null, target: null }); setImpact(null); }}><RotateCcw size={14} />Wyczyść</button><button onClick={recordShot} disabled={!result}><Save size={14} />Zapisz rozwiązanie</button></div></div>
      {impact && <div className="correction"><span>Korekta ręczna: przesunięcie punktu celowania przeciwne do błędu trafienia.</span><button onClick={correctAim} disabled={!effectiveTarget || !effectiveOwn}>Zastosuj korektę</button></div>}
    </section>

    {advanced && <section className="panel"><div className="panel-heading"><div><h2>Kalibracja własnego obrazu</h2><p>Wskaż A i B oraz znany dystans. Jednostki ustal na podstawie gry.</p></div><Settings2 size={20} /></div>
      <div className="calibration-grid"><NumberField label={`Dystans A–B (${s.unit})`} value={referenceDistance} onChange={setReferenceDistance} min={0.001} /><button className="primary align-end" onClick={calibrate}>Wyznacz skalę</button><NumberField label="Skala (jedn./px)" value={s.scale} onChange={scale => update({ scale: scale !== null && scale > 0 ? scale : null })} min={0.000001} /><NumberField label="Góra mapy: azymut (°)" value={s.rotation} onChange={rotation => update({ rotation: rotation ?? 0 })} /><label className="field"><span>Układ jednostek obliczeń</span><select value={s.mapUnit} onChange={e => update({ mapUnit: e.target.value as Settings['mapUnit'], unit: e.target.value === 'game_meters' ? 'm gry' : 'jedn. gry' })}><option value="game_units">Własne jednostki gry</option><option value="game_meters">Metry gry (przelicznik społeczności)</option></select></label><NumberField label="Niepewność pozycji (jedn.)" value={s.uncertainty} onChange={uncertainty => update({ uncertainty: uncertainty !== null && uncertainty >= 0 ? uncertainty : 0 })} min={0} /><label className="field"><span>Nazwa jednostki</span><input value={s.unit} maxLength={30} onChange={e => update({ unit: e.target.value })} /></label></div>
      <div className="map-catalog"><label className="field"><span>Wymiar z katalogu dla własnego obrazu pełnej mapy</span><select value={s.communityMapId ?? ''} onChange={e => { update({ communityMapId: e.target.value || null }); setOwnImageCovered(false); }}><option value="">Wybierz nazwę znanej mapy…</option>{filterCatalog(communityMaps, '').map(m => <option key={m.id} value={m.id}>{m.name} — {m.sizeGameMeters.toFixed(0)} m gry</option>)}</select></label><label className="check-field"><input type="checkbox" checked={ownImageCovered} onChange={e => setOwnImageCovered(e.target.checked)} />Obraz obejmuje pełną kwadratową mapę (bez przycięcia i obrotu)</label><button onClick={useCommunityScale} disabled={!s.catalogSourceAccepted}>Użyj wymiaru źródłowego</button></div>
      <p className="hint">Skala z jednego odcinka zakłada płaską mapę bez perspektywy. Fotografie monitora wymagają wcześniejszego prostowania.{!s.catalogSourceAccepted && ' Wymiar z katalogu wymaga akceptacji źródła w trybie „Mapa z katalogu”.'}</p>
      <div className="button-group"><button onClick={() => void attempt(() => saveJson('mtc-oznaczenia.json', { schemaVersion: 1, settings: s }))}>Eksportuj oznaczenia</button><button onClick={() => void attempt(async () => { const path = await pickJson(); if (!path) return; const value = await call<{ schemaVersion: number; settings: Settings }>('import_markers', { path }); if (value.schemaVersion !== 1) throw new Error('Nieobsługiwana wersja oznaczeń'); const imported = validateSettings(value.settings); if (imported.mapWidth !== s.mapWidth || imported.mapHeight !== s.mapHeight || imported.mapName !== s.mapName) throw new Error('Oznaczenia dotyczą innej mapy — najpierw zaimportuj właściwy obraz'); update({ own: imported.own, target: imported.target, scale: imported.scale, rotation: imported.rotation, uncertainty: imported.uncertainty, unit: imported.unit, mapUnit: imported.mapUnit }); log('Import oznaczeń lokalnych'); })}>Importuj oznaczenia</button></div>
    </section>}
  </>;
}


function PointFields({ legend, value, width, height, onChange }: { legend: string; value: Point | null; width: number; height: number; onChange: (point: Point | null) => void }) {
  const [x, setX] = useState<number | null>(value?.x ?? null);
  const [y, setY] = useState<number | null>(value?.y ?? null);
  useEffect(() => { setX(value?.x ?? null); setY(value?.y ?? null); }, [value?.x, value?.y]);
  const commit = (nx: number | null, ny: number | null) => {
    setX(nx); setY(ny);
    const point = pointFromFields(nx, ny, width, height);
    if (point !== 'incomplete') onChange(point);
  };
  const incomplete = pointFromFields(x, y, width, height) === 'incomplete';
  return <fieldset><legend>{legend}</legend>
    <NumberField label="X" value={x} min={0} max={width} onChange={nx => commit(nx, y)} />
    <NumberField label="Y" value={y} min={0} max={height} onChange={ny => commit(x, ny)} />
    {incomplete && <small className="hint">Podaj obie współrzędne w zakresie obrazu.</small>}
  </fieldset>;
}
