import { useEffect, useId, useRef, useState, type FormEvent, type PointerEvent } from 'react';
import { RefreshCw } from 'lucide-react';
import { call } from './api';
import type { Point } from './types';
import type { CalibratedVisibleMarker, MarkerPickerProps, MarkerSetupPreview, SelectedMarkerRole } from './marker-types';
import { markerPreviewIsFresh, markerPreviewMatchesImage, maxNormalizedCoordinate, normalizedPreviewPoint, validMarkerPreviewGeometry } from './marker-preview';

const roleName = (role: SelectedMarkerRole) => role === 'own' ? 'mój marker' : 'wybrany cel';
const isImageData = (value: string) => /^data:image\/(png|jpeg);base64,/.test(value);
const validPoint = (point: Point) => Number.isFinite(point.x) && Number.isFinite(point.y)
  && point.x >= 0 && point.x <= 1 && point.y >= 0 && point.y <= 1;

export function MarkerPicker({ handle, backend, config, autoMap, mapId, scope, selections, onSelect, notice }: MarkerPickerProps) {
  const id = useId();
  const [role, setRole] = useState<SelectedMarkerRole>('own');
  const [snapshot, setSnapshot] = useState<{ sourceKey: string; data: MarkerSetupPreview } | null>(null);
  const [imageReady, setImageReady] = useState(false);
  const [busy, setBusy] = useState(false);
  const [coordinates, setCoordinates] = useState({ x: '', y: '' });
  const [message, setMessage] = useState('');
  const [, setTick] = useState(0);
  const generation = useRef(0);
  const active = useRef(true);
  const press = useRef<{ pointerId: number; x: number; y: number; token: string } | null>(null);
  const currentSource = useRef('');
  const currentConfig = useRef(config);

  const markerConfig = config.markerConfig;
  // Signatures, marker-enabled state and acceptance flags can change after the
  // first selection. They do not alter the displayed source geometry.
  const manualGeometry = autoMap ? null : [markerConfig?.mapRoi.x, markerConfig?.mapRoi.y,
    markerConfig?.mapRoi.width, markerConfig?.mapRoi.height, markerConfig?.mapWidth, markerConfig?.mapHeight];
  const sourceKey = JSON.stringify([handle, backend, autoMap, mapId, scope, manualGeometry]);
  currentSource.current = sourceKey;
  currentConfig.current = config;
  const dimensionsMatch = (data: MarkerSetupPreview) => {
    const current = currentConfig.current;
    return (current.referenceWidth === 0 || current.referenceWidth === data.width)
      && (current.referenceHeight === 0 || current.referenceHeight === data.height);
  };
  const setup = snapshot?.sourceKey === sourceKey && dimensionsMatch(snapshot.data) ? snapshot.data : null;
  // Re-read the clock during each render; a received frame can be newer than
  // the previous timer tick. Never alter the native source timestamp.
  const nowMs = Date.now();
  const fresh = !!setup && markerPreviewIsFresh(setup.timestampMs, nowMs);
  const canSelect = !!handle && !!setup && fresh && imageReady && !busy;
  const ageMs = setup ? nowMs - setup.timestampMs : null;
  const compatibleSelections = selections.filter(selection => selection.handle === handle && selection.backend === backend
    && (mapId === null || selection.mapId === mapId));

  useEffect(() => {
    active.current = true;
    return () => { active.current = false; generation.current += 1; };
  }, []);
  useEffect(() => {
    generation.current += 1;
    setSnapshot(null);
    setImageReady(false);
    setBusy(false);
    setMessage('');
    setCoordinates({ x: '', y: '' });
    setRole('own');
    press.current = null;
  }, [sourceKey]);
  useEffect(() => {
    if (!snapshot) return;
    const timer = setInterval(() => setTick(previous => previous + 1), 100);
    return () => clearInterval(timer);
  }, [snapshot?.data.token]);
  useEffect(() => {
    if (snapshot && !dimensionsMatch(snapshot.data)) {
      generation.current += 1;
      setSnapshot(null);
      setImageReady(false);
      setBusy(false);
      setMessage('Zmieniono rozmiar źródła. Pobierz nowy podgląd.');
    }
  }, [config.referenceWidth, config.referenceHeight, snapshot]);
  const reportError = (error: unknown) => {
    const text = String(error);
    setMessage(text);
    notice(text);
  };
  const refresh = async () => {
    if (!handle || busy) return;
    const request = ++generation.current;
    const source = sourceKey;
    setBusy(true);
    setSnapshot(null);
    setImageReady(false);
    setMessage('');
    try {
      const data = await call<MarkerSetupPreview>('capture_marker_setup', { handle, backend, config, autoMap, mapId, scope });
      if (!active.current || request !== generation.current || source !== currentSource.current) return;
      if (!data.token || !isImageData(data.preview) || !validMarkerPreviewGeometry(data)
        || !dimensionsMatch(data)) throw new Error('Podgląd nie odpowiada geometrii wybranego źródła.');
      setSnapshot({ sourceKey: source, data });
      if (!markerPreviewIsFresh(data.timestampMs, Date.now())) setMessage('Podgląd jest już nieaktualny. Odśwież go przed wyborem.');
    } catch (error) {
      if (active.current && request === generation.current && source === currentSource.current) reportError(error);
    } finally {
      if (active.current && request === generation.current) setBusy(false);
    }
  };

  const select = async (point: Point) => {
    if (!setup || busy || !imageReady || !handle) return;
    if (!markerPreviewIsFresh(setup.timestampMs, Date.now())) {
      reportError('Podgląd jest nieaktualny. Odśwież go i ponownie wskaż marker.');
      return;
    }
    if (!validPoint(point)) { reportError('Wskaż punkt wewnątrz podglądu.'); return; }
    const request = ++generation.current;
    const source = sourceKey;
    const selectedRole = role;
    setBusy(true);
    setMessage('');
    setCoordinates({ x: (point.x * 100).toFixed(3), y: (point.y * 100).toFixed(3) });
    try {
      const result = await call<CalibratedVisibleMarker>('calibrate_visible_marker', { token: setup.token, click: point, role: selectedRole });
      if (!active.current || request !== generation.current || source !== currentSource.current) return;
      if (result.selection.handle !== handle || result.selection.backend !== backend || result.selection.hint.role !== selectedRole
        || result.signature.role !== 'unassigned' || result.width !== setup.width || result.height !== setup.height
        || result.previewWidth !== setup.previewWidth || result.previewHeight !== setup.previewHeight
        || result.timestampMs !== setup.timestampMs || !isImageData(result.preview)) throw new Error('Wynik wyboru pochodzi z innego źródła lub innej klatki.');
      onSelect(result.signature, result.selection, result.preview);
      setMessage(selectedRole === 'own' ? 'Wybrano mój marker. Odśwież podgląd i wskaż cel.' : 'Wybrano cel. Odczyt pozycji wymaga kolejnych świeżych klatek.');
      if (selectedRole === 'own') setRole('target');
    } catch (error) {
      if (active.current && request === generation.current && source === currentSource.current) reportError(error);
    } finally {
      if (active.current && request === generation.current) setBusy(false);
    }
  };
  const pointer = (event: PointerEvent<HTMLImageElement>) => {
    const start = press.current;
    press.current = null;
    if (event.button !== 0 || !event.isPrimary || !canSelect || !start || start.pointerId !== event.pointerId
      || start.token !== setup?.token || Math.hypot(start.x - event.clientX, start.y - event.clientY) > 6) return;
    const bounds = event.currentTarget.getBoundingClientRect();
    const point = normalizedPreviewPoint(event.clientX, event.clientY, bounds);
    if (point) void select(point);
  };
  const selectCoordinates = (event: FormEvent) => {
    event.preventDefault();
    if (!coordinates.x.trim() || !coordinates.y.trim()) { reportError('Podaj obie współrzędne punktu.'); return; }
    // 100% is the far border; it maps to the last source pixel like a pointer click.
    const toNormalized = (value: string) => Math.min(Number(value) / 100, maxNormalizedCoordinate);
    void select({ x: toNormalized(coordinates.x), y: toNormalized(coordinates.y) });
  };

  return <section className="panel marker-picker">
    <div className="panel-heading"><div><h2>Wskaż widoczne markery</h2><p>Wybierz rolę i kliknij jej marker na aktualnym podglądzie. Rolę potwierdzasz kliknięciem.</p></div></div>
    <div className="form-grid">
      <label className="field" htmlFor={`${id}-role`}><span>Rola wybranego markera</span><select id={`${id}-role`} value={role} disabled={busy} onChange={event => setRole(event.target.value as SelectedMarkerRole)}>
        <option value="own">Mój marker</option><option value="target">Wybrany cel</option>
      </select></label>
      <button type="button" className="primary align-end" disabled={!handle || busy} onClick={() => void refresh()}><RefreshCw size={16} aria-hidden="true"/>{busy ? 'Przetwarzanie…' : 'Odśwież podgląd markerów'}</button>
    </div>
    <ul className="hint">{(['own', 'target'] as const).map(item => {
      const selection = compatibleSelections.find(selected => selected.hint.role === item);
      return <li key={item}>{item === 'own' ? 'Mój marker' : 'Wybrany cel'}: {selection ? `wybrany (${selection.hint.confirmedPoint.x.toFixed(1)}, ${selection.hint.confirmedPoint.y.toFixed(1)} px mapy)` : 'oczekuje na wskazanie'}</li>;
    })}</ul>
    {!handle && <p className="hint">Wybierz okno w analizie obrazu, aby pobrać podgląd.</p>}
    {setup && <figure style={{ margin: '16px 0' }}>
      <div style={{ position: 'relative', border: '1px solid #61728b', borderRadius: 6, overflow: 'hidden' }}>
        <img key={setup.token} src={setup.preview} draggable={false} alt="Aktualny podgląd wybranego okna do wskazania markera; punkt można też podać w polach poniżej"
          onPointerDown={event => { if (event.button === 0 && event.isPrimary && canSelect) press.current = { pointerId: event.pointerId, x: event.clientX, y: event.clientY, token: setup.token }; }}
          onPointerUp={pointer} onPointerCancel={() => { press.current = null; }} onDragStart={event => event.preventDefault()}
          onLoad={event => {
            if (snapshot?.data.token !== setup.token) return;
            if (!markerPreviewMatchesImage(setup, event.currentTarget.naturalWidth, event.currentTarget.naturalHeight)) {
              setImageReady(false); reportError('Wymiary wyświetlonego obrazu nie pasują do klatki. Odśwież podgląd.');
            } else setImageReady(true);
          }}
          onError={() => { setImageReady(false); reportError('Nie udało się wyświetlić podglądu. Odśwież klatkę.'); }}
          style={{ display: 'block', width: '100%', height: 'auto', cursor: canSelect ? 'crosshair' : 'default' }}/>
        <svg aria-hidden="true" viewBox="0 0 1000 1000" preserveAspectRatio="none" style={{ position: 'absolute', inset: 0, width: '100%', height: '100%', pointerEvents: 'none' }}>
          <rect x={setup.mapRoi.x * 1000} y={setup.mapRoi.y * 1000} width={setup.mapRoi.width * 1000} height={setup.mapRoi.height * 1000}
            fill="none" stroke="#ffffff" strokeDasharray="6 4" vectorEffect="non-scaling-stroke"/>
          {setup.proposals.filter(proposal => validPoint(proposal.framePositionNormalized)).map(proposal => <circle key={proposal.id}
            cx={proposal.framePositionNormalized.x * 1000} cy={proposal.framePositionNormalized.y * 1000} r={7}
            fill={`rgb(${proposal.signature.rgb.join(',')})`} stroke="#fff" strokeWidth={2} vectorEffect="non-scaling-stroke"/>) }
        </svg>
      </div>
      <figcaption className="hint" id={`${id}-image-status`}>
        {setup.width}×{setup.height} px · {ageMs !== null && ageMs >= 0 ? `wiek klatki ${(ageMs / 1000).toFixed(1)} s` : 'oczekiwanie na czas źródła'} · {fresh ? `wskaż ${roleName(role)}` : 'podgląd nieaktualny — odśwież'}
      </figcaption>
      {!fresh && <p className="hint">Wybór jest dostępny przez 2 sekundy od czasu źródłowej klatki.</p>}
    </figure>}
    <form onSubmit={selectCoordinates} aria-describedby={`${id}-coordinate-hint`}>
      <p className="hint" id={`${id}-coordinate-hint`}>Możesz podać punkt klawiaturą: X i Y w procentach całego podglądu, od lewego górnego narożnika.</p>
      <div className="form-grid">
        {(['x', 'y'] as const).map(axis => <label className="field" key={axis} htmlFor={`${id}-${axis}`}><span>{axis.toUpperCase()} na podglądzie (%)</span>
          <input id={`${id}-${axis}`} type="number" min={0} max={100} step="any" value={coordinates[axis]} disabled={busy}
            onChange={event => setCoordinates(previous => ({ ...previous, [axis]: event.target.value }))}/></label>)}
        <button className="align-end" type="submit" disabled={!canSelect}>Wskaż {roleName(role)} w podanym punkcie</button>
      </div>
    </form>
    <p className="hint" role="status">{message}</p>
    {setup?.warnings.map((warning, index) => <p className="hint" key={index}>{warning}</p>)}
  </section>;
}
