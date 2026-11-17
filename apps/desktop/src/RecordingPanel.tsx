import { useEffect, useId, useRef, useState } from 'react';
import { Circle, FolderOpen, Square } from 'lucide-react';
import { call, native } from './api';
import type { VisionConfig } from './types';
import type { LoadedRecording, RecordingAnalysis, RecordingBackend, RecordingOptions, RecordingStatus } from './recording-types';
import { buildRecordingRequest, defaultRecordingOptions, formatBytes, formatDuration, recordingOptionsError, recordingProgress } from './recording';
import './vision.css';

type Props = {
  handle: number | null;
  windowTitle: string | null;
  backend: RecordingBackend;
  config: VisionConfig;
  /** Optional: open one recorded frame in the regular batch analysis view. */
  analyseFrame?: (path: string) => void;
};

const statusLabels: Record<string, string> = { observed: 'odczyt', ambiguous: 'niejednoznaczny', empty: 'pusty', error: 'błąd', blocked: 'zablokowany' };

export function RecordingPanel({ handle, windowTitle, backend, config, analyseFrame }: Props) {
  const id = useId();
  const [options, setOptions] = useState<RecordingOptions>(defaultRecordingOptions);
  // Consent lives only in component state: off by default, reset after each
  // recording and never written to settings.
  const [consent, setConsent] = useState(false);
  const [directory, setDirectory] = useState<string | null>(null);
  const [status, setStatus] = useState<RecordingStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState('');
  const [loaded, setLoaded] = useState<LoadedRecording | null>(null);
  const [useRecordedRois, setUseRecordedRois] = useState(true);
  const [replayStart, setReplayStart] = useState(1);
  const [replayCount, setReplayCount] = useState(100);
  const [replay, setReplay] = useState<RecordingAnalysis | null>(null);
  const [frameIndex, setFrameIndex] = useState(1);
  const active = status?.active === true;
  const activeRef = useRef(false);
  activeRef.current = active;
  const mounted = useRef(true);
  const starting = useRef(false);

  const stop = async () => {
    try {
      const final = await call<RecordingStatus>('recording_stop');
      setStatus(final);
    } catch (error) {
      setMessage(String(error));
    } finally {
      setConsent(false);
    }
  };

  useEffect(() => {
    if (!native) return;
    mounted.current = true;
    void call<RecordingStatus>('recording_status').then(setStatus).catch(() => {});
    // Leaving the panel ends the recording — also one whose start is still pending
    // (the pending start stops it itself once it returns, see `start`).
    return () => {
      mounted.current = false;
      if (activeRef.current || starting.current) void call('recording_stop').catch(() => {});
    };
  }, []);

  useEffect(() => {
    if (!active) return;
    const timer = setInterval(() => {
      void call<RecordingStatus>('recording_status').then(next => {
        setStatus(next);
        if (!next.active) setConsent(false);
      }).catch(error => setMessage(String(error)));
    }, 500);
    return () => clearInterval(timer);
  }, [active]);

  useEffect(() => {
    // Another window or backend means another source: stop instead of mixing.
    if (activeRef.current) void stop();
    setConsent(false);
  }, [handle, backend]);

  const chooseDirectory = async () => {
    try {
      if (!native) throw new Error('Nagrywanie wymaga wersji .exe');
      const { open } = await import('@tauri-apps/plugin-dialog');
      const selected = await open({ directory: true, multiple: false, title: 'Katalog na nagrania testowe' });
      if (typeof selected === 'string') { setDirectory(selected); setConsent(false); }
    } catch (error) { setMessage(String(error)); }
  };

  const start = async () => {
    setMessage('');
    try {
      const request = buildRecordingRequest(options, consent, directory, handle, backend, config);
      setBusy(true);
      starting.current = true;
      const started = await call<RecordingStatus>('recording_start', { request });
      starting.current = false;
      if (!mounted.current) { void call('recording_stop').catch(() => {}); return; }
      setStatus(started);
    } catch (error) {
      starting.current = false;
      if (!mounted.current) return;
      setMessage(error instanceof Error ? error.message : String(error));
      setConsent(false);
    } finally {
      if (mounted.current) setBusy(false);
    }
  };

  const loadRecording = async () => {
    setMessage('');
    try {
      if (!native) throw new Error('Wczytanie nagrania wymaga wersji .exe');
      const { open } = await import('@tauri-apps/plugin-dialog');
      const selected = await open({ multiple: false, filters: [{ name: 'Nagranie MTC (session.json)', extensions: ['json'] }] });
      if (typeof selected !== 'string') return;
      const data = await call<LoadedRecording>('recording_load', { path: selected });
      setLoaded(data);
      setReplay(null);
      setReplayStart(1);
      setFrameIndex(1);
      setUseRecordedRois(data.session.analysisConfig !== null);
    } catch (error) { setMessage(String(error)); }
  };

  const analyseLoaded = async () => {
    if (!loaded) return;
    setBusy(true);
    setMessage('');
    try {
      const result = await call<RecordingAnalysis>('analyse_recording', {
        path: loaded.directory,
        config: useRecordedRois ? null : config,
        start: Math.max(0, Math.round(replayStart) - 1),
        count: Math.max(1, Math.min(500, Math.round(replayCount))),
      });
      setReplay(result);
    } catch (error) { setMessage(String(error)); } finally { setBusy(false); }
  };

  const optionsError = recordingOptionsError(options);
  const canStart = native && !active && !busy && consent && !!directory && handle !== null && !optionsError;
  const setNumber = (key: keyof RecordingOptions) => (value: number) => setOptions(previous => ({ ...previous, [key]: value }));
  const selectedFrame = loaded?.frames.find(frame => frame.index === frameIndex) ?? null;

  return <section className="recording-section">
    {active && status && <>
      <div className="recording-badge" aria-hidden="true"><span className="recording-dot"/>NAGRYWANIE</div>
      <div className="recording-indicator" role="status" aria-live="polite">
        <span className="recording-dot" aria-hidden="true"/><strong>NAGRYWANIE</strong>
        <span>{status.frames} kl. · {formatBytes(status.bytes)} · {formatDuration(status.elapsedMs)}{status.skipped ? ` · pominięte ${status.skipped}` : ''}</span>
        <progress max={1} value={recordingProgress(status)} aria-label="Wykorzystanie limitów nagrania"/>
        <button className="danger" onClick={() => void stop()}><Square size={14} aria-hidden="true"/>Zatrzymaj</button>
      </div>
    </>}
    <details className="recording-panel">
    <summary>Nagrywanie testowe klatek (fixtures do analizy offline)</summary>
    <p className="hint">Zapis klatek wybranego okna (sam obszar klienta) do lokalnego katalogu. Domyślnie wyłączone. Zgoda dotyczy tylko jednego nagrania i nie jest zapamiętywana. Pliki nie są wysyłane. Przejście do innej zakładki, zmiana okna lub zamknięcie okna gry kończy nagrywanie.</p>
    <div className="recording-grid">
      <label className="field"><span>Katalog nagrań</span><input readOnly value={directory ?? ''} placeholder="Nie wybrano"/></label>
      <button className="align-end" disabled={active || busy} onClick={() => void chooseDirectory()}><FolderOpen size={15} aria-hidden="true"/>Wybierz katalog…</button>
      <label className="field"><span>Format</span><select value={options.format} disabled={active} onChange={e => setOptions({ ...options, format: e.target.value as RecordingOptions['format'] })}><option value="png">PNG (bezstratny, zalecany do OCR)</option><option value="jpeg">JPEG (mniejszy)</option></select></label>
      {options.format === 'jpeg' && <label className="field"><span>Jakość JPEG (40–100)</span><input type="number" min={40} max={100} step={1} disabled={active} value={options.jpegQuality} onChange={e => setNumber('jpegQuality')(e.target.valueAsNumber)}/></label>}
      <label className="field"><span>Limit FPS (0,1–5)</span><input type="number" min={0.1} max={5} step={0.1} disabled={active} value={options.maxFps} onChange={e => setNumber('maxFps')(e.target.valueAsNumber)}/></label>
      <label className="field"><span>Maks. klatek</span><input type="number" min={1} max={10000} step={1} disabled={active} value={options.maxFrames} onChange={e => setNumber('maxFrames')(e.target.valueAsNumber)}/></label>
      <label className="field"><span>Maks. rozmiar (MB)</span><input type="number" min={1} max={8192} step={1} disabled={active} value={options.maxMegabytes} onChange={e => setNumber('maxMegabytes')(e.target.valueAsNumber)}/></label>
      <label className="field"><span>Maks. czas (s)</span><input type="number" min={1} max={3600} step={1} disabled={active} value={options.maxSeconds} onChange={e => setNumber('maxSeconds')(e.target.valueAsNumber)}/></label>
    </div>
    {optionsError && <p className="hint recording-error">{optionsError}</p>}
    <label className="check-field" htmlFor={`${id}-consent`}><input id={`${id}-consent`} type="checkbox" checked={consent} disabled={active || busy} onChange={e => setConsent(e.target.checked)}/>
      Zgadzam się na zapis klatek okna {windowTitle ? `„${windowTitle}”` : '(nie wybrano)'} do wskazanego katalogu na tym komputerze, w ramach powyższych limitów.</label>
    <div className="button-group">
      {active
        ? <button className="danger" onClick={() => void stop()}><Square size={15} aria-hidden="true"/>Zatrzymaj nagrywanie</button>
        : <button className="primary" disabled={!canStart} onClick={() => void start()}><Circle size={15} aria-hidden="true"/>Rozpocznij nagrywanie</button>}
    </div>
    {status && !active && status.stopReason && <p className="hint">Ostatnie nagranie: {status.frames} kl., {formatBytes(status.bytes)} · {status.stopReason}{status.directory ? ` · ${status.directory}` : ''}</p>}
    {status?.warning && <p className="hint">{status.warning}</p>}
    {status?.error && <p className="hint recording-error">{status.error}</p>}

    <h4>Analiza nagrania offline</h4>
    <p className="hint">Wczytaj <code>session.json</code> nagrania. Każda klatka jest analizowana w swoim zapisanym czasie źródłowym; kolejne klatki są stabilizowane jak na żywo.</p>
    <div className="button-group"><button disabled={busy} onClick={() => void loadRecording()}>Wczytaj nagranie…</button></div>
    {loaded && <>
      <p className="hint">{loaded.frames.length} klatek · {loaded.session.format.toUpperCase()} · {formatBytes(loaded.session.totalBytes)} · źródło: {loaded.session.source.windowTitle ?? '—'} ({loaded.session.source.backend}){loaded.session.completed ? '' : ' · nagranie niezakończone'}</p>
      <div className="recording-grid">
        <label className="check-field"><input type="checkbox" checked={useRecordedRois} disabled={!loaded.session.analysisConfig} onChange={e => setUseRecordedRois(e.target.checked)}/>Użyj ROI zapisanych w nagraniu{loaded.session.analysisConfig ? '' : ' (brak w nagraniu — użyta bieżąca konfiguracja)'}</label>
        <label className="field"><span>Od klatki</span><input type="number" min={1} max={Math.max(1, loaded.frames.length)} step={1} value={replayStart} onChange={e => Number.isFinite(e.target.valueAsNumber) && setReplayStart(e.target.valueAsNumber)}/></label>
        <label className="field"><span>Liczba klatek (≤ 500)</span><input type="number" min={1} max={500} step={1} value={replayCount} onChange={e => Number.isFinite(e.target.valueAsNumber) && setReplayCount(e.target.valueAsNumber)}/></label>
        <button className="primary align-end" disabled={busy || loaded.frames.length === 0} onClick={() => void analyseLoaded()}>{busy ? 'Analiza…' : 'Analizuj nagranie'}</button>
      </div>
      {analyseFrame && loaded.frames.length > 0 && <div className="recording-grid">
        <label className="field"><span>Klatka do pełnej analizy batch</span><input type="number" min={1} max={loaded.frames.length} step={1} value={frameIndex} onChange={e => Number.isFinite(e.target.valueAsNumber) && setFrameIndex(Math.round(e.target.valueAsNumber))}/></label>
        <button className="align-end" disabled={!selectedFrame} onClick={() => selectedFrame && analyseFrame(selectedFrame.path)}>Otwórz klatkę w analizie batch</button>
      </div>}
    </>}
    {replay && <div className="recording-replay">
      <p className="hint">Przeanalizowano {replay.frames.length} z {replay.totalFrames} klatek w {(replay.elapsedMs / 1000).toFixed(1)} s.</p>
      {replay.warnings.map((warning, index) => <p className="hint" key={index}>{warning}</p>)}
      <table>
        <thead><tr><th>Obszar</th><th>Odczyty</th><th>Stabilne (≥85%)</th><th>Niejednoznaczne</th><th>Puste/błędy/blokady</th><th>Ostatni tekst</th></tr></thead>
        <tbody>{replay.regions.map(region => <tr key={region.id}>
          <td>{region.id}</td><td>{region.observed}/{region.frames}</td><td>{region.stable}</td><td>{region.ambiguous}</td>
          <td>{region.empty}/{region.errors}/{region.blocked}</td><td>{region.lastText ?? '—'}</td></tr>)}</tbody>
      </table>
      <details><summary>Wyniki poszczególnych klatek</summary><ol className="recording-frames">{replay.frames.map(frame => <li key={frame.index} value={frame.index}>
        {frame.file}: {frame.error ?? frame.analysis?.ocrRegions.map(region => `${region.id} ${statusLabels[region.status] ?? region.status}${region.observation ? ` ${region.observation.value} (${Math.round(region.observation.confidence * 100)}%)` : region.text ? ` „${region.text}”` : ''}`).join(' · ')}
      </li>)}</ol></details>
    </div>}
    {message && <p className="hint recording-error" role="alert">{message}</p>}
    </details>
  </section>;
}
