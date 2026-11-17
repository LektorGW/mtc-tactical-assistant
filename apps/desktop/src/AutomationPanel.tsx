import { useState, type FormEvent } from 'react';
import { call, pickImage } from './api';
import type { BatchAnalysis, Profile, Settings, StoredTerrain } from './types';
import { parseCalibrationCsv, type CalibrationFit } from './calibration';

type Props = { settings: Settings; update: (patch: Partial<Settings>)=>void; analysis: BatchAnalysis|null; running: boolean; profile: Profile|undefined; registerMap: (source: string)=>Promise<void>; notice: (text: string)=>void; applyFit: (fit: CalibrationFit)=>void };
export function AutomationPanel({settings:s,update,analysis,running,profile,registerMap,notice,applyFit}:Props) {
  const [mapSource,setMapSource]=useState('');
  const [mapConfirmed,setMapConfirmed]=useState(false);
  const [terrainSource,setTerrainSource]=useState('');
  const [minimum,setMinimum]=useState('');
  const [maximum,setMaximum]=useState('');
  const [terrainMap,setTerrainMap]=useState(s.maps[0]?.id??'');
  const [terrainConfirmed,setTerrainConfirmed]=useState(false);
  const [samples,setSamples]=useState('');
  const [measurementSource,setMeasurementSource]=useState('');
  const [gameVersion,setGameVersion]=useState('');
  const [fit,setFit]=useState<CalibrationFit|null>(null);
  const [busy,setBusy]=useState(false);
  const report=running&&analysis&&analysis.timestampMs<=Date.now()&&Date.now()-analysis.timestampMs<=2000?analysis.automation:null;
  const attempt=async(task:()=>Promise<void>)=>{setBusy(true);try{await task();}catch(e){notice(String(e));}finally{setBusy(false);}};
  const fitSamples=(e:FormEvent)=>{e.preventDefault();void attempt(async()=>{
    setFit(null);if(!profile)throw new Error('Wybierz profil');
    const values=parseCalibrationCsv(samples,s.mapUnit,measurementSource);
    const result=await call<CalibrationFit>('fit_weapon',{profile,samples:values,options:{unit:s.mapUnit,maxResidualUnits:2,maxRelativeResidual:0.03,minSamples:3,source:measurementSource,gameVersion:gameVersion||null}});
    setFit(result);notice('Dopasowano model. Sprawdź błędy pomiarów przed zastosowaniem.');
  });};
  const importTerrain=(e:FormEvent)=>{e.preventDefault();void attempt(async()=>{
    const map=s.maps.find(m=>m.id===terrainMap);if(!map)throw new Error('Wybierz mapę z biblioteki');
    const minHeight=Number(minimum),maxHeight=Number(maximum);
    if(!minimum.trim()||!maximum.trim()||!Number.isFinite(minHeight)||!Number.isFinite(maxHeight)||minHeight>=maxHeight||!terrainConfirmed)throw new Error('Podaj zakres i potwierdź zgodność obrazu wysokości');
    const path=await pickImage();if(!path)return;
    const sha256=await call<string>('image_fingerprint',{path});
    const terrain:StoredTerrain={sha256,path,mapId:map.id,mapWidth:map.width,mapHeight:map.height,minHeight,maxHeight,unit:map.unit,source:terrainSource,confirmed:true,coordinateAlignmentConfirmed:true};
    await call('load_automation_assets',{maps:s.maps,terrain,terrains:s.terrains});update({terrain});notice('Mapa wysokości sprawdzona i wczytana lokalnie.');
  });};
  return <>
    <section className="panel"><div className="panel-heading"><div><h2>Automatyczne przygotowanie odczytu</h2><p>W analizie obrazu wybierz okno i uruchom odczyt. Dwa zgodne odczyty zatwierdzają wykryte pola.</p></div></div>
      <label className="check-field"><input type="checkbox" checked={s.autoHud} onChange={e=>update({autoHud:e.target.checked})}/>Wykrywaj pola HUD: czas, kompas oraz jawne etykiety pojazdu, amunicji i mapy</label>
      <label className="check-field"><input type="checkbox" checked={s.autoWeapon} onChange={e=>update({autoWeapon:e.target.checked})}/>Wybieraj konkretny profil z nazwy pojazdu i amunicji</label>
      <label className="check-field"><input type="checkbox" checked={s.autoMap} onChange={e=>update({autoMap:e.target.checked})}/>Szukaj widocznej mapy w lokalnej bibliotece i przeliczaj jej pozycje</label>
      <label className="field"><span>Preferowana mapa do dopasowania obrazu</span><select value={s.communityMapId??''} onChange={e=>update({communityMapId:e.target.value||null})}><option value="">Cała biblioteka (wolniej)</option>{s.maps.map(m=><option key={m.id} value={m.id}>{m.name}</option>)}</select></label><p className="hint">Wybór ogranicza wyszukiwanie; pozycje wymagają także zgodności obrazu z tą mapą.</p>
      <div className="automation-status" aria-live="polite"><p>Odczyt: <b>{running?'aktywny':'zatrzymany'}</b></p><p>HUD: <b>{report?.hudApplied?'pola ustabilizowane':report?.hud?.status??'oczekuje'}</b></p><p>Mapa: <b>{report?.activeMap?.name??report?.map?.status??'oczekuje'}</b></p><p>Amunicja: <b>{report?.weapon?.status==='matched'?'jednoznaczne dopasowanie':report?.weapon?.status??'oczekuje'}</b></p></div>
      {report?.hud && <details><summary>Dowody wykrywania HUD ({report.hud.regions.length})</summary>{report.hud.regions.map(r=><p key={r.region.id}>{r.region.kind}: {r.text} · {(r.confidence*100).toFixed(0)}% · {r.evidence.join('; ')}</p>)}</details>}
      {report?.weapon && report.weapon.status!=='matched' && <p className="hint">Widoczne warianty: {report.weapon.candidates.map(c=>`${c.vehicle}${c.projectile?' / '+c.projectile:''}`).join(', ')||'brak zgodnych nazw'}. Brak jednoznacznego pocisku wstrzymuje automatykę uzbrojenia.</p>}
      {report?.warnings.map((w,i)=><p className="hint" key={i}>{w}</p>)}
    </section>
    <section className="panel"><h2>Biblioteka rozpoznawanych map</h2><p className="hint">Zaimportuj pełną mapę w zakładce „Mapa i cel”, ustaw skalę i północ, następnie dodaj ją tutaj. Rozpoznawanie wymaga tekstury referencyjnej; obrazy społeczności możesz wczytać w panelu zasobów powyżej.</p>
      <form onSubmit={e=>{e.preventDefault();if(mapConfirmed)void attempt(()=>registerMap(mapSource));}}><label className="field"><span>Źródło obrazu i kalibracji</span><input value={mapSource} onChange={e=>setMapSource(e.target.value)} required maxLength={500}/></label><label className="check-field"><input type="checkbox" checked={mapConfirmed} onChange={e=>setMapConfirmed(e.target.checked)} required/>Potwierdzam pełny kadr mapy, jej orientację, skalę i prawo do używania pliku</label><button disabled={busy||!mapConfirmed||!s.mapPath||!s.scale||s.maps.length>=32}>Dodaj bieżącą mapę do biblioteki</button></form>
      {s.maps.map(m=><div className="library-row" key={m.id}><span><b>{m.name}</b><small>{m.width}×{m.height} · {m.calibration.unitsPerPixel.toFixed(4)} {m.unit}/px</small></span><button onClick={()=>update({maps:s.maps.filter(x=>x.id!==m.id),terrain:s.terrain?.mapId===m.id?null:s.terrain,terrains:s.terrains.filter(t=>t.mapId!==m.id)})}>Usuń</button></div>)}
      <p className="hint">Dopasowanie obsługuje skalę i skonfigurowane obroty co 90°. Brak mapy, zbyt mały fragment lub podobne wzorce wstrzymują pozycje automatyczne.</p>
    </section>
    <section className="panel"><h2>Wysokość terenu</h2><p className="hint">Liniowa mapa szarości: 0 = minimum, 255 = maksimum; przezroczystość oznacza brak danych. Obraz musi mieć taki sam zakres i kierunek jak pełna mapa. Wysokość pojazdu nad terenem wymaga osobnego pomiaru.</p><form onSubmit={importTerrain}><div className="form-grid"><label className="field"><span>Mapa</span><select value={terrainMap} onChange={e=>setTerrainMap(e.target.value)} required><option value="">Wybierz mapę</option>{s.maps.map(m=><option key={m.id} value={m.id}>{m.name}</option>)}</select></label><label className="field"><span>Minimum (jednostki wybranej mapy)</span><input type="number" step="any" value={minimum} onChange={e=>setMinimum(e.target.value)} required/></label><label className="field"><span>Maksimum</span><input type="number" step="any" value={maximum} onChange={e=>setMaximum(e.target.value)} required/></label><label className="field"><span>Źródło wysokości i kodowania</span><input value={terrainSource} onChange={e=>setTerrainSource(e.target.value)} required maxLength={500}/></label></div><label className="check-field"><input type="checkbox" checked={terrainConfirmed} onChange={e=>setTerrainConfirmed(e.target.checked)} required/>Potwierdzam jednostki, zakres i zgodność współrzędnych</label><button disabled={busy||!terrainConfirmed||!s.maps.length}>Importuj i sprawdź mapę wysokości</button>{s.terrain&&<p>Wczytana: {s.terrain.mapId} · {s.terrain.minHeight}–{s.terrain.maxHeight} <button type="button" onClick={()=>update({terrain:null})}>Usuń</button></p>}</form></section>
    <section className="panel"><h2>Model pocisku z obserwowanych strzałów</h2><p className="hint">Co najmniej trzy różne pomiary w grze dla wybranego pocisku. Jeden wiersz: dystans; elewacja w stopniach; czas lotu w sekundach; cel wyżej o. Liczby dziesiętne zapisuj kropką. Model stałej prędkości i grawitacji wymaga zgodności z pomiarami.</p><form onSubmit={fitSamples}><label className="field"><span>Pomiary ({s.mapUnit})</span><textarea value={samples} onChange={e=>setSamples(e.target.value)} rows={5} required placeholder={'dystans; elewacja; czas lotu; różnica wysokości'}/></label><div className="form-grid"><label className="field"><span>Źródło pomiarów</span><input value={measurementSource} onChange={e=>setMeasurementSource(e.target.value)} required maxLength={500}/></label><label className="field"><span>Wersja / data gry</span><input value={gameVersion} onChange={e=>setGameVersion(e.target.value)} maxLength={200}/></label></div><button disabled={busy||!profile||profile.category==='guided_missile'}>Dopasuj i sprawdź model</button></form>{fit&&<div className="fit-result"><p>Prędkość: {fit.fittedProfile.speed?.toFixed(3)} · grawitacja: {fit.fittedProfile.gravity?.toFixed(3)}</p><p>Błąd RMS: {fit.rmsResidualUnits.toFixed(4)} · maksimum: {fit.maxResidualUnits.toFixed(4)} · błąd względny: {(100*fit.maxRelativeResidual).toFixed(2)}%</p><p>Zakres pomiarów: {fit.observedRangeMin.toFixed(1)}–{fit.observedRangeMax.toFixed(1)} {s.unit} · uwarunkowanie: {fit.conditionNumber.toFixed(2)}</p><ul className="assumptions">{fit.assumptions.map(a=><li key={a}>{a}</li>)}</ul><button className="primary" onClick={()=>{applyFit(fit);setFit(null);}}>Zastosuj lokalną kalibrację beta</button></div>}</section>
  </>;
}
