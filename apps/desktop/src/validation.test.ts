import { describe, it, expect } from 'vitest';
import { defaultSettings, type Observation, type Profile } from './types';
import { validateSettings, validateProfile, fresh, freshBatch, stabilize } from './validation';
const obs: Observation = { kind: 'heading', value: 138, timestampMs: 1000, unit: '°', text: '138', confidence: 0.6, source: 'OCR' };
describe('wiarygodność odczytu', () => {
  it('unieważnia batch już przy zmianie źródła lub kalibracji',()=>{
    const batch={timestampMs:1000,backend:'wgc'};
    expect(freshBatch(batch,'window1-scale1','window1-scale1',true,1200)).toBe(true);
    expect(freshBatch(batch,'window1-scale1','window1-scale2',true,1200)).toBe(false);
    expect(freshBatch(batch,'window1-scale1','window2-scale1',true,1200)).toBe(false);
    expect(freshBatch(batch,'same','same',false,1200)).toBe(false);
    expect(freshBatch({...batch,backend:'file'},'same','same',true,1200)).toBe(false);
  });
  it('odrzuca pomiar stary i z przyszłości', () => { expect(fresh(obs, 4000)).toBe(false); expect(fresh(obs, 0)).toBe(false); expect(fresh(obs, 2000)).toBe(true); });
  it('wymaga dwóch zgodnych odczytów', () => { expect(stabilize({ ...obs, timestampMs: 1500 }, obs).confidence).toBe(0.9); expect(stabilize({ ...obs, value: 200, timestampMs: 1500 }, obs).confidence).toBe(0.6); });
  it('obsługuje przejście kompasu przez północ', () => { expect(stabilize({ ...obs, value: 0, timestampMs: 1500 }, { ...obs, value: 359 }).confidence).toBe(0.9); });
});
describe('ustawienia i import oznaczeń', () => {
  it('odrzuca brak skali zastąpiony zerem', () => { expect(() => validateSettings({ ...defaultSettings, scale: 0 })).toThrow(); });
  it('pozwala na uczciwy stan bez danych', () => { expect(validateSettings(defaultSettings).scale).toBeNull(); });
  it('odrzuca punkt poza mapą i ROI poza ekranem', () => {
    expect(() => validateSettings({ ...defaultSettings, target: { x: 900, y: 1 } })).toThrow();
    expect(() => validateSettings({ ...defaultSettings, roi: { x: 0.9, y: 0, width: 0.2, height: 0.1 } })).toThrow();
  });
  it('odrzuca uszkodzony zapis profilu przed renderowaniem', () => { expect(() => validateSettings({ ...defaultSettings, overrides: { bm21: { source: null } } })).toThrow(); });
  it('migruje ustawienia v1 bez automatycznego przyznawania źródeł', () => {
    const migrated=validateSettings({...defaultSettings,schemaVersion:1,kind:'heading',roi:{x:0.4,y:0.1,width:0.1,height:0.04}});
    expect(migrated.schemaVersion).toBe(4);expect(migrated.hudRegions.find(r=>r.id==='heading')?.enabled).toBe(true);
    expect(migrated.markerConfig.coordinateCalibrationConfirmed).toBe(false);expect(migrated.markerConfig.viewSignature).toBeNull();
  });
  it('odrzuca ROI z powielonym ID i ułamkowe powiększenie',()=>{
    const r=defaultSettings.hudRegions[0];
    expect(()=>validateSettings({...defaultSettings,hudRegions:[r,r]})).toThrow();
    expect(()=>validateSettings({...defaultSettings,hudRegions:[{...r,upscale:1.5}]})).toThrow();
  });
  it('odrzuca progi markerów poza zakresem natywnym',()=>{
    const markerConfig={...defaultSettings.markerConfig,signatures:[{...defaultSettings.markerConfig.signatures[0],tolerance:101}]};
    expect(()=>validateSettings({...defaultSettings,markerConfig})).toThrow();
  });
  it('migruje v2 z wyłączoną nową automatyką i pustą biblioteką',()=>{
    const {autoHud:_,autoMap:__,maps:___,terrain:____,...legacy}=defaultSettings;
    const migrated=validateSettings({...legacy,schemaVersion:2});
    expect(migrated.autoHud).toBe(false);expect(migrated.autoMap).toBe(false);expect(migrated.maps).toEqual([]);expect(migrated.terrain).toBeNull();
  });
  it('odrzuca bibliotekę bez skali i osieroconą mapę wysokości',()=>{
    expect(()=>validateSettings({...defaultSettings,maps:[{id:'test',path:'map.png'}]})).toThrow();
    expect(()=>validateSettings({...defaultSettings,terrain:{path:'height.png',mapId:'missing',minHeight:0,maxHeight:10}})).toThrow();
  });
});


// Prior releases migrate without inventing terrain metadata.
it('migrates version three to a multiple-terrain library',()=>{
  const older={...defaultSettings,schemaVersion:3};delete (older as Partial<typeof defaultSettings>).terrains;
  const current=validateSettings(older);
  expect(current.schemaVersion).toBe(4);expect(current.terrains).toEqual([]);
});

describe('zgodność walidacji profilu z Rust', () => {
  it('akceptuje cały wbudowany katalog, w tym jawny rozrzut 9M22', async () => {
    const { default: community } = await import('../../../data/weapons/community-profiles.json');
    const { default: builtin } = await import('../../../data/weapons/builtin.json');
    for (const p of [...builtin, ...community]) expect(validateProfile(p).id).toBe(p.id);
  });
  it('odrzuca nieznane pole i niekompletne pochodzenie jak deny_unknown_fields/SourceMetadata::validate', async () => {
    const { default: community } = await import('../../../data/weapons/community-profiles.json');
    const p = community[0] as unknown as Profile;
    expect(() => validateProfile({ ...p, inventedDrag: 0.1 })).toThrow(/Nieznane pole/);
    for (const patch of [{ experienceId: undefined }, { experienceId: 'abc' }, { fetchedAt: ' ' }, { urls: [] }, { repository: 'http://example.org' }]) {
      expect(() => validateProfile({ ...p, sourceMetadata: { ...p.sourceMetadata!, ...patch } })).toThrow(/pochodzenie/);
    }
  });
  it('traktuje brak opcjonalnych liczb jak null, ale odrzuca błędny rozrzut salwy', () => {
    const p = { ...defaultProfile, dispersionSigma: undefined } as unknown as Profile;
    expect(validateProfile(p).id).toBe('t');
    expect(() => validateProfile({ ...defaultProfile, salvo: { unit: 'game_meters', status: 'no_data' } } as unknown as Profile)).toThrow(/Jednostka/);
  });
});
const defaultProfile: Profile = { schemaVersion: 1, id: 't', name: 'T', category: 'tube_artillery', gameVersion: 'x', status: 'no_data', source: 's', calibrated: false, speed: null, gravity: null, minElevationDeg: 0, maxElevationDeg: 45, dispersionSigma: null, salvoSize: 1, notes: '' };
