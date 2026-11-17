import { chromium } from '@playwright/test';
import { spawn } from 'node:child_process';
import { mkdir, writeFile, readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import net from 'node:net';
import { resolve, resolve as resolvePath } from 'node:path';
import assert from 'node:assert/strict';
const root = resolve(import.meta.dirname, '../../..');
const exe = process.argv[2] || resolve(root, 'target/release/mtc-tactical-assistant.exe');
const output = resolve(root, 'tests/output/v0.3.0');
const runId=Date.now();
await mkdir(output, { recursive: true });
await new Promise((res,reject)=>{
  const probe=net.createServer();
  probe.once('error',()=>reject(new Error('Port 9223 is occupied; close the previous isolated test before launching')));
  probe.listen(9223,'127.0.0.1',()=>probe.close(res));
});
const exeSha256=createHash('sha256').update(await readFile(exe)).digest('hex');
const child = spawn(exe, ['--test-data-dir', resolve(output, `settings-${runId}`)], {
  windowsHide: true,
  env: { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: '--remote-debugging-port=9223', WEBVIEW2_USER_DATA_FOLDER: resolve(output, `webview-${runId}`) },
  stdio: ['ignore', 'pipe', 'pipe'],
});
let stderr = '';
child.stderr.on('data', data => stderr += data.toString());
const checks = [];
const webviewEvents=[];
let browser;
let fixture;
const pause=milliseconds=>new Promise(resolve=>setTimeout(resolve,milliseconds));
// Await native IPC before checking truthiness. An async waitForFunction
// predicate is a truthy Promise even when its eventual result is false.
async function waitForNative(check,label,timeout=15000) {
  const started=Date.now();let lastError;
  while(Date.now()-started<timeout) {
    try {const value=await check();if(value)return value;}catch(error){lastError=error;}
    await pause(100);
  }
  throw new Error(`Timed out waiting for ${label}${lastError?`: ${lastError}`:''}`);
}
try {
  let connected = false;
  for (let i = 0; i < 100; i++) {
    try { const r = await fetch('http://127.0.0.1:9223/json/version'); if (r.ok) { connected = true; break; } } catch {}
    await new Promise(r => setTimeout(r, 300));
  }
  assert(connected, 'WebView2 should start with a diagnostic CDP endpoint');
  browser = await chromium.connectOverCDP('http://127.0.0.1:9223');
  const context = browser.contexts()[0];
  const observePage=p=>{const url=p.url();p.on('close',()=>webviewEvents.push({event:'close',url,at:Date.now()}));p.on('crash',()=>webviewEvents.push({event:'crash',url,at:Date.now()}));};
  context.pages().forEach(observePage);context.on('page',observePage);
  let page;
  for(let attempt=0;attempt<100&&!page;attempt++) {
    for(const candidate of context.pages()) {
      if(candidate.url().startsWith('http://tauri.localhost/')&&!candidate.url().includes('overlay=1')) {
        try { if(await candidate.locator('h1').count()) {page=candidate;break;} } catch {}
      }
    }
    if(!page)await new Promise(r=>setTimeout(r,100));
  }
  assert(page, 'Main WebView should exist');
  await page.waitForSelector('h1');
  const invoke = (command, args = {}) => page.evaluate(({ command, args }) => window.__TAURI_INTERNALS__.invoke(command, args), { command, args });
  const startupDiagnostics=await waitForNative(()=>invoke('diagnostics'),'native startup diagnostics');
  assert.equal(typeof startupDiagnostics.overlayVisible,'boolean');
  assert(startupDiagnostics.dpiScale>0,'Startup diagnostics must contain the actual window scale');
  console.log('Main PID',child.pid,'pages',context.pages().map(p=>p.url()));
  const profiles = await invoke('profiles');
  assert.equal(profiles.length, 160);
  assert.equal(profiles.filter(p=>p.sourceMetadata).length,153);
  assert.equal((await invoke('community_maps')).length,25);
  checks.push('Native IPC loads 153 community projectiles + 7 templates and 25 map dimensions');
  await page.getByRole('button', { name: 'Symulacja', exact: true }).click();
  await page.waitForFunction(() => document.querySelector('.metric strong')?.textContent.includes('502.5'));
  const metrics = await page.locator('.metric strong').allTextContents();
  assert(metrics[0].includes('502.5')); assert(metrics[1].includes('58.8')); assert(!metrics[2].startsWith('—'));
  checks.push('Map distance 502.5, azimuth 58.8 and synthetic trajectory rendered from Rust');
  console.log('Before screenshot',await invoke('diagnostics'));
  await page.screenshot({ path: resolve(output, 'desktop-map.png'), fullPage: true });
  console.log('After screenshot',await invoke('diagnostics'),'pages',context.pages().map(p=>p.url()));
  await page.getByRole('button', { name: 'Pokaż nakładkę', exact: true }).click();
  let diagnostics = await invoke('diagnostics');
  assert.equal(diagnostics.overlayVisible, true); assert.equal(diagnostics.alwaysOnTop, true); assert.equal(diagnostics.decorated, false);
  assert.equal(diagnostics.shortcutsRegistered.length,3);
  assert(diagnostics.shortcutsRegistered.every(Boolean)); assert(diagnostics.dpiScale > 0);
  checks.push('Native overlay visible, always on top, frameless; all three global hotkeys registered');
  await invoke('overlay_control', { visible: null, clickThrough: true });
  assert.equal((await invoke('diagnostics')).clickThrough, true);
  assert.equal((await invoke('diagnostics')).overlayVisible,true,'Click-through must preserve visibility');
  const inspectOverlay = () => new Promise((resolve, reject) => {
    const inspect = spawn(resolvePath(root, 'target/debug/examples/hud_fixture.exe'), ['--inspect-overlay'], { windowsHide: true });
    let text = '';
    inspect.stdout.on('data', data => text += data.toString());
    inspect.once('error', reject);
    inspect.once('exit', code => { if (code === 0) { try { resolve(JSON.parse(text)); } catch (e) { reject(e); } } else reject(new Error(`Inspector exited ${code}`)); });
  });
  assert.equal((await inspectOverlay()).clickThrough, true);
  await invoke('overlay_control', { visible: null, clickThrough: false });
  assert.equal((await invoke('diagnostics')).clickThrough, false);
  assert.equal((await invoke('diagnostics')).overlayVisible,true,'Editing must preserve visibility');
  assert.equal((await inspectOverlay()).clickThrough, false);
  checks.push('Native click-through toggles actual Win32 WS_EX_TRANSPARENT on and off');
  const overlay = context.pages().find(p => p.url().includes('overlay=1'));
  assert(overlay, 'Overlay WebView should exist');
  await overlay.waitForSelector('.overlay-card');
  await overlay.waitForFunction(() => document.querySelector('.overlay-metrics')?.textContent.includes('502.5'));
  assert.equal(await overlay.evaluate(() => getComputedStyle(document.documentElement).backgroundColor), 'rgba(0, 0, 0, 0)');
  await overlay.screenshot({ path: resolve(output, 'overlay.png'), omitBackground: true });
  checks.push('Overlay receives native calculation updates and has transparent HTML background');
  await invoke('overlay_control', { visible: false, clickThrough: false });
  assert.equal((await invoke('diagnostics')).overlayVisible, false);
  await page.getByRole('button', { name: 'Analiza obrazu', exact: true }).click();
  for (const [name, expected] of [['bm21', 1662], ['helicopter', 1773]]) {
    const data = await invoke('analyse_batch_fixture', { name });
    assert.equal(data.analysis.numericObservations[0]?.value, expected);
    assert.equal(data.targetMotion,null);
    checks.push(`Actual Windows batch OCR fixture ${name}: ${data.analysis.numericObservations[0].text} (${data.elapsedMs}ms)`);
  }
  await page.getByRole('button', { name: 'Zdjęcie BM-21', exact: true }).click();
  await page.waitForFunction(() => document.querySelector('.hud-result-grid')?.textContent.includes('27:42'));
  await page.screenshot({ path: resolve(output, 'desktop-ocr.png'), fullPage: true });
  checks.push('OCR fixture flow displays 27:42 through the application UI');
  await page.waitForFunction(() => document.querySelector('.hud-result-grid')?.textContent.includes('Brak świeżego'), undefined, { timeout: 5000 });
  assert((await page.locator('.hud-result-grid').textContent()).includes('Brak świeżego'));
  checks.push('Application expires stale OCR after two seconds');
  fixture = spawn(resolve(root, 'target/debug/examples/hud_fixture.exe'), ['--multi-hud','--passive','--seconds','180'], { windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'] });
  const handle = await new Promise((resolve, reject) => {
    fixture.once('error', reject);
    fixture.stdout.once('data', data => resolve(Number(data.toString().trim())));
  });
  assert(Number.isSafeInteger(handle));
  const preview = await invoke('capture_preview',{handle,backend:'wgc'});
  assert(preview.width>=620 && preview.height>=440);
  const roi = (x,y,width,height)=>({x:x/preview.width,y:y/preview.height,width:width/preview.width,height:height/preview.height});
  const captured = await invoke('analyse_capture', { handle, roi:roi(20,40,220,84), kind: 'timer' });
  assert.equal(captured.observation?.value, 1662);
  checks.push(`Live visible-window capture -> Windows OCR -> native IPC: ${captured.text} (${captured.elapsedMs}ms)`);
  const mapRoi=roi(270,240,350,200);
  const view=await invoke('calibrate_map_view',{handle,backend:'wgc',roi:mapRoi});
  assert.equal(view.signature.graySamples.length,256);
  assert(Math.abs(view.width-350)<=1&&Math.abs(view.height-200)<=1);
  checks.push('WGC snapshot and map-reference calibration return client geometry and 256-value descriptor');
  const signature=(id,role,rgb)=>({id,role,rgb,tolerance:10,minComponentPixels:4,maxComponentPixels:300,minAspectRatio:0.25,maxAspectRatio:4,minFillRatio:0.2});
  const config={schemaVersion:1,referenceWidth:preview.width,referenceHeight:preview.height,aspectTolerance:0.03,maxAgeMs:2000,
    ocrRegions:[{id:'timer',kind:'timer',roi:roi(20,40,220,84),upscale:2},{id:'heading',kind:'heading',roi:roi(30,124,140,60),upscale:2},{id:'vehicle',kind:'vehicleName',roi:roi(30,184,200,46),upscale:2}],
    markerConfig:{enabled:true,mapRoi,mapWidth:view.width,mapHeight:view.height,coordinateCalibrationConfirmed:true,aspectTolerance:0.03,viewSignature:view.signature,signatures:[signature('own','own',[0,255,0]),signature('target','target',[255,0,0])]}};
  const calibration={unitsPerPixel:1,northRotationDeg:0,originPixel:{x:0,y:0},originWorld:{x:0,y:0},uncertaintyUnits:1};
  const batch=conf=>invoke('analyse_batch',{request:{handle,backend:'wgc',config:conf,calibration,scope:'native-smoke',maxSpeed:500,includePreview:false,fps:3}});
  await batch(config);
  await new Promise(r=>setTimeout(r,400));
  let stable=await batch(config);
  for(let retry=0;retry<4 && stable.targetMotion?.status!=='ready';retry++) {
    await new Promise(r=>setTimeout(r,400));stable=await batch(config);
  }
  await writeFile(resolve(output,'batch-analysis.json'),JSON.stringify(stable,null,2));
  assert.equal(stable.analysis.numericObservations.find(o=>o.kind==='timer')?.value,1662);
  assert.equal(stable.analysis.numericObservations.find(o=>o.kind==='heading')?.value,138);
  assert.equal(stable.analysis.textObservations[0]?.text,'BM-21 Grad');
  assert.equal(stable.analysis.markers.length,2);
  assert(stable.analysis.markers.every(m=>m.confidence>=0.85));
  assert.equal(stable.targetMotion.status,'ready');
  assert.deepEqual(stable.targetMotion.velocity,[0,0]);
  assert.deepEqual(stable.ownMotion.velocity,[0,0]);
  await writeFile(resolve(output,'batch-analysis.json'),JSON.stringify(stable,null,2));
  checks.push('Live WGC batch reads timer1662, heading138, explicit BM-21 text, two stable markers and stationary motion');
  const own=stable.analysis.markers.find(m=>m.role==='own').mapPosition;
  const target=stable.analysis.markers.find(m=>m.role==='target').mapPosition;
  const community=profiles.find(p=>p.id==='community-bm_21_grad-1');
  assert(community);
  const input={calibration,own,target,profile:community,heightDelta:0,targetVelocity:null,mapUnit:'game_meters'};
  assert.equal((await invoke('calculate',{input})).solution,null);
  const enabled={...community,sourceAcknowledged:true};
  const solved=await invoke('calculate',{input:{...input,profile:enabled}});
  assert(solved.solution?.arcs.length>0);
  assert(Math.abs(solved.bearing.distance-Math.hypot(190,100))<2);
  assert.equal((await invoke('calculate',{input:{...input,profile:enabled,mapUnit:'game_units'}})).solution,null);
  const guided=profiles.find(p=>p.category==='guided_missile'&&p.sourceMetadata);
  assert.equal((await invoke('calculate',{input:{...input,profile:{...guided,sourceAcknowledged:true}}})).solution,null);
  assert.equal((await invoke('calculate',{input:{...input,profile:enabled,targetVelocity:[1,0],maxLeadTime:0.1}})).lead,null);
  checks.push('Rust calculates trajectory from markers and enforces community acknowledgement, unit, guided-mechanics and motion-horizon gates');
  assert.equal((await batch({...config,markerConfig:{...config.markerConfig,viewSignature:null}})).analysis.markers.length,0);
  const wrongAspect=await batch({...config,referenceWidth:preview.width*2});
  assert.equal(wrongAspect.analysis.numericObservations.length,0);
  assert.equal(wrongAspect.analysis.markers.length,0);
  checks.push('Missing map reference and changed capture aspect block image-derived positions/readouts');
  assert((await batch(config)).analysis.markers.every(m=>m.confidence<0.85));
  checks.push('Changed configuration invalidates previous stabilization history');
  await invoke('stop_capture');

  // Exercise the actual Automatic UI using our own visible fixture window.
  await new Promise(r=>setTimeout(r,600));
  const previousSettings=await invoke('load_settings');
  await invoke('save_settings',{value:{...previousSettings,mode:'assisted',selected:community.id,mapUnit:'game_meters',unit:'m gry',scale:null,own:null,target:null,mapPath:null,mapWidth:view.width,mapHeight:view.height,captureBackend:'wgc',captureReferenceWidth:preview.width,captureReferenceHeight:preview.height,hudRegions:config.ocrRegions.map(r=>({...r,enabled:true})),markerConfig:config.markerConfig,overrides:{...previousSettings.overrides,[community.id]:enabled}}});
  await page.reload();
  await page.waitForSelector('h1');
  const reloadedDiagnostics=await waitForNative(()=>invoke('diagnostics'),'reloaded native diagnostics');
  assert.equal(typeof reloadedDiagnostics.overlayVisible,'boolean');
  assert(reloadedDiagnostics.dpiScale>0,'Reload must retain native diagnostics');
  await page.getByRole('button',{name:'Analiza obrazu',exact:true}).click();
  await page.getByRole('button',{name:'Odśwież okna',exact:true}).click();
  await page.locator('.capture-select select').selectOption(String(handle));
  await page.locator('.marker-settings summary').click();
  await page.getByRole('button',{name:'Zapamiętaj widok mapy i użyj go jako referencji',exact:true}).click();
  await page.waitForFunction(()=>document.querySelector('.marker-settings')?.textContent.includes('Deskryptor mapy jest zapisany'));
  // Map calibration invalidates the previous height observation. Explicitly
  // enter zero for this synthetic stationary fixture instead of relying on
  // an implicit default measurement.
  await page.getByRole('button',{name:'Profile i balistyka',exact:true}).click();
  await page.getByLabel('Cel wyżej niż stanowisko (jedn.)',{exact:true}).fill('0');
  assert.equal(await page.getByLabel('Cel wyżej niż stanowisko (jedn.)',{exact:true}).inputValue(),'0');
  await page.getByRole('button',{name:'Mapa i cel',exact:true}).click();
  await page.getByLabel('Skala (jedn./px)',{exact:true}).fill('1');
  await page.getByRole('button',{name:'Automatic',exact:false}).click();
  await page.getByRole('button',{name:'Analiza obrazu',exact:true}).click();
  await page.getByRole('button',{name:'Rozpocznij OCR',exact:true}).click();
  await page.waitForFunction(()=>document.querySelector('.hud-result-grid')?.textContent.includes('138'),undefined,{timeout:15000});
  const automaticResult=await waitForNative(async()=>{
    const value=await invoke('latest');
    return value.mode==='automatic'&&value.result?.solution?.arcs?.length>0?value:false;
  },'Automatic native trajectory',15000);
  assert.equal(automaticResult.mode,'automatic');
  assert(automaticResult.result?.solution?.arcs?.length>0,'Automatic must publish a complete native trajectory');
  assert(Math.abs(automaticResult.result.bearing.distance-Math.hypot(190,100))<2);
  await page.screenshot({path:resolve(output,'desktop-automatic.png'),fullPage:true});
  checks.push('Actual UI Automatic mode produces trajectory from live stabilized markers and publishes overlay data');
  fixture.kill();fixture=null;
  const invalidatedResult=await waitForNative(async()=>{
    const value=await invoke('latest');
    return value.result===null?value:false;
  },'closed-window Automatic invalidation',8000);
  assert.equal(invalidatedResult.result,null,'Closing the captured fixture must invalidate its result');
  assert.equal(invalidatedResult.mode,'automatic');
  checks.push('Closing the captured window invalidates the Automatic calculation');
  await page.getByRole('button',{name:'Zatrzymaj',exact:true}).click();
  await page.getByRole('button', { name: 'Mapa i cel', exact: true }).click();
  await page.getByRole('button',{name:'Symulacja',exact:true}).click();
  const select = page.locator('.sidebar select');
  await select.selectOption('bm21');
  await page.waitForFunction(() => document.querySelector('.warning-strip')?.textContent.includes('kalibracja profilu'));
  assert((await page.locator('.metric strong').nth(2).textContent()).startsWith('—'));
  checks.push('BM-21 without calibrated constants blocks elevation while keeping map distance and bearing');
  const settings = await invoke('load_settings');
  assert.equal(settings.schemaVersion, 3);
  checks.push('Versioned settings persist in the isolated test directory');
  await writeFile(resolve(output, 'smoke-test.json'), JSON.stringify({ passed: true, checks, diagnostics, stderr, exe,exeSha256,webviewEvents,pid:child.pid,physicalHotkeysTested:0,date: new Date().toISOString() }, null, 2));
  console.log(JSON.stringify({ passed: true, checks, diagnostics }, null, 2));
} catch (error) {
  await writeFile(resolve(output, 'smoke-test.json'), JSON.stringify({ passed: false, checks, error: String(error), stderr,exe,exeSha256,webviewEvents,pid:child.pid,physicalHotkeysTested:0,date:new Date().toISOString() }, null, 2));
  throw error;
} finally {
  await browser?.close();
  fixture?.kill();
  child.kill();
  await Promise.race([new Promise(r=>child.exitCode!==null?r():child.once('exit',r)),new Promise(r=>setTimeout(r,5000))]);
}
