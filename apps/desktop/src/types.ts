import type { SalvoDispersion } from './salvo';
import type { ObservedSpecs, ReticleMap } from './direct-fire';
export type Point = { x: number; y: number };
export type Unit = 'game_units' | 'game_meters';
export type SourceMetadata = { repository: string; commit: string; fetchedAt: string; license: string; urls: string[]; experienceId?: string };
export type Category = 'rocket_artillery' | 'tube_artillery' | 'tank' | 'helicopter_gun' | 'helicopter_rocket' | 'guided_missile' | 'anti_aircraft' | 'direct_fire';
export type ProjectileDynamics = {model:'vacuum'|'drag'|'rocket';unit:Unit;linearDrag:number|null;quadraticDrag:number|null;rocketAcceleration:number|null;burnTime:number|null;thrustDirectionFixedLaunch:boolean|null;calibrated:boolean;status:'no_data'|'experimental'|'beta';source:string};
export type Profile = {
  schemaVersion: number; id: string; name: string; category: Category; gameVersion: string;
  status: string; source: string; calibrated: boolean; speed: number | null; gravity: number | null;
  minElevationDeg: number; maxElevationDeg: number; dispersionSigma: number | null; salvoSize: number; notes: string;
  unit?: Unit; sourceAcknowledged?: boolean; sourceMetadata?: SourceMetadata | null;
  dynamics?: ProjectileDynamics | null;
  /** Salvo dispersion model (Rust `SalvoDispersion`); absent = no data. */
  salvo?: SalvoDispersion | null;
  /** Speed read from the game UI / typed by the player (Rust `ObservedSpecs`). */
  observedSpecs?: ObservedSpecs | null;
  /** Player-defined sight graduations (Rust `ReticleMap`), status unverified by default. */
  reticle?: ReticleMap | null;
};
export type Calibration = { unitsPerPixel: number; northRotationDeg: number; originPixel: Point; originWorld: Point; uncertaintyUnits: number };
export type Roi = { x: number; y: number; width: number; height: number };
export type Observation = { kind: string; value: number; unit: string; text: string; confidence: number; timestampMs: number; source: string };
export type Analysis = { observation: Observation | null; text: string; preview: string; elapsedMs: number; timestampMs: number; width: number; height: number };
export type HudRegion = { id: string; kind: string; roi: Roi; upscale: number; enabled: boolean };
export type MarkerSignature = { id: string; role: 'own' | 'target' | 'reference' | 'unassigned'; rgb: [number, number, number]; tolerance: number; minComponentPixels: number; maxComponentPixels: number; minAspectRatio: number; maxAspectRatio: number; minFillRatio: number };
export type MapViewSignature = { schemaVersion: number; gridSize: number; graySamples: number[]; textureRms: number; aspectRatio: number; threshold: number };
export type MarkerConfig = { enabled: boolean; mapRoi: Roi; mapWidth: number; mapHeight: number; coordinateCalibrationConfirmed: boolean; aspectTolerance: number; signatures: MarkerSignature[]; viewSignature: MapViewSignature | null };
export type VisionConfig = { schemaVersion: number; referenceWidth: number; referenceHeight: number; aspectTolerance: number; maxAgeMs: number; ocrRegions: Omit<HudRegion, 'enabled'>[]; markerConfig: MarkerConfig | null };
export type Marker = { signatureId: string; role: string; mapPosition: Point; framePositionNormalized: Point; boundingBox: Roi; pixelCount: number; confidence: number; timestampMs: number; source: string };
export type Motion = { status: string; velocity: [number, number] | null; speedUnitsPerSecond: number | null; positionWorld: Point | null; count: number; confidence: number; timestampMs: number; maxPredictionSeconds: number; assumptions: string[]; warning: string | null };
export type TextObservation = { kind: string; text: string; confidence: number; timestampMs: number; source: string; regionId?: string };
export type BatchAnalysis = { analysis: { status: string; numericObservations: Observation[]; textObservations: TextObservation[]; markers: Marker[]; ocrRegions: { id: string; kind: string; status: string; warning: string | null; text: string; observation: Observation | null }[]; markerWarning: string | null; warnings: string[] }; configuration:VisionConfig; preview: string | null; elapsedMs: number; timestampMs: number; width: number; height: number; backend: 'wgc' | 'gdi' | 'file'; targetMotion: Motion | null; ownMotion: Motion | null; automation?: AutomationReport };
export type CommunityMap = { schemaVersion: number; id: string; name: string; key: string; sizeStuds: number; sizeGameMeters: number; unit: Unit; status: string; inRotation: boolean; sourceMetadata: SourceMetadata; heightRangeStuds?: { minimum: number; maximum: number } | null };
export type HeightSource = 'terrain' | 'manual';
export type Result = { heightDelta?: number | null; heightSource?: HeightSource | null; bearing: { distance: number; azimuthDeg: number; uncertaintyUnits: number }; solution: { arcs: { elevationDeg: number; flightTime: number; highArc: boolean }[]; dispersionRadius95: number | null; assumptions: string[] } | null; ballisticWarning: string | null; lead: [number, number] | null; leadWarning: string | null; intercept3d?: Intercept3d | null; dynamicSolution?: {arcs:{elevationDeg:number;azimuthDeg:number;flightTime:number;missDistance:number;highArc:boolean}[];unit:Unit;minimumResidualUnits:number;stepsUsed:number;evaluations:number;assumptions:string[]}|null };
export type StoredMap = { sha256: string; id: string; name: string; path: string; width: number; height: number; unit: Unit; source: string; confirmed: boolean; calibration: Calibration };
export type StoredTerrain = { sha256: string; path: string; mapId: string; mapWidth: number; mapHeight: number; minHeight: number; maxHeight: number; unit: Unit; source: string; confirmed: boolean; coordinateAlignmentConfirmed: boolean };
export type HudDiscovery = { status: string; timestampMs: number; referenceWidth: number; referenceHeight: number; regions: { region: Omit<HudRegion, 'enabled'>; confidence: number; text: string; evidence: string[] }[]; warnings: string[]; elapsedMs: number };
/** `equivalents`: optional ballistic-equivalence group from the resolver (shape tolerated: IDs or identities). */
export type WeaponResolution = { status: string; profileId: string | null; confidence: number; candidates: { profileId: string; vehicle: string; projectile: string | null; category: Category }[]; evidence: string[]; equivalents?: (string | { profileId?: string; vehicle?: string; projectile?: string | null; name?: string })[] };
export type AutomationReport = { hud: HudDiscovery | null; hudApplied: boolean; map: { status: string; warning: string | null; best: { mapId: string; score: number; coverageRatio: number } | null } | null; mapName: { status: string; mapId: string | null } | null; activeMap: StoredMap | null; weapon: WeaponResolution | null; warnings: string[] };
export type Intercept3d = { arcs: { flightTime: number; elevationDeg: number; azimuthDeg: number; lead: [number,number,number]; missDistance: number }[]; unit: Unit; assumptions: string[] };
export type GridColumnStyle = 'letters-no-i' | 'letters' | 'numbers';
/** Display grid only; labels are unconfirmed against the live game. */
export type MapGrid = { visible: boolean; divisions: number; columns: GridColumnStyle; rowsFrom: 'top' | 'bottom' };
export type MapSource = 'catalog' | 'custom';
export type AssetsStatus = { maps: string[]; terrains: string[]; warnings: string[]; terrainBytes: number };
export type CatalogMapResult = { map: StoredMap; terrain: StoredTerrain | null; terrainInSource: boolean; origin: 'cache' | 'downloaded'; sizeGameMeters: number; warnings: string[] };
export type Settings = {
  schemaVersion: 4; selected: string; mode: 'manual' | 'assisted' | 'automatic';
  scale: number | null; rotation: number; uncertainty: number; unit: string;
  own: Point | null; target: Point | null; mapWidth: number; mapHeight: number;
  mapPath: string | null; mapName: string; overrides: Record<string, Profile>;
  roi: Roi; kind: string; fps: number; history: { at: string; distance: number; azimuth: number; note: string; unit?:string; mapUnit?:Unit }[];
  calibrationLog: { at: string; event: string }[];
  mapUnit: Unit; communityMapId: string | null;
  captureBackend: 'wgc' | 'gdi'; captureReferenceWidth: number; captureReferenceHeight: number;
  hudRegions: HudRegion[]; markerConfig: MarkerConfig; autoWeapon: boolean;
  maxTrackingSpeed: number;
  autoHud: boolean; autoMap: boolean; maps: StoredMap[]; terrain: StoredTerrain | null;
  terrains: StoredTerrain[];
  /** Map tab mode: community catalog (one click) or own image + two-point calibration. */
  mapSource: MapSource;
  /** One-time acceptance of the experimental community map source and local download. */
  catalogSourceAccepted: boolean;
  mapGrid: MapGrid;
  /** Use a loaded heightmap automatically when it matches the current map. */
  useTerrainHeight: boolean;
  /** Cell size read by the user from the in-game map (bottom left, key M); comparison only. */
  gameGridSize: number | null;
};
export const defaultSettings: Settings = {
  schemaVersion: 4, selected: 'bm21', mode: 'manual', scale: null, rotation: 0, uncertainty: 0,
  unit: 'jedn. gry', own: null, target: null, mapWidth: 800, mapHeight: 600,
  mapPath: null, mapName: 'Mapa niezaimportowana', overrides: {},
  roi: { x: 0.43, y: 0.01, width: 0.14, height: 0.09 }, kind: 'timer', fps: 1, history: [], calibrationLog: [],
  mapUnit: 'game_units', communityMapId: null, captureBackend: 'wgc', captureReferenceWidth: 0, captureReferenceHeight: 0,
  hudRegions: [
    { id: 'timer', kind: 'timer', roi: { x: 0.43, y: 0.01, width: 0.14, height: 0.09 }, upscale: 2, enabled: true },
    { id: 'heading', kind: 'heading', roi: { x: 0.475, y: 0.09, width: 0.05, height: 0.035 }, upscale: 2, enabled: false },
    { id: 'speed', kind: 'speed', roi: { x: 0.85, y: 0.85, width: 0.08, height: 0.04 }, upscale: 2, enabled: false },
    { id: 'altitude', kind: 'altitude', roi: { x: 0.85, y: 0.78, width: 0.08, height: 0.04 }, upscale: 2, enabled: false },
    { id: 'vehicle', kind: 'vehicleName', roi: { x: 0.02, y: 0.90, width: 0.25, height: 0.06 }, upscale: 2, enabled: false },
    { id: 'ammunition', kind: 'weaponName', roi: { x: 0.02, y: 0.83, width: 0.25, height: 0.06 }, upscale: 2, enabled: false },
    { id: 'map-name', kind: 'mapName', roi: { x: 0.35, y: 0.18, width: 0.30, height: 0.06 }, upscale: 2, enabled: false },
  ],
  markerConfig: { enabled: false, mapRoi: { x: 0, y: 0.65, width: 0.25, height: 0.30 }, mapWidth: 800, mapHeight: 600, coordinateCalibrationConfirmed: false, viewSignature: null, aspectTolerance: 0.03, signatures: [
    { id: 'own', role: 'own', rgb: [0, 200, 255], tolerance: 45, minComponentPixels: 4, maxComponentPixels: 300, minAspectRatio: 0.25, maxAspectRatio: 4, minFillRatio: 0.2 },
    { id: 'target', role: 'target', rgb: [255, 50, 50], tolerance: 45, minComponentPixels: 4, maxComponentPixels: 300, minAspectRatio: 0.25, maxAspectRatio: 4, minFillRatio: 0.2 },
  ] }, autoWeapon: false, maxTrackingSpeed: 500, autoHud: false, autoMap: false, maps: [], terrain: null, terrains: [],
  mapSource: 'catalog', catalogSourceAccepted: false,
  mapGrid: { visible: true, divisions: 9, columns: 'letters-no-i', rowsFrom: 'top' },
  useTerrainHeight: true, gameGridSize: null,
};

