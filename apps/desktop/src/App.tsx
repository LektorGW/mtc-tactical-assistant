import { useCallback, useEffect, useRef, useState, type FormEvent } from 'react';
import { listen } from '@tauri-apps/api/event';
import { Crosshair, Radar, Map as MapIcon, ScanLine, Navigation, Settings2, Rocket, Monitor, AlertTriangle, Check, Gamepad2, Target } from 'lucide-react';
import { call, native, pickImage, pickJson, saveJson, profileFor } from './api';
import { defaultSettings, type Settings, type Profile, type Result, type Observation, type Analysis, type BatchAnalysis, type CommunityMap, type VisionConfig, type MapViewSignature, type AssetsStatus, type StoredMap } from './types';
import { fresh, freshBatch, stabilize, validateSettings } from './validation';
import { VisionPanel } from './VisionPanel';
import { AutomationPanel } from './AutomationPanel';
import type { CalibrationFit } from './calibration';
import { DynamicsPanel } from './DynamicsPanel';
import { DynamicInterceptPanel, SalvoPanel } from './SalvoPanel';
import type { RecordingStatus } from './recording-types';
import { MapResourcesPanel } from './MapResourcesPanel';
import type { ResourceLibrary } from './resource-types';
import { MarkerPicker } from './MarkerPicker';
import type { NativeMarkerSelection } from './marker-types';
import { MapPanel } from './MapPanel';
import { normalizeMapSettings } from './map-settings';
import { isCurrentTerrain, sameScale } from './map-catalog';
import { currentSalvo, salvoScopeKey, type SalvoForecast } from './map-salvo';
import { invalidateCatalogLoads, loadCatalogMap } from './useCatalogMap';
import { useLiveWeaponPanel } from './useLiveWeaponPanel';
import { aimCorrection, panelFresh, requiredAimFrom } from './aim-correction';
import { LiveGamePanel } from './LiveGamePanel';
import { endSimulation, simulationActive, SIMULATION_MAP_NAME } from './simulation';
import { emptyTracker, holdPositions, respawnGap, trackAutoMap, type AutoMapDecision, type HeldPositions } from './auto-map';
import { isCatalogEntry } from './map-catalog';
import { useMapPointHotkeys } from './useMapPointHotkeys';
import { useOwnPosition } from './useOwnPosition';
import { OwnPositionToggle } from './OwnPositionToggle';
import { isCaptureBusy, nextScanDelay } from './vision-utils';
import { sectorOf, worldDelta } from './map-geometry';
import { DirectFirePanel, type OverlayDirectCard } from './DirectFirePanel';
import { useQuickAim } from './useQuickAim';
function NumberField({ label, value, onChange, min, max, step = 'any' }: { label: string; value: number | null; onChange: (n: number | null) => void; min?: number; max?: number; step?: string }) {
  return <label className="field"><span>{label}</span><input type="number" value={value ?? ''} min={min} max={max} step={step} onChange={e => onChange(e.target.value === '' ? null : e.target.valueAsNumber)} /></label>;
}
const format = (n: number | undefined, digits = 1) => n === undefined ? '—' : n.toFixed(digits);
const iso = () => new Date().toISOString();

export function App() {
  const [s, setS] = useState<Settings>(defaultSettings);
  const settingsRef=useRef(s);settingsRef.current=s;
  const [loaded, setLoaded] = useState(false);
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [communityMaps, setCommunityMaps] = useState<CommunityMap[]>([]);
  const [profileSearch, setProfileSearch] = useState('');
  const [rawResult, setResult] = useState<Result | null>(null);
  const [warning, setWarning] = useState('Wybierz mapę z katalogu albo zaimportuj własny obraz.');
  const [notice, setNotice] = useState('');
  const [tab, setTab] = useState<'map' | 'aim' | 'vision' | 'weapons' | 'direct' | 'log' | 'automation'>('map');
  const [mapImage, setMapImage] = useState<string | null>(null);
  const [mapFingerprint,setMapFingerprint]=useState<string|null>(null);
  const [height, setHeight] = useState<number | null>(null);
  const [velocityX, setVelocityX] = useState<number | null>(null);
  const [velocityY, setVelocityY] = useState<number | null>(null);
  const [overlayVisible, setOverlayVisible] = useState(false);
  const [clickThrough, setClickThrough] = useState(false);
  const [windows, setWindows] = useState<{ handle: number; title: string }[]>([]);
  const [windowHandle, setWindowHandle] = useState<number | null>(null);
  const [running, setRunning] = useState(false);
  const [batch, setBatch] = useState<BatchAnalysis | null>(null);
  const [batchScope,setBatchScope]=useState('');
  const [capturePreview, setCapturePreview] = useState<string | null>(null);
  const [analysis, setAnalysis] = useState<Analysis | null>(null);
  const [observation, setObservation] = useState<Observation | null>(null);
  const [, setClock] = useState(Date.now());
  const now=Date.now();
  const previousObservation = useRef<Observation | null>(null);
  const [speed, setSpeed] = useState<number | null>(null);
  const [gravity, setGravity] = useState<number | null>(null);
  const [sigma, setSigma] = useState<number | null>(null);
  const [salvo, setSalvo] = useState<number | null>(1);
  const [source, setSource] = useState('');
  const [ownVelocityX, setOwnVelocityX] = useState<number | null>(null);
  const [ownVelocityY, setOwnVelocityY] = useState<number | null>(null);
  const [useOwnVelocity, setUseOwnVelocity] = useState(false);
  const [use3d,setUse3d]=useState(false);
  const [salvoForecast,setSalvoForecast]=useState<SalvoForecast|null>(null);
  /** Settings could not be read: autosave stays off until the original file is backed up. */
  const [loadFailed,setLoadFailed]=useState(false);
  const [preferHigh,setPreferHigh]=useState(false);
  const autoMapTracker=useRef(emptyTracker);
  const [autoMapStatus,setAutoMapStatus]=useState<AutoMapDecision['status']|null>(null);
  const [heldAuto,setHeldAuto]=useState<HeldPositions>({own:null,target:null,timestampMs:0});
  const [ownMaybeStale,setOwnMaybeStale]=useState(false);
  const lastPanelTs=useRef<number|null>(null);
  const [autoProfile,setAutoProfile]=useState(false);
  const [recording,setRecording]=useState<RecordingStatus|null>(null);
  const [verticalVelocity,setVerticalVelocity]=useState<number|null>(null);
  const [ownVerticalVelocity,setOwnVerticalVelocity]=useState<number|null>(null);
  const [heightConfirmed,setHeightConfirmed]=useState(false);
  const [assetsReady,setAssetsReady]=useState(false);
  const [assetsStatus,setAssetsStatus]=useState<AssetsStatus|null>(null);
  const savedError=useRef('');
  const [markerSelections,setMarkerSelections]=useState<NativeMarkerSelection[]>([]);
  const profile = profileFor(profiles, s.selected, s.overrides);
  const simulation = simulationActive(s);
  const training = (profile?.source.startsWith('SYMULACJA') ?? false) || simulation;
  const update = useCallback((patch: Partial<Settings>) => setS(old => ({ ...old, ...patch })), []);
  const log = (event: string) => setS(old => ({ ...old, calibrationLog: [...old.calibrationLog, { at: iso(), event }].slice(-100) }));
  const attempt = async (fn: () => Promise<void>) => { try { setNotice(''); await fn(); } catch (e) { setNotice(String(e)); } };
  const automaticPositions = s.mode==='automatic' && s.markerConfig.enabled;
  const configuredCalibration=s.scale ? {unitsPerPixel:s.scale,northRotationDeg:s.rotation,originPixel:{x:0,y:0},originWorld:{x:0,y:0},uncertaintyUnits:s.uncertainty}:null;
  const visionConfig:VisionConfig = { schemaVersion:1,referenceWidth:s.captureReferenceWidth,referenceHeight:s.captureReferenceHeight,aspectTolerance:0.03,maxAgeMs:2000,ocrRegions:s.hudRegions.filter(r=>r.enabled).map(({enabled:_,...r})=>r),markerConfig:{...s.markerConfig,mapWidth:s.mapWidth,mapHeight:s.mapHeight} };
  const visionConfigKey=JSON.stringify(visionConfig);
  const calibrationKey=JSON.stringify(configuredCalibration);
  const assetsKey=JSON.stringify({maps:s.maps,terrain:s.terrain,terrains:s.terrains});
  const scanKey=JSON.stringify([windowHandle,s.captureBackend,visionConfigKey,calibrationKey,s.fps,s.maxTrackingSpeed,s.autoHud,s.autoMap,s.communityMapId,assetsKey,JSON.stringify(markerSelections)]);
  const liveFresh = freshBatch(batch,batchScope,scanKey,running,now);
  const autoMapReady=!s.autoMap||!!(liveFresh&&batch?.automation?.activeMap);
  const autoWeaponReady=!s.autoWeapon||!!(liveFresh&&batch?.automation?.weapon?.status==='matched'&&batch.automation.weapon.confidence>=0.85&&batch.automation.weapon.profileId===s.selected);
  const result=s.mode==='automatic'&&(!liveFresh||!autoMapReady||!autoWeaponReady)?null:rawResult;
  const ownMarkers = liveFresh ? batch!.analysis.markers.filter(m=>m.role==='own'&&m.confidence>=0.85) : [];
  const targetMarkers = liveFresh ? batch!.analysis.markers.filter(m=>m.role==='target'&&m.confidence>=0.85) : [];
  const effectiveOwn = automaticPositions ? autoMapReady&&ownMarkers.length===1 ? ownMarkers[0].mapPosition : null : s.own;
  const effectiveTarget = automaticPositions ? autoMapReady&&targetMarkers.length===1 ? targetMarkers[0].mapPosition : null : s.target;
  const trackedVelocity = liveFresh && batch?.targetMotion?.status==='ready' && batch.targetMotion.confidence>=0.6 ? batch.targetMotion.velocity : null;
  const trackedOwnVelocity = liveFresh && batch?.ownMotion?.status==='ready' && batch.ownMotion.confidence>=0.6 ? batch.ownMotion.velocity : null;
  const calculationVelocity = automaticPositions ? trackedVelocity : velocityX!==null&&velocityY!==null ? [velocityX,velocityY] as [number,number] : null;
  const calculationOwnVelocity = useOwnVelocity ? automaticPositions ? trackedOwnVelocity : ownVelocityX!==null&&ownVelocityY!==null ? [ownVelocityX,ownVelocityY] as [number,number] : null : null;
  const activeMap=s.autoMap&&liveFresh?batch?.automation?.activeMap:null;
  const calibration = activeMap?.calibration??configuredCalibration;
  const calculationMapId=activeMap?.id??s.maps.find(m=>m.path===s.mapPath&&m.sha256===mapFingerprint&&m.width===s.mapWidth&&m.height===s.mapHeight&&m.unit===s.mapUnit)?.id??null;

  const calculationMap=activeMap??s.maps.find(m=>m.id===calculationMapId)??null;
  const terrainCandidate=s.terrain?.mapId===calculationMapId?s.terrain:s.terrains.find(t=>t.mapId===calculationMapId)??null;
  // Older community terrain stretched non-square heightmaps; it is never used.
  const selectedTerrain=calculationMap&&isCurrentTerrain(terrainCandidate,calculationMap)?terrainCandidate:null;
  const staleTerrain=!!terrainCandidate&&!selectedTerrain;
  // Terrain is bound to one library map: same file, same pixel→world geometry (uncertainty excluded).
  const terrainGeometry=!!calculationMap&&!!calibration&&sameScale(calculationMap.calibration.unitsPerPixel,calibration.unitsPerPixel)&&calculationMap.calibration.northRotationDeg===calibration.northRotationDeg;
  const terrainLoaded=assetsReady&&!!selectedTerrain&&!!assetsStatus?.terrains.includes(selectedTerrain.mapId);
  const terrainActive=s.useTerrainHeight&&terrainLoaded&&terrainGeometry&&selectedTerrain!.unit===(activeMap?.unit??s.mapUnit);
  const sourceAccepted=s.catalogSourceAccepted||Object.values(s.overrides).some(p=>p.sourceMetadata&&p.sourceAcknowledged);
  // Height actually used by the last calculation (heightmap or manual), never a default 0.
  const usedHeight = result?.heightDelta ?? null;
  const salvoScope = salvoScopeKey({ profileId: profile?.id ?? null, profileSalvo: profile?.salvo, profileSpeed: profile?.speed ?? null, profileGravity: profile?.gravity ?? null, salvoSize: profile?.salvoSize ?? null,
    calibration, mapPath: s.mapPath, own: effectiveOwn, target: effectiveTarget, heightDelta: usedHeight, arcs: result?.solution?.arcs ?? null });
  const salvoScopeRef = useRef(salvoScope); salvoScopeRef.current = salvoScope;
  const salvoPattern = currentSalvo(salvoForecast, salvoScope);
  const live = useLiveWeaponPanel(windowHandle, s.captureBackend);
  // Quick tank / hand-held mode: profile and RANGE from the fresh in-game panel, else presets.
  const livePanelFresh = panelFresh(live.analysis?.panel ?? null, now);
  const quickAim = useQuickAim({ profiles, overrides: s.overrides,
    panelProfileId: livePanelFresh && live.analysis?.profile?.status === 'matched' ? live.analysis.profile.profileId : null,
    rangefinderMeters: livePanelFresh ? live.analysis?.panel.rangeMeters ?? null : null });
  const [directCard, setDirectCard] = useState<OverlayDirectCard | null>(null);
  const requiredAim = requiredAimFrom(result, preferHigh);
  // Never correct the real gun against synthetic simulation data.
  const correction = simulation ? null : aimCorrection(requiredAim, live.analysis?.panel ?? null, now);
  const installResources=async(library:ResourceLibrary)=>{
    const merge=(current:Settings):Settings=>({...current,
      maps:[...current.maps.filter(m=>!library.maps.some(n=>n.id===m.id)),...library.maps],
      terrains:[...current.terrains.filter(t=>!library.terrains.some(n=>n.mapId===t.mapId)),...library.terrains],
      terrain:current.terrain&&library.terrains.some(t=>t.mapId===current.terrain!.mapId)?null:current.terrain,
    });
    validateSettings(merge(settingsRef.current));
    setMarkerSelections([]);setBatch(null);
    setS(current=>merge(current));log(`Wczytano ${library.maps.length} map i ${library.terrains.length} map wysokości`);
  };

  useEffect(() => {
    let alive = true;
    Promise.all([call<Profile[]>('profiles'), call<Settings | null>('load_settings'),call<CommunityMap[]>('community_maps')]).then(async ([p, saved, maps]) => {
      if (!alive) return;
      setProfiles(p);
      setCommunityMaps(maps);
      if (saved) {
        const valid = normalizeMapSettings(saved, validateSettings(saved));
        setProfiles([...p, ...Object.values(valid.overrides).filter(x => !p.some(b => b.id === x.id))]);
        setS(valid);
        if (valid.mapPath) {
          try { const img = await call<{ dataUrl: string; width: number; height: number; sha256:string }>('import_image', { path: valid.mapPath });
            if (img.width !== valid.mapWidth || img.height !== valid.mapHeight) throw new Error('Rozmiar zapisanej mapy zmienił się — ponownie zaimportuj i skalibruj mapę');
            if (alive) {setMapImage(img.dataUrl);setMapFingerprint(img.sha256);}
          } catch (e) { if (alive) { setNotice(`Nie wczytano zapisanej mapy (${String(e)}). ${valid.mapSource==='catalog'?'Wybierz ją ponownie z katalogu.':'Zaimportuj obraz ponownie.'}`); setS(old => ({ ...old, scale: null, own: null, target: null, mapPath: null })); } }
        }
      }
    }).catch(async e => {
      if (!alive) return;
      // Never let defaults overwrite an unreadable file: back it up first.
      setLoadFailed(true);
      try {
        const backup = await call<string | null>('backup_settings');
        if (alive) { setLoadFailed(false); setNotice(`Nie odczytano ustawień (${String(e)}). ${backup ? `Kopia oryginału: ${backup}. ` : ''}Używane są ustawienia domyślne.`); }
      } catch (backupError) {
        if (alive) setNotice(`Nie odczytano ustawień (${String(e)}) i nie udało się zrobić kopii (${String(backupError)}). Autozapis jest wyłączony, aby nie nadpisać pliku.`);
      }
    }).finally(() => { if (alive) setLoaded(true); });
    const timer = setInterval(() => setClock(Date.now()), 500);
    return () => { alive = false; clearInterval(timer); };
  }, []);
  useEffect(()=>{
    if(!loaded)return;let cancelled=false;setAssetsReady(false);setBatch(null);setMarkerSelections([]);
    // Lenient: one missing cache file is reported, not allowed to disable the whole library.
    void call('load_automation_assets',{...JSON.parse(assetsKey),lenient:true}).then(()=>call<AssetsStatus>('automation_assets_status')).then(status=>{if(!cancelled){setAssetsStatus(status);setAssetsReady(true);}}).catch(e=>{if(!cancelled){if(!String(e).includes('zastąpiono'))setNotice(String(e));setAssetsReady(false);setAssetsStatus(null);}});
    return()=>{cancelled=true;};
  },[loaded,assetsKey]);
  useEffect(() => {
    if (!loaded || loadFailed) return;
    const t = setTimeout(() => {
      try { validateSettings(s); savedError.current=''; void call('save_settings', { value: s }).catch(e => setNotice(String(e))); }
      catch (e) { const message=String(e); if (savedError.current!==message) { savedError.current=message; setNotice(`Ustawienia nie zostały zapisane: ${message}`); } }
    }, 500);
    return () => clearTimeout(t);
  }, [s, loaded, loadFailed]);
  useEffect(() => {
    setSpeed(profile?.speed ?? null); setGravity(profile?.gravity ?? null); setSigma(profile?.dispersionSigma ?? null); setSalvo(profile?.salvoSize ?? 1); setSource(profile?.calibrated ? profile.source : '');
  }, [profile]);
  useEffect(() => {
    let cancelled = false;
    setResult(null);
    if(s.mode==='automatic'&&(!autoWeaponReady||!autoMapReady)) {setWarning(!autoMapReady?`Brak obserwacji mapy — trzymam ostatnią mapę (${s.mapName}); pozycje Automatic wstrzymane.`:'Automatyka czeka na jednoznaczną nazwę pojazdu i amunicji.');return;}
    if(useOwnVelocity&&!calculationOwnVelocity) {setWarning('Brak pełnej świeżej prędkości własnego stanowiska.');return;}
    if(use3d&&((!heightConfirmed&&!terrainActive)||(!terrainActive&&height===null)||verticalVelocity===null||(useOwnVelocity&&ownVerticalVelocity===null)||!calculationVelocity)) {setWarning('Model 3D wymaga potwierdzonej różnicy wysokości oraz wszystkich składowych prędkości celu i stanowiska.');return;}
    if (!s.scale || !effectiveOwn || !effectiveTarget || !profile) {
      setWarning(!s.scale ? (s.mapSource==='catalog'?'Wybierz mapę z katalogu — skala zostanie ustawiona automatycznie.':'Wymagana skala mapy — ustaw kalibrację.') : automaticPositions && (!effectiveOwn||!effectiveTarget) ? 'Automatic: brak świeżej, jednoznacznej pozycji z widocznej mapy.' : !effectiveOwn ? 'Kliknij mapę, aby wskazać działo.' : !effectiveTarget ? 'Wskaż cel: prawy klik lub narzędzie „Cel”.' : 'Brak poprawnego profilu pocisku.');
      return;
    }
    try { validateSettings(s); } catch (e) { setWarning(String(e)); return; }
    call<Result>('calculate', { input: { calibration, own: effectiveOwn, target: effectiveTarget, profile, heightDelta: height, targetVelocity: calculationVelocity,ownVelocity:calculationOwnVelocity,mapUnit:activeMap?.unit??s.mapUnit,maxLeadTime:automaticPositions?batch?.targetMotion?.maxPredictionSeconds??10:null,terrainMapId:terrainActive?selectedTerrain!.mapId:null,currentMapId:calculationMapId,motion3d:use3d?{targetVerticalVelocity:verticalVelocity,ownVerticalVelocity:useOwnVelocity?ownVerticalVelocity:0,inheritOwnVelocity:useOwnVelocity,maxTime:automaticPositions?batch?.targetMotion?.maxPredictionSeconds??10:60}:null } }).then(r => {
      if (!cancelled) { setResult(r); setWarning(''); }
    }).catch(e => { if (!cancelled) setWarning(String(e)); });
    return () => { cancelled = true; };
  }, [s.scale, s.rotation, s.uncertainty, s.mapUnit, effectiveOwn?.x,effectiveOwn?.y,effectiveTarget?.x,effectiveTarget?.y, profile, height, calculationVelocity?.[0],calculationVelocity?.[1],calculationOwnVelocity?.[0],calculationOwnVelocity?.[1], automaticPositions,liveFresh,autoWeaponReady,autoMapReady,use3d,verticalVelocity,ownVerticalVelocity,heightConfirmed,terrainActive,selectedTerrain,activeMap?.id,calculationMapId,assetsReady,useOwnVelocity,scanKey]);
  useEffect(() => {
    const accepted = s.mode === 'automatic' && (observation?.confidence ?? 0) < 0.85 ? null : observation;
    const map={name:s.mapName,ownSector:effectiveOwn?sectorOf(effectiveOwn,s.mapWidth,s.mapHeight,s.mapGrid)?.label??null:null,targetSector:effectiveTarget?sectorOf(effectiveTarget,s.mapWidth,s.mapHeight,s.mapGrid)?.label??null:null,heightDelta:result?.heightDelta??null,heightSource:result?.heightSource??null};
    void call('publish', { value: { result, unit: s.unit, profile: profile?.name, observation: accepted, observations:batch?.analysis.numericObservations??[], mode: s.mode, warning, training,positionsTimestamp:automaticPositions?batch?.timestampMs??null:null,experimental:!!profile?.sourceMetadata&&!profile.calibrated,map,aim:correction?{elevation:correction.elevation,azimuth:correction.azimuth,onTarget:correction.onTarget,panelTimestampMs:correction.panelTimestampMs,highArc:requiredAim?.highArc??false}:null,salvo:salvoPattern?{status:salvoPattern.status,salvoSize:salvoPattern.salvoSize,semiMajor:salvoPattern.salvoEnvelope.semiMajor,semiMinor:salvoPattern.salvoEnvelope.semiMinor,unit:salvoPattern.unit,incomplete:salvoPattern.missingComponents.length>0}:null,
      quick:quickAim.config.enabled?{line:quickAim.line,profile:quickAim.profile?.name??null,source:quickAim.profileSource,rangefinder:quickAim.rangefinder}:null,
      direct:tab==='direct'||quickAim.config.showCard?directCard:null } }).catch(() => {});
  }, [result, s.unit, s.mode, profile, observation, batch,warning, training,automaticPositions,s.mapName,s.mapWidth,s.mapHeight,s.mapGrid,effectiveOwn?.x,effectiveOwn?.y,effectiveTarget?.x,effectiveTarget?.y,salvoPattern,correction?.elevation?.label,correction?.azimuth?.label,correction?.onTarget,correction?.panelTimestampMs,quickAim.line,quickAim.config.enabled,quickAim.profile?.name,quickAim.profileSource,quickAim.rangefinder,tab,directCard]);
  useEffect(() => {
    if (!native) return;
    const off = listen<{clickThrough: boolean; visible: boolean}>('overlay-state', e => { setClickThrough(e.payload.clickThrough); setOverlayVisible(e.payload.visible); });
    return () => { void off.then(f => f()); };
  }, []);

  // Global indicator only. Recording itself is controlled (and stopped on leaving
  // the tab) by RecordingPanel; we poll while it can be active.
  useEffect(() => {
    if (!native) return;
    let alive = true;
    const poll = () => void call<RecordingStatus>('recording_status').then(next => { if (alive) setRecording(next); }).catch(() => {});
    poll();
    const timer = setInterval(poll, tab === 'vision' || recording?.active ? 1000 : 3000);
    return () => { alive = false; clearInterval(timer); };
  }, [tab, recording?.active]);

  const acceptAnalysis = (data: Analysis) => {
    setAnalysis(data);
    if (data.observation && fresh(data.observation, Date.now())) {
      const stable = stabilize(data.observation, previousObservation.current);
      previousObservation.current = stable; setObservation(stable);
    } else { previousObservation.current = null; setObservation(null); }
  };
  const acceptBatch = (data:BatchAnalysis) => {
    setBatch(data);
    setBatchScope(scanKey);
    const preferred=data.analysis.numericObservations.find(o=>o.kind==='heading')??data.analysis.numericObservations.find(o=>o.kind==='timer')??data.analysis.numericObservations[0]??null;
    setObservation(preferred);
    if(data.preview) setCapturePreview(data.preview);
    if(s.captureReferenceWidth===0 && data.backend!=='file') update({captureReferenceWidth:data.width,captureReferenceHeight:data.height});
    // The chosen map is persistent: only two consecutive recognitions of ANOTHER
    // map switch it; lost observations (death/respawn screen) hold the last map.
    if(s.autoMap&&data.backend!=='file') {
      const map=data.automation?.activeMap??null;
      const current=settingsRef.current;
      const currentId=current.maps.find(m=>m.path===current.mapPath)?.id??null;
      const decision=trackAutoMap(autoMapTracker.current,{mapId:map?.id??null,timestampMs:data.timestampMs},currentId);
      autoMapTracker.current=decision.tracker;setAutoMapStatus(decision.status);
      if(decision.switchTo&&map) void switchToAutoMap(map);
    }
  };
  useEffect(() => {
    if (!running || !windowHandle || (s.autoMap&&!assetsReady)) return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;
    previousObservation.current = null; setObservation(null);
    setBatch(null);setBatchScope('');
    const scan = async () => {
      const started = performance.now();
      try {
        validateSettings(s);
        const data = await call<BatchAnalysis>('analyse_batch', { request:{ handle: windowHandle, backend:s.captureBackend,config:JSON.parse(visionConfigKey),calibration:JSON.parse(calibrationKey),scope:'ui',maxSpeed:s.maxTrackingSpeed,includePreview:false,fps:s.fps,automation:{autoHud:s.autoHud,autoMap:s.autoMap,preferredMapId:s.communityMapId},selections:markerSelections } });
        if (!cancelled) { acceptBatch(data); setNotice(''); }
      } catch (e) {
        // Another capture consumer (marker preview, recording) holds the frame slot: retry
        // without discarding the last observation; the 2 s freshness gate still applies.
        if (!cancelled && !isCaptureBusy(e)) { setNotice(String(e)); setObservation(null); setBatch(null);previousObservation.current = null; }
      }
      if (!cancelled) timer = setTimeout(scan, nextScanDelay(s.fps, performance.now() - started));
    };
    void scan();
    return () => { cancelled = true; clearTimeout(timer); };
  }, [running, scanKey,assetsReady]);
  useEffect(()=>{if(!running){setBatch(null);setObservation(null);setMarkerSelections([]);void call('stop_capture').catch(()=>{});}},[running]);
  useEffect(()=>{setMarkerSelections([]);void call('stop_capture').catch(()=>{});},[windowHandle,s.captureBackend,s.autoMap,s.communityMapId]);
  useEffect(()=>{if(s.mode==='manual'&&running)setRunning(false);},[s.mode]);
  useEffect(()=>{
    if(!liveFresh||!s.autoWeapon||!batch)return;
    const match=batch.automation?.weapon;
    if(match?.status==='matched'&&match.confidence>=0.85&&match.profileId&&match.profileId!==s.selected) {update({selected:match.profileId});setNotice(`Profil dopasowany z jawnego HUD: ${profiles.find(p=>p.id===match.profileId)?.name??match.profileId}`);}
  },[batch,liveFresh,s.autoWeapon,profiles,s.selected]);
  // The displayed map changes only with the persistent map choice (no per-frame image swap).
  const registerMap=async(source:string)=>{
    if(!s.mapPath||!calibration||!source.trim())throw new Error('Wymagane lokalny plik mapy, skala i źródło');
    const id=s.communityMapId??`local-${crypto.randomUUID()}`;
    const sha256=await call<string>('image_fingerprint',{path:s.mapPath});
    if(sha256!==mapFingerprint)throw new Error('Plik mapy zmienił się od importu — ponów import i kalibrację');
    const entry={sha256,id,name:s.mapName,path:s.mapPath,width:s.mapWidth,height:s.mapHeight,unit:s.mapUnit,source:source.trim(),confirmed:true,calibration};
    const maps=[...s.maps.filter(m=>m.id!==id),entry];
    await call('load_automation_assets',{maps,terrain:s.terrain,terrains:s.terrains});update({maps});log(`Dodano wzorzec mapy ${entry.name}`);
  };
  const applyFit=(fit:CalibrationFit)=>{const p=fit.fittedProfile;update({selected:p.id,overrides:{...s.overrides,[p.id]:p}});log(`Dopasowano model ${p.name} z pomiarów; status beta`);};
  const snapshot = () => attempt(async()=>{
    if(!windowHandle)throw new Error('Wybierz okno');
    const data=await call<{dataUrl:string;width:number;height:number}>('capture_preview',{handle:windowHandle,backend:s.captureBackend});
    setCapturePreview(data.dataUrl);update({captureReferenceWidth:data.width,captureReferenceHeight:data.height});
  });
  const selectWindow=(handle:number|null)=>{setRunning(false);setBatch(null);setWindowHandle(handle);update({captureReferenceWidth:0,captureReferenceHeight:0,markerConfig:{...s.markerConfig,coordinateCalibrationConfirmed:false,viewSignature:null}});};
  const openCatalog=(known:CommunityMap)=>loadCatalogMap(known,{settings:()=>settingsRef.current,setSettings:setS,showImage:(url,sha)=>{mapChanged();setMapImage(url);setMapFingerprint(sha);},log});
  // Global Ctrl+Shift+T / Ctrl+Shift+G: cursor over the open in-game map sets target / own position (read-only).
  const mapHotkeys=useMapPointHotkeys({enabled:true,settings:s,update,handle:windowHandle,backend:s.captureBackend,communityMaps,openCatalog,notice:setNotice});
  // Beta, off by default and not persisted: own position from the minimap player arrow.
  const [autoOwn,setAutoOwn]=useState(false);
  const ownPosition=useOwnPosition(autoOwn,s,update,windowHandle,s.captureBackend);
  void mapHotkeys;
  const switchToAutoMap=(map:StoredMap)=>attempt(async()=>{
    const img=await call<{dataUrl:string;width:number;height:number;sha256:string}>('import_image',{path:map.path});
    if(img.sha256!==map.sha256||img.width!==map.width||img.height!==map.height)throw new Error(`Plik mapy ${map.name} zmienił się — automatyczne przełączenie wstrzymane`);
    invalidateCatalogLoads();mapChanged();
    const known=communityMaps.find(m=>m.id===map.id);
    setS(current=>({...endSimulation(current),mapPath:map.path,mapName:map.name,mapWidth:map.width,mapHeight:map.height,scale:map.calibration.unitsPerPixel,rotation:map.calibration.northRotationDeg,mapUnit:map.unit,unit:map.unit==='game_meters'?'m gry':'jedn. gry',own:null,target:null,
      ...(known&&isCatalogEntry(map,known)?{mapSource:'catalog' as const,communityMapId:map.id}:{mapSource:'custom' as const})}));
    setMapImage(img.dataUrl);setMapFingerprint(img.sha256);
    log(`Automatyka: dwukrotnie rozpoznano inną mapę — przełączono na ${map.name}`);setNotice(`Automatyka przełączyła mapę na ${map.name} (dwa zgodne rozpoznania).`);
  });
  /** Any new map invalidates height inputs, pending catalog loads and the salvo forecast. */
  const mapChanged = () => { setHeight(null); setHeightConfirmed(false); setSalvoForecast(null); setHeldAuto({own:null,target:null,timestampMs:0}); setOwnMaybeStale(false); autoMapTracker.current=emptyTracker; };
  const calibrateMapView = () => attempt(async()=>{
    invalidateCatalogLoads(); mapChanged();
    if(!windowHandle)throw new Error('Wybierz okno z otwartą mapą');
    const data=await call<{signature:MapViewSignature;mapImage:string;width:number;height:number;referenceWidth:number;referenceHeight:number}>('calibrate_map_view',{handle:windowHandle,backend:s.captureBackend,roi:s.markerConfig.mapRoi});
    setMapImage(data.mapImage);
    setS(current=>endSimulation(current));
    update({mapSource:'custom',mapPath:null,mapName:'Mapa z widocznego kadru — wybrana ręcznie',mapWidth:data.width,mapHeight:data.height,scale:null,own:null,target:null,captureReferenceWidth:data.referenceWidth,captureReferenceHeight:data.referenceHeight,markerConfig:{...s.markerConfig,mapWidth:data.width,mapHeight:data.height,viewSignature:data.signature,coordinateCalibrationConfirmed:true}});
    log('Zapamiętano deskryptor widoku mapy (256 wartości); klatka nie jest zapisywana');setNotice('Wzorzec widoku gotowy. Ustaw teraz skalę i kierunek mapy.');
  });
  const analyseOffline = () => attempt(async()=>{
    const path=await pickImage();if(!path)return;
    acceptBatch(await call<BatchAnalysis>('analyse_batch_file',{path,config:visionConfig,calibration,maxSpeed:s.maxTrackingSpeed}));
  });
  const analyseFixture = (name:string) => attempt(async()=>{
    const data=await call<BatchAnalysis>('analyse_batch_fixture',{name});
    acceptBatch(data);update({hudRegions:s.hudRegions.map(r=>r.id==='timer'?{...r,enabled:true,roi:data.configuration.ocrRegions[0].roi}:{...r,enabled:false})});
  });
  const applyMarkers = () => {
    if(!batch||batch.timestampMs>Date.now()||Date.now()-batch.timestampMs>2000){setNotice('Brak świeżych markerów');return;}
    const own=batch.analysis.markers.filter(m=>m.role==='own'&&m.confidence>=0.6);const target=batch.analysis.markers.filter(m=>m.role==='target'&&m.confidence>=0.6);
    if(own.length!==1||target.length!==1){setNotice('Markery są niejednoznaczne lub przycięte — wskaż punkty ręcznie');return;}
    update({own:own[0].mapPosition,target:target[0].mapPosition,mode:'assisted'});log('Przyjęto ręcznie jednoznaczne pozycje widocznych markerów');
  };

  const demo = () => {
    invalidateCatalogLoads(); setSalvoForecast(null);
    setHeight(0);setHeightConfirmed(true);setUse3d(false);setUseOwnVelocity(false);
    const p = profiles.find(p => p.id === 'tube'); if (!p) return;
    const demoProfile: Profile = { ...p, speed: 100, gravity: 10, calibrated: true, status: 'experimental', source: 'SYMULACJA: v=100 jedn./s, g=10 jedn./s²; bez odniesienia do MTC', gameVersion: 'syntetyczna', dispersionSigma: 12, salvoSize: 1 };
    setMapImage(null);
    setRunning(false);
    update({ mapSource:'custom', useTerrainHeight:false, selected: 'tube', mode:'manual',mapUnit:'game_units',unit:'jedn. gry',mapPath: null, mapName: SIMULATION_MAP_NAME, mapWidth: 800, mapHeight: 600, scale: 1, rotation: 0, uncertainty: 2, own: { x: 180, y: 430 }, target: { x: 610, y: 170 }, overrides: { ...s.overrides, tube: demoProfile },markerConfig:{...s.markerConfig,enabled:false,coordinateCalibrationConfirmed:false,viewSignature:null} });
    log('Uruchomiono symulację z syntetyczną mapą i parametrami');
  };
  const applyProfile = (event: FormEvent) => { event.preventDefault(); void attempt(async () => {
    if (!profile || profile.category === 'guided_missile') throw new Error('Mechanika naprowadzania wymaga osobnego modelu');
    if (!speed || !gravity || !source.trim() || !salvo || !Number.isInteger(salvo) || salvo < 1 || salvo > 1000 || (sigma !== null && sigma < 0)) throw new Error('Podaj dodatnią prędkość, grawitację, źródło i poprawną salwę');
    const calibrated: Profile = { ...profile, calibrated: true, speed, gravity, dispersionSigma: sigma, salvoSize: salvo, source: source.trim(), status: source.startsWith('SYMULACJA') ? 'experimental' : 'beta' };
    // Rust validates the profile when calculating; import/export retains provenance.
    update({ overrides: { ...s.overrides, [profile.id]: calibrated } });
    log(`Kalibracja profilu ${profile.name}: ${source.trim()}`); setNotice('Zapisano lokalną kalibrację. Sprawdź wyniki na strzelnicy w grze.');
  }); };
  const toggleOverlay = () => attempt(async () => { await call('overlay_control', { visible: !overlayVisible, clickThrough: null }); setOverlayVisible(!overlayVisible); });
  const toggleClick = () => attempt(async () => { await call('overlay_control', { visible: null, clickThrough: !clickThrough }); });
  const refreshWindows = () => attempt(async () => { setWindows(await call('capture_windows')); });
  const calculationUnit = activeMap?.unit ?? s.mapUnit;
  const delta = effectiveOwn && effectiveTarget && calibration ? worldDelta(effectiveOwn, effectiveTarget, calibration.unitsPerPixel, calibration.northRotationDeg) : null;
  const unitsAgree = calculationUnit === (profile?.unit ?? 'game_units');
  const interceptDefaults = {
    relative: unitsAgree && delta && usedHeight !== null ? [delta[0], delta[1], usedHeight] as [number, number, number] : null,
    targetVelocity: unitsAgree && calculationVelocity && verticalVelocity !== null ? [calculationVelocity[0], calculationVelocity[1], verticalVelocity] as [number, number, number] : null,
    // Unknown own motion stays empty: the user must enter it (also 0) explicitly.
    ownVelocity: unitsAgree && calculationOwnVelocity && ownVerticalVelocity !== null ? [calculationOwnVelocity[0], calculationOwnVelocity[1], ownVerticalVelocity] as [number, number, number] : null,
    maxTime: automaticPositions ? batch?.targetMotion?.maxPredictionSeconds ?? 10 : 60,
    note: !unitsAgree ? `Jednostki mapy (${calculationUnit === 'game_meters' ? 'metry gry' : 'jednostki gry'}) i profilu są różne — pozycje z mapy nie są podstawiane.` : !delta || usedHeight === null ? 'Brak pełnej pozycji 3D z mapy (dwa punkty i różnica wysokości).' : null,
  };
  // Optional: follow an unambiguous profile read from the in-game panel (user opt-in only).
  const panelProfile = panelFresh(live.analysis?.panel ?? null, now) && live.analysis?.profile?.status === 'matched' ? live.analysis.profile.profileId : null;
  useEffect(() => { if (autoProfile && panelProfile && panelProfile !== s.selected && profiles.some(p => p.id === panelProfile)) { update({ selected: panelProfile }); log(`Profil z panelu gry: ${panelProfile}`); } }, [autoProfile, panelProfile]);
  // Long weapon-panel pause (death, respawn, loadout): own position may be outdated — inform, never clear.
  const panelTs = live.analysis?.panel.status !== 'blocked' ? live.analysis?.panel.timestampMs ?? null : null;
  useEffect(() => { if (panelTs === null) return; if (respawnGap(lastPanelTs.current, panelTs) && settingsRef.current.own) setOwnMaybeStale(true); lastPanelTs.current = panelTs; }, [panelTs]);
  useEffect(() => { if (!live.running) lastPanelTs.current = null; }, [live.running]);
  useEffect(() => { setOwnMaybeStale(false); }, [s.own?.x, s.own?.y]);
  // Automatic: keep the last seen positions on the map when markers disappear (display only).
  useEffect(() => { if (automaticPositions && (effectiveOwn || effectiveTarget)) setHeldAuto(previous => holdPositions(previous, effectiveOwn, effectiveTarget, Date.now())); }, [automaticPositions, effectiveOwn?.x, effectiveOwn?.y, effectiveTarget?.x, effectiveTarget?.y]);
  const respawnProps = { ownStale: ownMaybeStale, onClearOwn: () => { update({ own: null }); setOwnMaybeStale(false); }, onDismissOwnStale: () => setOwnMaybeStale(false) };
  useEffect(() => { if (salvoForecast && salvoForecast.scope !== salvoScope) setSalvoForecast(null); }, [salvoForecast, salvoScope]);
  const validObservation = fresh(observation, now) && (s.mode !== 'automatic' || (observation?.confidence ?? 0) >= 0.85);

  return <div className="app-shell">
    <header className="topbar"><div className="brand"><div className="brand-icon"><Crosshair size={27} /></div><div><h1>MTC <span>TACTICAL ASSISTANT</span></h1><p>Panel operatora / Modern</p></div></div><div className="topbar-actions">{recording?.active && <span className="recording-indicator" role="status" aria-live="polite" title={recording.directory ?? undefined}><i aria-hidden="true" />NAGRYWANIE · {recording.frames} kl.</span>}<span className="build-tag">WERSJA 0.5</span><button className={overlayVisible ? 'primary' : ''} onClick={toggleOverlay}><Monitor size={16} />{overlayVisible ? 'Ukryj nakładkę' : 'Pokaż nakładkę'}</button></div></header>
    {!native && <div className="banner">Podgląd interfejsu w przeglądarce. Obliczenia Rust i przechwytywanie są dostępne w aplikacji Windows.</div>}
    {training && <div className="banner training"><AlertTriangle size={16} /> SYMULACJA — sztuczna skala i parametry; wyniki nie są kalibracją MTC.</div>}
    <div className="workspace">
      <aside className="sidebar">
        <div className="side-label">STANOWISKO</div>
        <label className="field"><span>Szukaj pojazdu / amunicji</span><input value={profileSearch} onChange={e=>setProfileSearch(e.target.value)} placeholder="Np. BM-21, M109, HE"/></label>
        <label className="field"><span>Uzbrojenie</span><select value={s.selected} onChange={e => update({ selected: e.target.value })}>{profiles.filter(p=>p.id===s.selected||p.name.toLowerCase().includes(profileSearch.toLowerCase())).map(p => <option key={p.id} value={p.id}>{p.name}</option>)}</select></label>
        <span className={`profile-status ${profile?.calibrated ? 'beta' : ''}`}>{training ? 'Eksperymentalny / symulacja' : profile?.calibrated ? 'Beta / lokalna kalibracja' : profile?.sourceMetadata ? 'Eksperymentalny / źródło community' : 'Brak danych balistycznych'}</span>
        <div className="side-label">TRYB ASYSTENTA</div>
        <div className="modes">{(['manual', 'assisted', 'automatic'] as const).map((m, i) => <button key={m} className={s.mode === m ? 'active' : ''} onClick={() => update({ mode: m })}><span>{['Manual', 'Assisted', 'Automatic'][i]}</span><small>{['Punkty i dane ręczne', 'Mapa ręczna + odczyt HUD', 'HUD po stabilizacji ≥85%'][i]}</small></button>)}</div>
        <p className="hint">Pozycje Automatic wymagają mapy, wzorca widoku i jednoznacznych markerów. HUD nie dostarcza współrzędnych świata.</p>
        <nav className="navigation">{([{ id: 'map', label: 'Mapa i cel', icon: MapIcon }, { id: 'aim', label: 'Celowanie (gra na żywo)', icon: Gamepad2 }, { id: 'vision', label: 'Analiza obrazu', icon: ScanLine }, { id: 'automation', label: 'Automatyka', icon: Settings2 }, { id: 'weapons', label: 'Profile i balistyka', icon: Rocket }, { id: 'direct', label: 'Broń bezpośrednia / śmigłowiec', icon: Target }, { id: 'log', label: 'Historia i diagnostyka', icon: Radar }] as const).map(t => <button key={t.id} className={tab === t.id ? 'active' : ''} onClick={() => setTab(t.id)}><t.icon size={18} />{t.label}</button>)}</nav>
        <div className="sidebar-bottom"><button onClick={toggleClick}><Navigation size={16} />{clickThrough ? 'Włącz edycję nakładki' : 'Przepuszczaj kliknięcia'}</button><p>Ctrl+Shift+O · widoczność<br />Ctrl+Shift+I · kliknięcia<br />Ctrl+Shift+M · przywróć panel</p><span className="local-indicator"><i />Przetwarzanie lokalne</span></div>
      </aside>
      <main>
        <div className="metrics-row"><div className="metric"><small>DYSTANS DO CELU</small><strong>{format(result?.bearing.distance)}<span>{s.unit}</span></strong><p>{result ? (result.bearing.uncertaintyUnits > 0 ? `Niepewność pozycji ±${result.bearing.uncertaintyUnits.toFixed(1)} ${s.unit}` : "Niepewność nieoszacowana") : 'Wymaga skali i dwóch pozycji'}</p></div><div className="metric"><small>AZYMUT</small><strong>{format(result?.intercept3d?.arcs[0]?.azimuthDeg??result?.bearing.azimuthDeg)}<span>°</span></strong><p>Północ 0° · wschód 90°</p></div><div className="metric"><small>ELEWACJA / CZAS LOTU</small><strong>{format((result?.intercept3d?.arcs[0]??result?.solution?.arcs[0])?.elevationDeg, 2)}<span>° / {format((result?.intercept3d?.arcs[0]??result?.solution?.arcs[0])?.flightTime, 2)} s</span></strong><p>{result?.solution ? profile?.sourceMetadata&&!profile.calibrated?'Model community / eksperymentalny':'Model kalibrowany bez oporu' : 'Wymagana kalibracja pocisku'}</p></div></div>
        {(warning || result?.ballisticWarning) && <div className="warning-strip"><AlertTriangle size={16} />{warning || result?.ballisticWarning}</div>}
        {notice && <div className="notice" role="status">{notice}<button aria-label="Zamknij komunikat" onClick={() => setNotice('')}>×</button></div>}

        {tab === 'map' && <MapPanel settings={s} update={update} setSettings={setS} getSettings={()=>settingsRef.current} log={log} notice={setNotice} communityMaps={communityMaps}
          mapImage={mapImage} setMapImage={setMapImage} setMapFingerprint={setMapFingerprint} result={result} warning={warning} profile={profile} training={training}
          effectiveOwn={effectiveOwn} effectiveTarget={effectiveTarget} automaticPositions={automaticPositions}
          terrain={{available:!!selectedTerrain,active:terrainActive,loading:!!selectedTerrain&&!assetsReady}} assetsStatus={assetsStatus}
          height={height} setHeight={setHeight} onDemo={demo} native={native} salvoPattern={salvoPattern}
          calibration={calibration} calculationUnit={calculationUnit} onMapChanged={mapChanged} staleTerrain={staleTerrain}
          {...respawnProps} heldPositions={automaticPositions?heldAuto:null} autoMapNote={s.autoMap&&running?(autoMapStatus==='holding'||!autoMapReady?`Brak obserwacji mapy — trzymam ostatnią mapę (${s.mapName}).`:autoMapStatus==='confirming'?'Rozpoznano inną mapę — czekam na drugie zgodne rozpoznanie.':null):null}/>}

        {tab === 'aim' && <LiveGamePanel settings={s} update={update} windows={windows} refreshWindows={refreshWindows} handle={windowHandle} chooseWindow={selectWindow}
          live={live} now={now} required={requiredAim} correction={correction} preferHigh={preferHigh} setPreferHigh={setPreferHigh}
          profile={profile} profiles={profiles} autoProfile={autoProfile} setAutoProfile={setAutoProfile} communityMaps={communityMaps} openCatalog={openCatalog} notice={setNotice}
          simulation={simulation} onEndSimulation={()=>{setS(current=>endSimulation(current));log('Zakończono symulację');}} unitMismatch={!!result?.ballisticWarning?.includes('Jednostki')} {...respawnProps}/>}
        {tab === 'aim' && <OwnPositionToggle enabled={autoOwn} setEnabled={setAutoOwn} reading={ownPosition.reading} message={ownPosition.message}/>}
        {tab === 'vision' && <><MarkerPicker handle={windowHandle} backend={s.captureBackend} config={visionConfig} autoMap={s.autoMap} mapId={s.communityMapId} scope="ui" selections={markerSelections} notice={setNotice} onSelect={(signature,selection,preview)=>{
          const compatible=s.markerConfig.signatures.find(sig=>sig.role==='unassigned'&&sig.rgb.every((n,i)=>n===signature.rgb[i])&&sig.tolerance===signature.tolerance);
          const id=compatible?.id??signature.id;
          const merged=compatible?{...signature,id,minComponentPixels:Math.min(compatible.minComponentPixels,signature.minComponentPixels),maxComponentPixels:Math.max(compatible.maxComponentPixels,signature.maxComponentPixels),minAspectRatio:Math.min(compatible.minAspectRatio,signature.minAspectRatio),maxAspectRatio:Math.max(compatible.maxAspectRatio,signature.maxAspectRatio),minFillRatio:Math.min(compatible.minFillRatio,signature.minFillRatio)}:signature;
          const hint={...selection.hint,signatureId:id};
          setMarkerSelections(old=>[...old.filter(item=>item.hint.role!==hint.role),{...selection,hint}]);
          const signatures=[...s.markerConfig.signatures.filter(sig=>sig.id!==id&&sig.role!=='own'&&sig.role!=='target'),merged];
          update({mode:'automatic',captureReferenceWidth:selection.hint.frameWidth,captureReferenceHeight:selection.hint.frameHeight,markerConfig:{...s.markerConfig,enabled:true,signatures}});setCapturePreview(preview);setRunning(true);
        }}/><VisionPanel analyseRecordedFrame={path=>void attempt(async()=>{acceptBatch(await call<BatchAnalysis>('analyse_batch_file',{path,config:visionConfig,calibration,maxSpeed:s.maxTrackingSpeed}));})} settings={s} update={update} windows={windows} handle={windowHandle} chooseWindow={selectWindow} running={running} toggle={()=>{if(!windowHandle){setNotice('Najpierw wybierz okno');return;}if(!running&&s.mode==='manual')update({mode:'assisted'});setRunning(!running);}} refresh={refreshWindows} snapshot={snapshot} preview={capturePreview} analysis={batch} now={now} offline={analyseOffline} fixture={analyseFixture} applyMarkers={applyMarkers} calibrateMapView={calibrateMapView}/></>}

        {tab === 'automation' && <><MapResourcesPanel sourceAccepted={sourceAccepted} onInstall={installResources} notice={setNotice}/><AutomationPanel settings={s} update={update} analysis={batch} running={running} profile={profile} registerMap={registerMap} notice={setNotice} applyFit={applyFit}/></>}
        {tab === 'weapons' && <>
          <DynamicsPanel profile={profile} notice={setNotice} onApply={dynamics=>{if(profile){update({overrides:{...s.overrides,[profile.id]:{...profile,dynamics}}});log(`Zastosowano model ${dynamics?.model??'vacuum'} dla ${profile.name}`);}}}/>
          <SalvoPanel profile={profile} context={result?.solution && usedHeight !== null ? { distance: result.bearing.distance, azimuthDeg: result.bearing.azimuthDeg, heightDelta: usedHeight, arcs: result.solution.arcs } : null}
            onApplySalvo={salvo => { if (profile) { update({ overrides: { ...s.overrides, [profile.id]: { ...profile, salvo } } }); log(`Rozrzut salwy dla ${profile.name}: ${salvo ? salvo.status : 'usunięty'}`); } }}
            onPattern={pattern => setSalvoForecast(pattern ? { pattern, scope: salvoScopeRef.current } : null)} notice={setNotice} />
          {profile?.dynamics && profile.dynamics.model !== 'vacuum' && <DynamicInterceptPanel key={profile.id} profile={profile} defaults={interceptDefaults} notice={setNotice} />}
          <section className="panel"><h2>Wyprzedzenie z grawitacją i wysokością</h2><label className="check-field"><input type="checkbox" checked={use3d} onChange={e=>setUse3d(e.target.checked)}/>Obliczaj przechwycenie 3D z grawitacją (wymaga wysokości i prędkości)</label><label className="check-field"><input type="checkbox" checked={s.useTerrainHeight} onChange={e=>update({useTerrainHeight:e.target.checked})}/>Pobieraj różnicę wysokości z wczytanej mapy terenu ({terrainActive?'aktywna':selectedTerrain?'niedostępna dla bieżącej skali/plików':'brak dla tej mapy'})</label><label className="check-field"><input type="checkbox" checked={heightConfirmed} onChange={e=>setHeightConfirmed(e.target.checked)}/>Potwierdzam ręcznie podaną różnicę wysokości celu i stanowiska</label><div className="form-grid"><NumberField label="Prędkość celu w górę (jedn./s)" value={verticalVelocity} onChange={setVerticalVelocity}/><NumberField label="Własna prędkość w górę (jedn./s)" value={ownVerticalVelocity} onChange={setOwnVerticalVelocity}/></div><p className="hint">Wpisz 0 wyłącznie dla potwierdzonego braku ruchu pionowego. Automatyczny ruch z mapy dostarcza dwie składowe; wysokości celu powietrznego nie da się odczytać z płaskiej mapy.</p>{result?.intercept3d&&<><div className="arc-list">{result.intercept3d.arcs.map((arc,i)=><div key={i}><b>Rozwiązanie 3D {i+1}</b><strong>{arc.elevationDeg.toFixed(2)}°</strong><span>azymut {arc.azimuthDeg.toFixed(2)}° · {arc.flightTime.toFixed(2)} s</span></div>)}</div><ul className="assumptions">{result.intercept3d.assumptions.map(a=><li key={a}>{a}</li>)}</ul></>}</section>
          <section className="panel"><div className="panel-heading"><div><h2>{profile?.name || 'Profil uzbrojenia'}</h2><p>{profile?.notes}</p></div><Rocket size={22} /></div><p className="source">Źródło: {profile?.source}<br />Wersja gry: {profile?.gameVersion}</p>{profile?.sourceMetadata && <div className="community-source"><b>Dane społeczności — parametry niezweryfikowane w obecnej grze</b><p>Repo: {profile.sourceMetadata.repository}<br/>Commit: {profile.sourceMetadata.commit.slice(0,12)} · licencja {profile.sourceMetadata.license}<br/>Doświadczenie źródłowe: {profile.sourceMetadata.experienceId} · prędkość {profile.speed} m gry/s · grawitacja {profile.gravity} m gry/s²</p><label className="check-field"><input type="checkbox" checked={profile.sourceAcknowledged??false} onChange={e=>update({overrides:{...s.overrides,[profile.id]:{...profile,sourceAcknowledged:e.target.checked}}})}/>Używaj tych eksperymentalnych danych dla wybranego pocisku</label><p className="hint">Potwierdzenie nie zmienia statusu na verified. Mapa musi korzystać z metrów gry. Naprowadzanie pozostaje wyłączone.</p></div>}<form onSubmit={applyProfile}><fieldset disabled={profile?.category === 'guided_missile'}><legend>Kalibracja modelu pocisku w grze</legend><div className="form-grid"><label className="field"><span>Jednostka profilu</span><select value={profile?.unit??'game_units'} onChange={e=>{if(profile)update({overrides:{...s.overrides,[profile.id]:{...profile,unit:e.target.value as Settings['mapUnit'],calibrated:false,sourceAcknowledged:false}}});}}><option value="game_units">Własne jednostki gry</option><option value="game_meters">Metry gry</option></select></label><NumberField label="Prędkość pocisku (jedn./s)" value={speed} onChange={setSpeed} min={0.001} /><NumberField label="Grawitacja gry (jedn./s²)" value={gravity} onChange={setGravity} min={0.001} /><NumberField label="Rozrzut σ (jedn., opcjonalny)" value={sigma} onChange={setSigma} min={0} /><NumberField label="Liczba pocisków salwy" value={salvo} onChange={setSalvo} min={1} max={1000} step="1" /><label className="field wide"><span>Źródło / opis pomiaru (wymagane)</span><input value={source} onChange={e => setSource(e.target.value)} required maxLength={500} placeholder="Np. pomiar na strzelnicy, data i wersja gry" /></label></div><p className="hint">Użyj spójnych jednostek gry. Model pomija opór, napęd rakiet i ruch własny. Lokalna kalibracja ma stan beta; wymaga porównania z trafieniami.</p><button type="submit" className="primary"><Check size={16} />Zastosuj lokalną kalibrację</button></fieldset></form><div className="button-group separated"><button onClick={() => void attempt(async () => { const path = await pickJson(); if (!path) return; const p = await call<Profile>('import_profile', { path }); setProfiles(old => old.some(x => x.id === p.id) ? old : [...old, p]); update({ selected: p.id, overrides: { ...s.overrides, [p.id]: p } }); log(`Import profilu ${p.name}`); })}>Importuj profil JSON</button><button onClick={() => void attempt(() => saveJson(`mtc-${profile?.id}.json`, profile))}>Eksportuj profil</button><button onClick={() => { const overrides = { ...s.overrides }; delete overrides[s.selected]; update({ overrides }); }}>Usuń lokalną kalibrację</button></div></section>
          <section className="panel"><div className="panel-heading"><div><h2>Trajektorie i ruch celu</h2><p>Obliczenia na podstawie ręcznie oznaczonych pozycji</p></div></div><div className="form-grid"><NumberField label="Cel wyżej niż stanowisko (jedn.)" value={height} onChange={setHeight} /><NumberField label="Prędkość celu na wschód (jedn./s)" value={velocityX} onChange={setVelocityX} /><NumberField label="Prędkość celu na północ (jedn./s)" value={velocityY} onChange={setVelocityY} /><NumberField label="Własna prędkość na wschód (jedn./s)" value={ownVelocityX} onChange={setOwnVelocityX}/><NumberField label="Własna prędkość na północ (jedn./s)" value={ownVelocityY} onChange={setOwnVelocityY}/><label className="check-field"><input type="checkbox" checked={useOwnVelocity} onChange={e=>setUseOwnVelocity(e.target.checked)}/>Model wyprzedzenia dziedziczy prędkość stanowiska (przybliżenie, wymaga walidacji)</label></div>{result?.solution ? <><div className="arc-list">{result.solution.arcs.map(arc => <div key={String(arc.highArc)}><b>{arc.highArc ? 'Wysoki tor' : 'Niski tor'}</b><strong>{arc.elevationDeg.toFixed(2)}°</strong><span>{arc.flightTime.toFixed(2)} s</span></div>)}</div><p>Rozrzut 95% pojedynczego pocisku: {result.solution.dispersionRadius95 === null ? 'brak danych' : `${result.solution.dispersionRadius95.toFixed(1)} ${s.unit}`}. Salwa: {profile?.salvoSize} pocisków. Niezależność trafień niepotwierdzona.</p><ul className="assumptions">{result.solution.assumptions.map(a => <li key={a}>{a}</li>)}</ul></> : <p className="hint">Brak rozwiązania: {warning || result?.ballisticWarning || 'wymagane dane wejściowe'}.</p>}{result?.lead && <p>Wyprzedzenie: wschód {result.lead[0].toFixed(1)}, północ {result.lead[1].toFixed(1)} {s.unit}.</p>}{result?.leadWarning && <p className="hint">{result.leadWarning}</p>}</section>
          <section className="panel"><h2>Zakres modułów</h2><div className="module-list">{profiles.map(p => <div key={p.id}><span>{p.name}</span><small>{profileFor(profiles, p.id, s.overrides)?.calibrated ? 'Beta / kalibracja lokalna' : p.sourceMetadata ? 'Eksperymentalny / źródło community' : p.category === 'guided_missile' ? 'Brak modelu naprowadzania' : 'Brak stałych — gotowy szablon'}</small></div>)}</div></section>
        </>}
        {tab === 'direct' && <DirectFirePanel profiles={profiles} overrides={s.overrides} quick={quickAim} windowHandle={windowHandle} backend={s.captureBackend}
          onSaveProfile={p => { setProfiles(old => old.some(x => x.id === p.id) ? old : [...old, p]); update({ overrides: { ...settingsRef.current.overrides, [p.id]: p } }); }}
          log={log} notice={setNotice} onCard={setDirectCard} />}
        {tab === 'log' && <>
          <section className="panel"><h2>Historia rozwiązań</h2>{s.history.length ? <div className="history-list">{[...s.history].reverse().map((h, i) => <div key={`${h.at}-${i}`}><time>{new Date(h.at).toLocaleString('pl-PL')}</time><b>{h.distance.toFixed(1)} {h.unit??'jedn. ze starego zapisu'} / {h.azimuth.toFixed(1)}°</b><span>{h.note}</span></div>)}</div> : <p className="hint">Na mapie użyj „Zapisz rozwiązanie”.</p>}<button onClick={() => void attempt(() => saveJson('mtc-historia.json', { schemaVersion: 1, history: s.history, calibrationLog: s.calibrationLog }))}>Eksportuj historię</button></section><section className="panel"><h2>Log kalibracji</h2><div className="history-list">{[...s.calibrationLog].reverse().map((h, i) => <div key={`${h.at}-${i}`}><time>{new Date(h.at).toLocaleString('pl-PL')}</time><span>{h.event}</span></div>)}</div></section><section className="panel"><h2>Status techniczny</h2><p>Windows / Tauri 2 / Rust / systemowy OCR / Windows Graphics Capture lub jawny GDI</p><p>Odczyt ekranu: {running ? 'aktywny' : 'zatrzymany'} · zapis klatek: wyłączony · limit OCR: {s.fps} FPS.</p><p className="hint">Tryb gry: okno lub borderless. Ekskluzywny pełny ekran, zminimalizowane okno i zasłonięty HUD mogą uniemożliwić odczyt. Reguły MTC dotyczące nakładek nie są potwierdzone; aplikacja nie deklaruje zgodności ani ochrony przed blokadą.</p><p className="hint">Pozycje Automatic wymagają deskryptora widoku mapy, kalibracji i własnych wzorców markerów. Brak potwierdzenia aktualnego MTC nie jest zastępowany parametrami prawdziwego uzbrojenia.</p></section>
        </>}
        <footer className="app-footer"><span>Obserwacje OCR · dane ręczne · wyniki obliczeń opisane osobno</span><span>MTC TA / 0.5</span></footer>
      </main>
    </div>
  </div>;
}
