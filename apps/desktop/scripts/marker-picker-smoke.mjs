import { chromium } from '@playwright/test';
import { spawn } from 'node:child_process';
import { readFile,writeFile,mkdir } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { resolve } from 'node:path';
import assert from 'node:assert/strict';
const root=resolve(import.meta.dirname,'../../..');
const argv=process.argv.slice(2);const arg=name=>argv.includes(name)?argv[argv.indexOf(name)+1]:null;
const exe=resolve(arg('--exe')??resolve(root,'release/MTC-Tactical-Assistant-0.5.1.exe'));
const output=resolve(root,'tests/output/v0.5.1');const port=9228;
const run=Date.now();const settings=resolve(output,`marker-settings-${run}`);
const pause=ms=>new Promise(r=>setTimeout(r,ms));
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const exeSha256=hash(await readFile(exe));await mkdir(output,{recursive:true});
const manifest=JSON.parse(await readFile(resolve(output,'fixtures/manifest.json'),'utf8'));
const calibration={unitsPerPixel:1,northRotationDeg:0,originPixel:{x:0,y:0},originWorld:{x:0,y:0},uncertaintyUnits:0};
const map={id:'marker-smoke',name:'SYNTHETIC marker map',path:manifest.reference.path,sha256:manifest.reference.sha256,width:350,height:200,unit:'game_meters',source:manifest.source,confirmed:true,calibration};
const checks=[],evidence={};let app,fixture,browser,page,invoke;let passed=false,error;
async function poll(fn,label,timeout=15000){const start=Date.now();let last;while(Date.now()-start<timeout){try{const value=await fn();if(value)return value;}catch(e){last=String(e);}await pause(80);}throw new Error(`Timed out ${label}: ${last??''}`);}
// The UI capture loop may hold the single native frame slot; that busy rejection is by design,
// so a direct analyse_batch waits for the slot instead of racing the loop. Other errors still fail.
async function batchWhenIdle(request){const start=Date.now();for(;;){try{return await invoke('analyse_batch',{request});}catch(e){if(!String(e).includes('Poprzednia klatka jest przetwarzana')||Date.now()-start>5000)throw e;await pause(40);}}}
async function reject(command,args,label){let message;try{await invoke(command,args);}catch(e){message=String(e);}assert(message,label);checks.push(label);return message;}
function observer(){
  window.__markerSmoke={calls:[],frames:[],setups:[],selections:[]};const original=window.fetch;
  window.fetch=function(input,init){const url=typeof input==='string'?input:input?.url;
    if(!['capture_marker_setup','calibrate_visible_marker','analyse_batch','load_automation_assets'].some(cmd=>url===`http://ipc.localhost/${cmd}`))return original.call(this,input,init);
    const cmd=url.split('/').at(-1);const record={cmd,args:JSON.parse(init?.body??'{}'),result:null,error:null,startedAt:Date.now(),completedAt:null};window.__markerSmoke.calls.push(record);
    return original.call(this,input,init).then(response=>{void response.clone().text().then(body=>{if(response.headers.get('Tauri-Response')==='ok'){record.result=JSON.parse(body);if(cmd==='analyse_batch')window.__markerSmoke.frames.push(record.result);if(cmd==='capture_marker_setup')window.__markerSmoke.setups.push(record.result);if(cmd==='calibrate_visible_marker')window.__markerSmoke.selections.push(record.result);}else record.error=body;record.done=true;record.completedAt=Date.now();}).catch(e=>{record.error=String(e);record.done=true;record.completedAt=Date.now();});return response;});
  };
}
try{
  try{await fetch(`http://127.0.0.1:${port}/json/version`);throw new Error('CDP port already occupied');}catch(e){if(String(e).includes('occupied'))throw e;}
  app=spawn(exe,['--test-data-dir',settings],{windowsHide:true,env:{...process.env,WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port}`,WEBVIEW2_USER_DATA_FOLDER:resolve(output,`marker-webview-${run}`)},stdio:'ignore'});
  await poll(async()=> (await fetch(`http://127.0.0.1:${port}/json/version`)).ok,'CDP');browser=await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
  page=await poll(async()=>{for(const p of browser.contexts()[0].pages()){if(p.url().startsWith('http://tauri.localhost/')&&!p.url().includes('overlay=1')&&await p.locator('h1').count())return p;}return false;},'main page');
  invoke=(command,args={})=>page.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
  const initial=await poll(async()=>{const s=await invoke('load_settings');return s?.schemaVersion===4?s:false;},'settings');
  const profiles=await invoke('profiles');const profile={...profiles.find(p=>p.id==='community-bm_21_grad-1'),sourceAcknowledged:true};
  await invoke('save_settings',{value:{...initial,selected:profile.id,mode:'assisted',scale:1,rotation:0,uncertainty:0,mapUnit:'game_meters',unit:'m gry',mapPath:map.path,mapName:map.name,mapWidth:350,mapHeight:200,own:null,target:null,fps:3,maps:[map],terrains:[],terrain:null,communityMapId:map.id,overrides:{[profile.id]:profile},autoHud:false,autoWeapon:false,autoMap:true,captureBackend:'wgc',captureReferenceWidth:0,captureReferenceHeight:0,hudRegions:initial.hudRegions.map(r=>({...r,enabled:false})),markerConfig:{...initial.markerConfig,enabled:false,mapWidth:350,mapHeight:200,coordinateCalibrationConfirmed:false,viewSignature:null,signatures:[]}}});
  await page.addInitScript(observer);await page.reload();await page.locator('h1').waitFor();
  await poll(async()=>{const calls=await page.evaluate(()=>window.__markerSmoke?.calls??[]);return calls.some(c=>c.cmd==='load_automation_assets'&&c.done&&c.result===1);},'asset startup');
  fixture=spawn(resolve(root,'target/debug/examples/hud_fixture.exe'),['--auto-hud','--same-color','--passive','--seconds','180'],{windowsHide:true,stdio:['ignore','pipe','ignore']});
  const handle=await new Promise((accept,reject)=>{let text='';const timer=setTimeout(()=>reject(new Error('No fixture HWND')),5000);fixture.once('error',reject);fixture.stdout.on('data',data=>{text+=data;if(text.includes('\n')){clearTimeout(timer);accept(Number(text.split(/\r?\n/)[0]));}});});evidence.handle=handle;
  await page.getByRole('button',{name:'Analiza obrazu',exact:true}).click();await page.getByRole('button',{name:'Odśwież okna',exact:true}).click();
  await poll(async()=>await page.locator('.capture-select select option').filter({hasText:'MTC HUD fixture'}).count(),'fixture selection');await page.locator('.capture-select select').selectOption(String(handle));await pause(500);
  const picker=page.locator('.marker-picker');
  async function selectVisible(x,y){
    await picker.getByRole('button',{name:'Odśwież podgląd markerów',exact:true}).click();
    const setup=await poll(async()=>{const s=await page.evaluate(()=>window.__markerSmoke.setups.at(-1));return s&&s.timestampMs<=Date.now()&&Date.now()-s.timestampMs<1500?s:false;},'fresh setup');
    await picker.locator('img').waitFor();await picker.locator('img').evaluate(async img=>{await img.decode();});
    assert.equal(await picker.locator('img').evaluate(img=>img.naturalWidth),setup.previewWidth);
    const rect=await picker.locator('img').boundingBox();assert(rect);
    await picker.locator('img').click({position:{x:x/setup.width*rect.width,y:y/setup.height*rect.height}});
    return poll(async()=>{const result=await page.evaluate(()=>window.__markerSmoke.selections.at(-1));return result?.timestampMs===setup.timestampMs?result:false;},'clicked native approval');
  }
  const own=await selectVisible(316,276);assert.equal(own.selection.hint.role,'own');assert.deepEqual(own.signature.rgb,[0,255,0]);
  await poll(async()=>{const f=await page.evaluate(()=>window.__markerSmoke.frames.at(-1));return f?.analysis.markers.some(m=>m.role==='own'&&m.confidence>=0.85)?f:false;},'own selected ready');
  checks.push('Actual MarkerPicker click calibrates green own icon and starts fresh native tracking');
  await pause(2300); // Keep live capture running while the first snapshot expires.
  const target=await selectVisible(506,376);assert.equal(target.selection.hint.role,'target');assert.deepEqual(target.signature.rgb,own.signature.rgb);
  const ready=await poll(async()=>{const f=await page.evaluate(()=>window.__markerSmoke.frames.at(-1));return f?.markerSelections?.length===2&&f.markerSelections.every(s=>s.status==='ready')?f:false;},'two same-color selections ready',25000);
  const selectedOwn=ready.analysis.markers.find(m=>m.role==='own');const selectedTarget=ready.analysis.markers.find(m=>m.role==='target');
  assert(Math.abs(selectedOwn.mapPosition.x-46)<3);assert(Math.abs(selectedTarget.mapPosition.x-236)<3);assert(Math.abs(selectedTarget.mapPosition.y-136)<3);
  assert.notEqual(selectedOwn.signatureId,selectedTarget.signatureId);assert.equal(ready.analysis.markers.filter(m=>m.role==='own'||m.role==='target').length,2);
  checks.push('Refreshing after two seconds preserves the first track; second same-RGB click selects target and excludes third green distractor');
  const request=await page.evaluate(()=>window.__markerSmoke.calls.filter(c=>c.cmd==='analyse_batch').at(-1).args.request);
  evidence.ready={timestampMs:ready.timestampMs,markers:ready.analysis.markers,markerSelections:ready.markerSelections,configuration:ready.configuration};
  evidence.nativeSelections={own:own.selection,target:target.selection};
  await poll(async()=>{const value=await invoke('latest');return value?.result?.bearing?.distance>200&&value.positionsTimestamp?value:false;},'Automatic overlay calculation');
  checks.push('Actual Automatic UI calculates distance from the two approved live positions and publishes overlay data');
  await picker.screenshot({path:resolve(output,'marker-picker.png')});
  await pause(2200);await reject('calibrate_visible_marker',{token:(await page.evaluate(()=>window.__markerSmoke.setups.at(-1))).token,click:{x:316/ready.width,y:276/ready.height},role:'own'},'Stale source snapshot cannot be relabeled with a fresh click timestamp');
  const altered=structuredClone(request);altered.selections[0].hint.confirmedPoint.x+=40;
  const rejected=await batchWhenIdle(altered);assert(rejected.markerSelections.some(s=>s.status==='blocked'));assert(!rejected.analysis.markers.some(m=>m.role==='own'&&m.confidence>=0.85));
  checks.push('Tampered selected-point metadata has no native approval and cannot produce an own position');
  await page.getByRole('button',{name:'Zatrzymaj',exact:true}).click();await pause(500);
  const stopped=await batchWhenIdle(request);assert(stopped.markerSelections.every(s=>s.status==='blocked'));assert(!stopped.analysis.markers.some(m=>m.role==='own'||m.role==='target'));
  checks.push('Stop clears native approvals and prevents old selectors from restoring owned roles');
  const saved=await invoke('load_settings');assert(!('markerSelections'in saved));assert(saved.markerConfig.signatures.every(s=>s.role==='unassigned'));
  checks.push('Only unassigned color signatures persist; clicked role approvals remain transient');passed=true;
}catch(e){error=String(e.stack??e);if(page&&!page.isClosed())try{evidence.observed=await page.evaluate(()=>({calls:window.__markerSmoke.calls.map(c=>({cmd:c.cmd,args:c.cmd==='analyse_batch'?{selections:(c.args.request?.selections??[]).map(x=>({role:x.hint.role,hintTs:x.hint.timestampMs,epoch:x.captureEpoch}))}:c.args,error:c.error,done:c.done,startedAt:c.startedAt,completedAt:c.completedAt,frameTs:c.result?.timestampMs,selections:c.result?.markerSelections?.map(x=>({status:x.status,warning:x.warning}))})),lastFrame:window.__markerSmoke.frames.at(-1),selectionRoles:window.__markerSmoke.selections.map(s=>s.selection.hint.role)}));}catch{};if(page&&!page.isClosed())try{await page.screenshot({path:resolve(output,'marker-picker-failure.png')});}catch{}}
finally{
  await browser?.close().catch(()=>{});
  for(const child of [fixture,app])if(child){child.kill();await Promise.race([new Promise(r=>child.exitCode!==null||child.signalCode!==null?r():child.once('exit',r)),pause(5000)]);}
  const cleanup={appClosed:!app||app.exitCode!==null||app.signalCode!==null,fixtureClosed:!fixture||fixture.exitCode!==null||fixture.signalCode!==null};if(!Object.values(cleanup).every(Boolean))passed=false;
  await writeFile(resolve(output,'marker-picker-smoke.json'),JSON.stringify({passed,checks,exeSha256,exe,error:error??null,evidence,cleanup,physicalHotkeysTested:0,date:new Date().toISOString()},null,2));
  console.log(JSON.stringify({passed,checks:checks.length,exeSha256,error:error??null,cleanup},null,2));
}
if(!passed)process.exitCode=1;
