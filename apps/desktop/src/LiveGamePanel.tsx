import { useRef, useState, type PointerEvent as ReactPointerEvent } from 'react';
import { AlertTriangle, Crosshair, Map as MapIcon, Play, RefreshCw, Square } from 'lucide-react';
import { call } from './api';
import { equivalentNames, formatAngle, fractionToTexturePixel, panelFresh, type AimCorrection, type RequiredAim } from './aim-correction';
import { clientToImage } from './map-geometry';
import type { MtcMapPoint, MtcMapReading, WeaponPanelAnalysis } from './mtc-hud-types';
import type { CatalogLoad } from './useCatalogMap';
import type { CommunityMap, Point, Profile, Settings } from './types';

type Live = { running: boolean; setRunning: (running: boolean) => void; analysis: WeaponPanelAnalysis | null; error: string };
type Props = {
  settings: Settings;
  update: (patch: Partial<Settings>) => void;
  windows: { handle: number; title: string }[];
  refreshWindows: () => void;
  handle: number | null;
  chooseWindow: (handle: number | null) => void;
  live: Live;
  now: number;
  required: RequiredAim | null;
  correction: AimCorrection | null;
  preferHigh: boolean;
  setPreferHigh: (high: boolean) => void;
  profile: Profile | undefined;
  profiles: Profile[];
  autoProfile: boolean;
  setAutoProfile: (auto: boolean) => void;
  communityMaps: CommunityMap[];
  openCatalog: (known: CommunityMap) => Promise<CatalogLoad | null>;
  notice: (message: string) => void;
  /** Synthetic simulation data in use: no correction against the real panel. */
  simulation: boolean;
  onEndSimulation: () => void;
  /** Map and profile units differ (no ballistic solution). */
  unitMismatch: boolean;
  ownStale: boolean;
  onClearOwn: () => void;
  onDismissOwnStale: () => void;
};
type GameMap = { reading: MtcMapReading; preview: string; width: number; height: number; chosenId: string | null };
const angle = (value: number | undefined | null) => value === undefined || value === null ? '—' : formatAngle(value);

/** Live MTC HUD: weapon panel, aim advice and in-game map recognition. Advice only — no input is sent to the game. */
export function LiveGamePanel(props: Props) {
  const { settings: s, update, windows, refreshWindows, handle, chooseWindow, live, now, required, correction, preferHigh, setPreferHigh, profile, profiles, autoProfile, setAutoProfile, communityMaps, openCatalog, notice, simulation, onEndSimulation, unitMismatch, ownStale, onClearOwn, onDismissOwnStale } = props;
  const [gameMap, setGameMap] = useState<GameMap | null>(null);
  const [mapBusy, setMapBusy] = useState(false);
  const [mapMessage, setMapMessage] = useState('');
  const [clickRole, setClickRole] = useState<'own' | 'target'>('own');
  const [clicks, setClicks] = useState<{ own?: Point & { sector: string }; target?: Point & { sector: string } }>({});
  const svg = useRef<SVGSVGElement>(null);
  const panel = live.analysis?.panel ?? null;
  const fresh = panelFresh(panel, now);
  const shown = fresh ? panel : null;
  const resolution = fresh ? live.analysis?.profile ?? null : null;
  const suggested = resolution?.status === 'matched' && resolution.profileId ? profiles.find(p => p.id === resolution.profileId) : undefined;
  const weaponName = shown?.weapon?.catalogMatch ?? shown?.weapon?.text ?? null;
  const equivalents = equivalentNames(resolution, suggested?.id ?? null, profiles);

  const loadKnown = async (id: string) => {
    const known = communityMaps.find(m => m.id === id);
    if (!known) { setMapMessage(`Mapa ${id} nie występuje w katalogu.`); return; }
    if (!s.catalogSourceAccepted) { setMapMessage(`Rozpoznano: ${known.name}. Aby ją wczytać, zaakceptuj źródło map społeczności (przycisk poniżej).`); return; }
    const load = await openCatalog(known);
    if (load?.phase === 'error') setMapMessage(load.message);
  };
  const recognize = async () => {
    setMapMessage('');
    if (handle === null) { setMapMessage('Najpierw wybierz okno gry.'); return; }
    setMapBusy(true);
    try {
      const preview = await call<{ dataUrl: string; width: number; height: number }>('capture_preview', { handle, backend: s.captureBackend });
      const reading = await call<MtcMapReading>('read_mtc_map', { handle, backend: s.captureBackend });
      if (preview.width !== reading.frameWidth || preview.height !== reading.frameHeight) throw new Error('Rozmiar okna zmienił się między klatkami — spróbuj ponownie przy otwartej, nieruchomej mapie.');
      setGameMap({ reading, preview: preview.dataUrl, width: preview.width, height: preview.height, chosenId: null });
      setClicks({});
      const cell = reading.gridSizeMeters ? ` (pole ${Math.round(reading.gridSizeMeters)} m)` : '';
      if (reading.status === 'ready' && reading.mapId) {
        const name = communityMaps.find(m => m.id === reading.mapId)?.name ?? reading.mapId;
        setMapMessage(`Rozpoznano: ${name}${cell}.`);
        await loadKnown(reading.mapId);
      } else if (reading.status === 'ambiguous') setMapMessage(`Kilka map ma ten rozmiar pola${cell} — wybierz właściwą.`);
      else if (reading.status === 'unidentified') setMapMessage(`Siatka znaleziona, ale nie rozpoznano mapy${cell}. Wybierz mapę ręcznie w zakładce Mapa i cel.`);
      else setMapMessage('Nie widać pełnej mapy. Otwórz mapę w grze klawiszem M i spróbuj ponownie.');
    } catch (e) { setMapMessage(String(e).replace(/^Error: /, '')); }
    finally { setMapBusy(false); }
  };
  const gameMapId = gameMap ? gameMap.chosenId ?? gameMap.reading.mapId : null;
  const clickable = !!gameMap?.reading.grid && !!gameMapId && s.mapSource === 'catalog' && s.communityMapId === gameMapId;
  const clickMap = async (event: ReactPointerEvent<SVGSVGElement>) => {
    if (!gameMap || !svg.current) return;
    if (!clickable) { notice('Najpierw wczytaj rozpoznaną mapę z katalogu.'); return; }
    const pixel = clientToImage(event.clientX, event.clientY, svg.current.getBoundingClientRect(), { x: 0, y: 0, width: gameMap.width, height: gameMap.height });
    try {
      const point = await call<MtcMapPoint>('mtc_map_point', { reading: gameMap.reading, point: pixel });
      const texture = fractionToTexturePixel(point.fraction, s.mapWidth, s.mapHeight);
      if (!texture) throw new Error('Punkt poza siatką mapy');
      update({ [clickRole]: texture, mode: s.mode === 'automatic' ? 'assisted' : s.mode });
      setClicks(old => ({ ...old, [clickRole]: { ...pixel, sector: point.sector } }));
      if (clickRole === 'own') setClickRole('target');
    } catch (e) { notice(String(e).replace(/^Error: /, '')); }
  };

  const big = (axis: AimCorrection['elevation']) => axis ? <strong className={axis.aligned ? 'aligned' : ''}>{axis.label}</strong> : <strong className="missing">brak odczytu</strong>;
  const aimCard = <>
    <section className="panel aim-panel">
      <div className="panel-heading"><div><h2>Korekta celowania</h2><p>Wymagane − bieżące, dla wybranego toru. Ustaw działo ręcznie w grze.</p></div></div>
      <div className="mode-switch"><button className={!preferHigh ? 'active' : ''} onClick={() => setPreferHigh(false)}>Tor niski</button><button className={preferHigh ? 'active' : ''} onClick={() => setPreferHigh(true)}>Tor wysoki</button></div>
      {simulation ? <div className="catalog-status error" role="status">Tryb symulacji — korekta wyłączona (wczytaj mapę).<div className="button-group"><button onClick={onEndSimulation}>Zakończ symulację</button></div></div>
      : unitMismatch ? <div className="catalog-status error" role="status">Jednostki mapy i profilu są różne — wybierz profil w metrach gry (np. z katalogu społeczności) lub przelicz mapę.{suggested && suggested.id !== s.selected && <div className="button-group"><button className="primary" onClick={() => update({ selected: suggested.id })}>Przełącz na profil z gry: {suggested.name}</button></div>}</div>
      : !required ? <p className="hint">Brak rozwiązania kalkulatora: wskaż działo i cel na mapie oraz wybierz profil z danymi pocisku (w tych samych jednostkach co mapa).</p>
        : <p className="hint">Wymagane: elewacja {angle(required.elevationDeg)}, azymut {angle(required.azimuthDeg)}, lot {required.flightTime.toFixed(2).replace('.', ',')} s ({required.highArc ? 'tor wysoki' : 'tor niski'}).</p>}
      {!simulation && correction ? <div className={`aim-correction ${correction.onTarget ? 'on-target' : ''}`} role="status" aria-live="polite">
        {correction.onTarget ? <strong className="aligned">✓ NA CELU</strong> : <>{big(correction.elevation)}{big(correction.azimuth)}</>}
      </div> : !simulation && required && <p className="hint">{live.running ? 'Czekam na świeży odczyt ELEVATION/AZIMUTH z panelu broni.' : 'Uruchom odczyt panelu broni, aby zobaczyć korektę.'}</p>}
      <p className="hint">Założenie do sprawdzenia w grze: AZIMUTH panelu to kurs 0–360° od północy zgodnie z ruchem wskazówek zegara, ELEVATION to kąt lufy nad poziomem. Próg „na celu”: 0,1°.</p>
    </section>
  </>;
  return <>
    {live.running && aimCard}
    {ownStale && <div className="catalog-status loading respawn-note" role="status">Własna pozycja sprzed odrodzenia? Odczyt panelu broni był przerwany na dłużej niż 10 s.<div className="button-group"><button onClick={onClearOwn}>Wyczyść pozycję działa</button><button onClick={onDismissOwnStale}>Pozycja aktualna</button></div></div>}
    <section className="panel live-panel">
      <div className="panel-heading"><div><h2>Gra na żywo (MTC)</h2><p>Odczyt widocznego panelu broni i mapy. Aplikacja tylko podpowiada — nie steruje działem ani nie strzela.</p></div><Crosshair size={22} /></div>
      <div className="form-grid">
        <label className="field"><span>Okno gry</span><select value={handle ?? ''} onChange={e => chooseWindow(e.target.value ? Number(e.target.value) : null)}><option value="">Wybierz okno Roblox…</option>{windows.map(w => <option key={w.handle} value={w.handle}>{w.title}</option>)}</select></label>
        <div className="button-group align-end"><button onClick={refreshWindows}><RefreshCw size={14} />Odśwież okna</button>
          {live.running ? <button onClick={() => live.setRunning(false)}><Square size={14} />Zatrzymaj odczyt</button> : <button className="primary" disabled={handle === null} onClick={() => live.setRunning(true)}><Play size={14} />Czytaj panel broni (2 Hz)</button>}</div>
      </div>
      {live.error && <p className="notice" role="alert">{live.error}</p>}
      <div className="map-readout live-readout">
        <div><small>BROŃ / AMUNICJA</small><b>{weaponName ?? '—'}</b><span>{shown?.ammo ? (shown.ammo.catalogMatch ?? shown.ammo.text) : 'brak odczytu amunicji'}</span></div>
        <div><small>ELEWACJA W GRZE</small><b>{angle(shown?.elevationDeg?.value)}</b><span>{shown?.elevationDeg ? `pewność ${Math.round(shown.elevationDeg.confidence * 100)}%` : '—'}</span></div>
        <div><small>AZYMUT W GRZE</small><b>{angle(shown?.azimuthDeg?.value)}</b><span>{shown?.azimuthDeg ? `pewność ${Math.round(shown.azimuthDeg.confidence * 100)}%` : '—'}</span></div>
        <div><small>STATUS</small><b>{!panel ? '—' : !fresh ? 'NIEAKTUALNY' : panel.status === 'ready' ? 'GOTOWY' : panel.status === 'partial' ? 'CZĘŚCIOWY' : 'BRAK PANELU'}</b><span>{panel && !fresh ? 'odczyt starszy niż 2 s ukryty' : shown?.rangeText ?? ''}</span></div>
      </div>
      {shown && shown.warnings.length > 0 && <ul className="assumptions">{shown.warnings.map(w => <li key={w}>{w}</li>)}</ul>}
      {suggested && suggested.id !== s.selected && <div className="button-group"><button className="primary" onClick={() => update({ selected: suggested.id })}>Użyj profilu z gry: {suggested.name}{equivalents.length ? ` (równoważne: ${equivalents.join(', ')})` : ''}</button></div>}
      {suggested && suggested.id === s.selected && equivalents.length > 0 && <p className="hint">Profil z gry: {suggested.name} (równoważne: {equivalents.join(', ')}).</p>}
      {resolution?.status === 'ambiguous' && <div className="button-group">{resolution.candidates.slice(0, 6).map(c => <button key={c.profileId} onClick={() => update({ selected: c.profileId })}>{profiles.find(p => p.id === c.profileId)?.name ?? c.profileId}</button>)}</div>}
      {shown && weaponName && (!resolution || resolution.status === 'no_match') && <p className="hint warning-inline"><AlertTriangle size={13} />Brak profilu w katalogu dla „{weaponName}” — wybierz ręcznie lub skalibruj.</p>}
      <label className="check-field"><input type="checkbox" checked={autoProfile} onChange={e => setAutoProfile(e.target.checked)} />Automatycznie przełączaj na jednoznaczny profil z gry</label>
    </section>


    {!live.running && aimCard}
    <section className="panel">
      <div className="panel-heading"><div><h2>Mapa z gry</h2><p>Otwórz pełną mapę w grze (klawisz M), potem kliknij przycisk.</p></div><MapIcon size={20} /></div>
      <div className="button-group"><button className="primary" disabled={mapBusy || handle === null} onClick={() => void recognize()}>{mapBusy ? 'Odczyt mapy…' : 'Rozpoznaj mapę z gry (otwórz mapę klawiszem M)'}</button>
        {!s.catalogSourceAccepted && <button onClick={() => update({ catalogSourceAccepted: true })}>Akceptuję źródło map społeczności</button>}</div>
      {mapMessage && <p className="catalog-status" role="status">{mapMessage}</p>}
      {gameMap?.reading.status === 'ambiguous' && <div className="button-group">{gameMap.reading.candidates.map(c => <button key={c.id} className={gameMap.chosenId === c.id ? 'active' : ''} onClick={() => { setGameMap({ ...gameMap, chosenId: c.id }); void loadKnown(c.id); }}>{c.name} · pole {Math.round(c.cellMeters)} m</button>)}</div>}
      {gameMap && gameMap.reading.warnings.length > 0 && <ul className="assumptions">{gameMap.reading.warnings.map(w => <li key={w}>{w}</li>)}</ul>}
      {gameMap?.reading.grid && <>
        <div className="map-tools"><button className={clickRole === 'own' ? 'active' : ''} onClick={() => setClickRole('own')}>Działo</button><button className={clickRole === 'target' ? 'active' : ''} onClick={() => setClickRole('target')}>Cel</button>
          <span>{clickable ? 'Kliknij na klatce mapy z gry — punkt trafi na mapę z katalogu.' : 'Najpierw wczytaj rozpoznaną mapę z katalogu.'}</span></div>
        <svg ref={svg} className="tactical-map game-frame" viewBox={`0 0 ${gameMap.width} ${gameMap.height}`} style={{ aspectRatio: `${gameMap.width} / ${gameMap.height}`, width: `min(100%, calc(70vh * ${gameMap.width / gameMap.height}))` }} onPointerDown={e => { if (e.button === 0) void clickMap(e); }} role="img" aria-label="Klatka mapy z gry; kliknij, aby wskazać punkt">
          <image href={gameMap.preview} x={0} y={0} width={gameMap.width} height={gameMap.height} preserveAspectRatio="none" />
          <rect x={gameMap.reading.grid.left} y={gameMap.reading.grid.top} width={gameMap.reading.grid.right - gameMap.reading.grid.left} height={gameMap.reading.grid.bottom - gameMap.reading.grid.top} fill="none" stroke="#e6af46" strokeWidth={gameMap.width / 500} strokeDasharray={`${gameMap.width / 120} ${gameMap.width / 200}`} pointerEvents="none" />
          {(['own', 'target'] as const).map(role => clicks[role] && <g key={role} pointerEvents="none"><circle cx={clicks[role]!.x} cy={clicks[role]!.y} r={gameMap.width / 150} fill="none" stroke={role === 'own' ? '#67c7c1' : '#e6af46'} strokeWidth={gameMap.width / 600} /><text x={clicks[role]!.x + gameMap.width / 120} y={clicks[role]!.y - gameMap.width / 150} fontSize={gameMap.width / 70} fill={role === 'own' ? '#67c7c1' : '#e6af46'} className="map-marker-label">{role === 'own' ? 'DZIAŁO' : 'CEL'} {clicks[role]!.sector}</text></g>)}
        </svg>
        <p className="hint">Klatka i siatka pochodzą z dwóch szybko po sobie wykonanych zrzutów — nie ruszaj mapy w grze między odczytem a klikaniem.</p>
      </>}
    </section>
  </>;
}
