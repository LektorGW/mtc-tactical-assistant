import { validateDynamics } from './dynamics-validation';
import { validateSalvo } from './salvo';
import { validateObservedSpecs, validateReticle } from './direct-fire';
import { defaultSettings, type Settings, type Observation, type Point, type Profile } from './types';

export function fresh(o: Observation | null, now: number, maxAge = 2000): boolean {
  return !!o && Number.isFinite(o.timestampMs) && o.timestampMs <= now && now - o.timestampMs <= maxAge;
}
export function freshBatch(batch: {timestampMs:number;backend:string}|null, acceptedScope:string, currentScope:string, running:boolean, now:number):boolean {
  return running&&!!batch&&batch.backend!=='file'&&acceptedScope===currentScope&&Number.isFinite(batch.timestampMs)&&batch.timestampMs<=now&&now-batch.timestampMs<=2000;
}
export function stabilize(current: Observation, previous: Observation | null): Observation {
  let confidence = 0.6;
  if (previous?.kind === current.kind && fresh(previous, current.timestampMs)) {
    const delta = Math.abs(current.value - previous.value);
    const same = current.kind === 'timer' ? delta <= 2 : current.kind === 'heading' ? Math.min(delta, 360 - delta) <= 1 : delta < 0.01;
    if (same) confidence = 0.9;
  }
  return { ...current, confidence };
}
const isPoint = (p: unknown, width: number, height: number): p is Point => {
  const q = p as Point;
  return !!q && Number.isFinite(q.x) && Number.isFinite(q.y) && q.x >= 0 && q.y >= 0 && q.x <= width && q.y <= height;
};
// Same field set as Rust `WeaponProfile` (`deny_unknown_fields`).
const profileFields = new Set(['schemaVersion','id','name','category','gameVersion','status','source','calibrated','speed','gravity','minElevationDeg','maxElevationDeg','dispersionSigma','salvoSize','notes','unit','sourceMetadata','sourceAcknowledged','dynamics','salvo','observedSpecs','reticle']);
const missing = (n: unknown) => n === null || n === undefined;
export function validateProfile(value: unknown): Profile {
  const p = value as Profile;
  if (p && typeof p === 'object' && Object.keys(p).some(key => !profileFields.has(key))) throw new Error('Nieznane pole profilu — Rust odrzuciłby ten profil');
  if (!p || p.schemaVersion !== 1 || typeof p.id !== 'string' || !p.id.trim()
    || typeof p.name !== 'string' || !p.name.trim() || typeof p.source !== 'string' || !p.source.trim()
    || typeof p.gameVersion !== 'string' || !p.gameVersion.trim() || typeof p.calibrated !== 'boolean'
    || !['rocket_artillery','tube_artillery','tank','helicopter_gun','helicopter_rocket','guided_missile','anti_aircraft','direct_fire'].includes(p.category)
    || !['no_data','experimental','beta','verified'].includes(p.status)
    || [p.speed, p.gravity].some(n => !missing(n) && (!Number.isFinite(n) || (n as number) <= 0))
    || (!missing(p.dispersionSigma) && (!Number.isFinite(p.dispersionSigma) || (p.dispersionSigma as number) < 0))
    || !Number.isFinite(p.minElevationDeg) || !Number.isFinite(p.maxElevationDeg) || p.minElevationDeg < -89 || p.maxElevationDeg > 89 || p.minElevationDeg > p.maxElevationDeg
    || !Number.isInteger(p.salvoSize) || p.salvoSize < 1 || p.salvoSize > 1000
    || typeof p.notes !== 'string' || (p.calibrated && (!p.speed || !p.gravity || p.category === 'guided_missile'))) {
    throw new Error('Uszkodzony profil w ustawieniach — wymagane poprawne źródło i parametry');
  }
  if (p.unit !== undefined && !['game_units','game_meters'].includes(p.unit)) throw new Error('Niepoprawna jednostka profilu');
  if (p.sourceAcknowledged !== undefined && typeof p.sourceAcknowledged !== 'boolean') throw new Error('Niepoprawny stan źródła');
  if (p.sourceMetadata) {
    const m = p.sourceMetadata;
    if (typeof m.repository !== 'string' || !m.repository.startsWith('https://') || typeof m.commit !== 'string' || !/^[a-f0-9]{40}$/i.test(m.commit)
      || typeof m.fetchedAt !== 'string' || !m.fetchedAt.trim() || typeof m.experienceId !== 'string' || !/^[0-9]+$/.test(m.experienceId)
      || typeof m.license !== 'string' || !m.license.trim() || !Array.isArray(m.urls) || !m.urls.length || m.urls.some(url => typeof url !== 'string' || !url.startsWith('https://')))
      throw new Error('Niepoprawne pochodzenie profilu');
  }
  if(p.dynamics!==undefined)validateDynamics(p.dynamics,p);
  const salvo=(p as Profile & { salvo?: unknown }).salvo;
  if(salvo!==undefined)validateSalvo(salvo,p);
  validateObservedSpecs(p.observedSpecs,p.speed,p.calibrated);
  validateReticle(p.reticle);
  return p;
}
export function validateSettings(value: unknown): Settings {
  if (!value || typeof value !== 'object') throw new Error('Niepoprawne ustawienia');
  const raw = value as Record<string, unknown>;
  if (![1,2,3,4].includes(Number(raw.schemaVersion))) throw new Error('Nieobsługiwana wersja ustawień');
  const s = { ...defaultSettings, ...raw, schemaVersion: 4 } as Settings;
  if (raw.schemaVersion === 1) {
    s.hudRegions = defaultSettings.hudRegions.map(r => ({ ...r, enabled: r.kind === s.kind, roi: r.kind === s.kind ? s.roi : r.roi }));
  }
  if (!s || s.schemaVersion !== 4 || typeof s.selected !== 'string' || !s.selected || typeof s.unit !== 'string'
    || typeof s.mapName !== 'string' || (s.mapPath !== null && typeof s.mapPath !== 'string')
    || !Number.isFinite(s.mapWidth) || !Number.isFinite(s.mapHeight)
    || s.mapWidth < 1 || s.mapHeight < 1 || s.mapWidth > 8192 || s.mapHeight > 8192
    || (s.scale !== null && (!Number.isFinite(s.scale) || s.scale <= 0))
    || !Number.isFinite(s.rotation) || !Number.isFinite(s.uncertainty) || s.uncertainty < 0
    || !['manual', 'assisted', 'automatic'].includes(s.mode)
    || (s.own !== null && !isPoint(s.own, s.mapWidth, s.mapHeight))
    || (s.target !== null && !isPoint(s.target, s.mapWidth, s.mapHeight))) {
    throw new Error('Niepoprawne ustawienia lub oznaczenia mapy');
  }
  const r = s.roi;
  if (!r || [r.x, r.y, r.width, r.height].some(n => !Number.isFinite(n)) || r.x < 0 || r.y < 0 || r.width <= 0 || r.height <= 0 || r.x + r.width > 1.000001 || r.y + r.height > 1.000001) {
    throw new Error('Obszar OCR wychodzi poza obraz');
  }
  if (!Number.isFinite(s.fps) || s.fps < 0.25 || s.fps > 3 || !['timer', 'heading', 'speed', 'altitude'].includes(s.kind)) throw new Error('Niepoprawna konfiguracja OCR');
  if (!s.overrides || typeof s.overrides !== 'object' || Array.isArray(s.overrides)) throw new Error('Niepoprawna lista profili');
  for (const [key, p] of Object.entries(s.overrides)) if (validateProfile(p).id !== key) throw new Error('Identyfikator profilu nie zgadza się z ustawieniami');
  if (!['game_units','game_meters'].includes(s.mapUnit) || !['wgc','gdi'].includes(s.captureBackend)
    || !Number.isFinite(s.captureReferenceWidth) || !Number.isFinite(s.captureReferenceHeight)
    || s.captureReferenceWidth < 0 || s.captureReferenceHeight < 0 || s.captureReferenceWidth > 8192 || s.captureReferenceHeight > 8192
    || (s.captureReferenceWidth === 0) !== (s.captureReferenceHeight === 0)) throw new Error('Niepoprawna konfiguracja przechwytywania');
  if (!Array.isArray(s.hudRegions) || s.hudRegions.length > 12 || new Set(s.hudRegions.map(r => r.id)).size !== s.hudRegions.length) throw new Error('Niepoprawna lista obszarów HUD');
  for (const r of s.hudRegions) {
    if (typeof r.id !== 'string' || !r.id || !['timer','heading','altitude','speed','vehicleName','weaponName','mapName','text'].includes(r.kind) || typeof r.enabled !== 'boolean' || !Number.isInteger(r.upscale) || r.upscale < 1 || r.upscale > 4) throw new Error('Niepoprawny obszar HUD');
    validateRoi(r.roi);
  }
  const m = s.markerConfig;
  if (!m || typeof m.enabled !== 'boolean' || typeof m.coordinateCalibrationConfirmed !== 'boolean' || !Number.isFinite(m.aspectTolerance) || m.aspectTolerance < 0 || m.aspectTolerance > 0.2 || !Array.isArray(m.signatures) || m.signatures.length > 8 || !Number.isInteger(m.mapWidth) || !Number.isInteger(m.mapHeight) || m.mapWidth < 1 || m.mapHeight < 1 || m.mapWidth * m.mapHeight > 64_000_000 || new Set(m.signatures.map(sig=>sig.id)).size!==m.signatures.length) throw new Error('Niepoprawna konfiguracja markerów');
  validateRoi(m.mapRoi);
  for (const sig of m.signatures) {
    if (!['own','target','reference','unassigned'].includes(sig.role) || !Array.isArray(sig.rgb) || sig.rgb.length !== 3 || sig.rgb.some(n => !Number.isInteger(n) || n < 0 || n > 255)
      || typeof sig.id!=='string' || !sig.id || !Number.isInteger(sig.tolerance) || sig.tolerance < 0 || sig.tolerance > 100 || !Number.isInteger(sig.minComponentPixels) || !Number.isInteger(sig.maxComponentPixels) || sig.minComponentPixels <= 0 || sig.maxComponentPixels < sig.minComponentPixels || sig.maxComponentPixels > 1_000_000
      || !Number.isFinite(sig.minAspectRatio) || !Number.isFinite(sig.maxAspectRatio) || sig.minAspectRatio<=0 || sig.maxAspectRatio<sig.minAspectRatio || !Number.isFinite(sig.minFillRatio) || sig.minFillRatio<0 || sig.minFillRatio>1)
      throw new Error('Niepoprawny wzorzec kolorystyczny markera');
  }
  if (!Number.isFinite(s.maxTrackingSpeed) || s.maxTrackingSpeed<=0 || s.maxTrackingSpeed>10_000) throw new Error('Niepoprawny limit skoku trackera');
  if ([s.autoHud,s.autoMap,s.autoWeapon].some(v=>typeof v!=='boolean') || !Array.isArray(s.maps) || s.maps.length>32 || new Set(s.maps.map(m=>m.id)).size!==s.maps.length) throw new Error('Niepoprawne ustawienia automatyki');
  for (const entry of s.maps) {
    if (!entry || typeof entry.sha256!=='string' || !/^[a-f0-9]{64}$/i.test(entry.sha256) || typeof entry.id!=='string' || !entry.id.trim() || typeof entry.name!=='string' || !entry.name.trim() || typeof entry.path!=='string' || !entry.path.trim() || typeof entry.source!=='string' || !entry.source.trim() || typeof entry.confirmed!=='boolean'
      || !['game_units','game_meters'].includes(entry.unit) || !Number.isInteger(entry.width) || !Number.isInteger(entry.height) || entry.width<1 || entry.height<1 || entry.width>8192 || entry.height>8192) throw new Error('Niepoprawna mapa w bibliotece');
    const c=entry.calibration;
    if (!c || !Number.isFinite(c.unitsPerPixel) || c.unitsPerPixel<=0 || !Number.isFinite(c.northRotationDeg) || !Number.isFinite(c.uncertaintyUnits) || c.uncertaintyUnits<0 || !isPoint(c.originPixel,entry.width,entry.height) || !c.originWorld || !Number.isFinite(c.originWorld.x) || !Number.isFinite(c.originWorld.y)) throw new Error('Niepoprawna kalibracja mapy w bibliotece');
  }
  if(!Array.isArray(s.terrains)||s.terrains.length>32||new Set(s.terrains.map(t=>t.mapId)).size!==s.terrains.length)throw new Error('Niepoprawna biblioteka wysokości');
  for(const t of [...s.terrains,...(s.terrain?[s.terrain]:[])]) {
    if(typeof t.sha256!=='string' || !/^[a-f0-9]{64}$/i.test(t.sha256) || typeof t.path!=='string' || !t.path.trim() || typeof t.mapId!=='string' || !s.maps.some(m=>m.id===t.mapId&&m.width===t.mapWidth&&m.height===t.mapHeight&&m.unit===t.unit) || typeof t.source!=='string' || !t.source.trim()
      || !Number.isFinite(t.minHeight) || !Number.isFinite(t.maxHeight) || t.maxHeight<=t.minHeight || typeof t.confirmed!=='boolean' || typeof t.coordinateAlignmentConfirmed!=='boolean') throw new Error('Mapa wysokości wymaga zgodnej mapy, jednostek i zakresu');
  }
  if (m.viewSignature) {
    const sig=m.viewSignature;
    if (sig.schemaVersion!==1 || sig.gridSize!==16 || !Array.isArray(sig.graySamples) || sig.graySamples.length!==256 || sig.graySamples.some(n=>!Number.isInteger(n)||n<0||n>255) || !Number.isFinite(sig.textureRms) || !Number.isFinite(sig.aspectRatio) || sig.aspectRatio<=0 || !Number.isFinite(sig.threshold) || sig.threshold<0.85 || sig.threshold>1) throw new Error('Niepoprawny wzorzec widoku mapy');
  }
  return { ...defaultSettings, ...s, history: Array.isArray(s.history) ? s.history.slice(-50) : [], calibrationLog: Array.isArray(s.calibrationLog) ? s.calibrationLog.slice(-100) : [], overrides: s.overrides ?? {} };
}

export function validateRoi(r: Settings['roi']) {
  if (!r || [r.x,r.y,r.width,r.height].some(n => !Number.isFinite(n)) || r.x < 0 || r.y < 0 || r.width <= 0 || r.height <= 0 || r.x+r.width > 1.000001 || r.y+r.height > 1.000001) throw new Error('Obszar OCR wychodzi poza obraz');
}

