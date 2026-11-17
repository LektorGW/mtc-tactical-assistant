import { chromium } from '@playwright/test';
import { spawn } from 'node:child_process';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import net from 'node:net';
import { resolve } from 'node:path';
import assert from 'node:assert/strict';

// Only our launched application and passive synthetic fixture are controlled.
// No desktop input, game frame recording or global key events are used.
const root = resolve(import.meta.dirname, '../../..');
const options = {};
for (let index = 2; index < process.argv.length; index++) {
  const arg = process.argv[index];
  assert(arg.startsWith('--'), `Unexpected argument ${arg}`);
  options[arg.slice(2)] = arg === '--preliminary' ? true : process.argv[++index];
}
const exe = resolve(options.exe ?? resolve(root, 'release/MTC-Tactical-Assistant-0.3.0.exe'));
const fixtureExe = resolve(root, 'target/debug/examples/hud_fixture.exe');
const output = resolve(root, options.output ?? 'tests/output/v0.3.0');
const port = Number(options.port ?? 9226);
assert(Number.isInteger(port) && port >= 1024 && port <= 65535, 'Invalid CDP port');
const endpoint = `http://127.0.0.1:${port}`;
const runId = `${Date.now()}-${process.pid}`;
const settingsDirectory = resolve(output, `map-hud-settings-${runId}`);
const webviewDirectory = resolve(output, `map-hud-webview-${runId}`);
const pause = milliseconds => new Promise(accept => setTimeout(accept, milliseconds));
const hashFile = async path => createHash('sha256').update(await readFile(path)).digest('hex');
await mkdir(output, { recursive: true });
await new Promise((accept, reject) => {
  const probe = net.createServer();
  probe.once('error', () => reject(new Error(`Port ${port} is occupied by another test`)));
  probe.listen(port, '127.0.0.1', () => probe.close(accept));
});
const manifest = JSON.parse(await readFile(resolve(output, 'fixtures/manifest.json'), 'utf8'));
assert.equal(await hashFile(manifest.reference.path), manifest.reference.sha256);
assert.equal(await hashFile(manifest.terrain.path), manifest.terrain.sha256);
const exeSha256 = await hashFile(exe);
const fixtureSha256 = await hashFile(fixtureExe);
const checks = [];
const evidence = { manifest, rejections: [], frames: [] };
const webviewEvents = [];
const diagnostics = {};
let child;
let fixture;
let browser;
let page;
let invoke;
let passed = false;
let failure;
let stderr = '';
let fixtureStderr = '';
let processError;

// Native promises are awaited in Node; never use async waitForFunction predicates.
async function poll(check, label, timeout = 15000, interval = 100) {
  const started = Date.now();
  let lastError;
  while (Date.now() - started < timeout) {
    try {
      const value = await check();
      if (value) return value;
    } catch (error) { lastError = String(error); }
    await pause(interval);
  }
  throw new Error(`Timed out: ${label}${lastError ? `; ${lastError}` : ''}`);
}
async function check(label, task) {
  try { await task(); checks.push({ label, status: 'passed' }); console.log(`PASS ${label}`); }
  catch (error) { checks.push({ label, status: 'failed', error: String(error) }); throw error; }
}
async function finalGate(label, task) {
  try { await task(); checks.push({ label, status: 'passed' }); console.log(`PASS ${label}`); }
  catch (error) {
    if (!options.preliminary) { checks.push({ label, status: 'failed', error: String(error) }); throw error; }
    checks.push({ label, status: 'pending-final-build', error: String(error) });
    console.log(`PENDING ${label}: ${error}`);
  }
}
async function rejection(command, args, label) {
  let error;
  try { await invoke(command, args); } catch (caught) { error = String(caught); }
  assert(error, `Expected native rejection: ${label}`);
  evidence.rejections.push({ label, error });
  return error;
}
const closeTo = (actual, expected, tolerance = 2) => assert(Math.abs(actual - expected) <= tolerance,
  `${actual} should be within ${tolerance} of ${expected}`);
const calibration = { unitsPerPixel: 1, northRotationDeg: 0, originPixel: { x: 0, y: 0 }, originWorld: { x: 0, y: 0 }, uncertaintyUnits: 0 };
const map = { sha256: manifest.reference.sha256, id: 'arctic_airbase', name: 'Arctic Airbase', path: manifest.reference.path,
  width: 350, height: 200, unit: 'game_meters', source: manifest.source, confirmed: true, calibration };
const terrain = { sha256: manifest.terrain.sha256, path: manifest.terrain.path, mapId: map.id, mapWidth: 350, mapHeight: 200,
  minHeight: 0, maxHeight: 100, unit: map.unit, source: manifest.source, confirmed: true, coordinateAlignmentConfirmed: true };
const signature = (id, role, rgb) => ({ id, role, rgb, tolerance: 5, minComponentPixels: 16, maxComponentPixels: 500,
  minAspectRatio: 0.5, maxAspectRatio: 2, minFillRatio: 0.7 });
const markerConfig = { enabled: true, mapRoi: { x: 0, y: 0, width: 1, height: 1 }, mapWidth: 350, mapHeight: 200,
  coordinateCalibrationConfirmed: false, aspectTolerance: 0.03, viewSignature: null,
  signatures: [signature('own', 'own', [0, 255, 0]), signature('target', 'target', [255, 0, 0])] };
const config = { schemaVersion: 1, referenceWidth: 0, referenceHeight: 0, aspectTolerance: 0.03, maxAgeMs: 2000,
  ocrRegions: [], markerConfig };
let handle;
let stable;

try {
  child = spawn(exe, ['--test-data-dir', settingsDirectory], { windowsHide: true,
    env: { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`, WEBVIEW2_USER_DATA_FOLDER: webviewDirectory },
    stdio: ['ignore', 'pipe', 'pipe'] });
  child.once('error', error => { processError = error; });
  child.stderr.on('data', data => { stderr = (stderr + data.toString()).slice(-64000); });
  await poll(async () => {
    if (processError) throw processError;
    assert(child.exitCode === null, `Application exited before CDP: ${child.exitCode}`);
    return (await fetch(`${endpoint}/json/version`)).ok;
  }, 'isolated application CDP', 25000, 200);
  browser = await chromium.connectOverCDP(endpoint);
  const context = browser.contexts()[0];
  const observe = target => {
    target.on('close', () => webviewEvents.push({ event: 'close', url: target.url(), at: Date.now() }));
    target.on('crash', () => webviewEvents.push({ event: 'crash', url: target.url(), at: Date.now() }));
    target.on('pageerror', error => webviewEvents.push({ event: 'pageerror', error: String(error), at: Date.now() }));
  };
  context.pages().forEach(observe);
  context.on('page', observe);
  page = await poll(async () => {
    for (const candidate of context.pages()) {
      if (candidate.url().startsWith('http://tauri.localhost/') && !candidate.url().includes('overlay=1') && await candidate.locator('h1').count()) return candidate;
    }
    return false;
  }, 'main application WebView');
  invoke = (command, args = {}) => page.evaluate(({ command, args }) => window.__TAURI_INTERNALS__.invoke(command, args), { command, args });
  diagnostics.initial = await poll(() => invoke('diagnostics'), 'native diagnostics');
  await poll(async () => (await invoke('load_settings'))?.schemaVersion === 3, 'initial schema 3 settings');
  const profiles = await invoke('profiles');
  const community = profiles.find(profile => profile.id === 'community-bm_21_grad-1');
  assert(community, 'Expected explicit BM-21 projectile profile');
  const enabledProfile = { ...community, sourceAcknowledged: true };
  await check('Loads an explicitly confirmed synthetic reference with unchanged SHA-256', async () => {
    assert.equal(await invoke('load_automation_assets', { maps: [map], terrain: null }), 1);
    assert.equal(await invoke('image_fingerprint', { path: map.path }), map.sha256);
  });
  fixture = spawn(fixtureExe, ['--auto-hud', '--passive', '--seconds', '120'], { windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'] });
  fixture.stderr.on('data', data => { fixtureStderr = (fixtureStderr + data.toString()).slice(-64000); });
  handle = await new Promise((accept, reject) => {
    let text = '';
    const timer = setTimeout(() => reject(new Error('Fixture did not publish its own HWND')), 5000);
    fixture.once('error', error => { clearTimeout(timer); reject(error); });
    fixture.stdout.on('data', data => {
      text += data.toString();
      if (text.includes('\n')) { clearTimeout(timer); accept(Number(text.split(/\r?\n/)[0])); }
    });
  });
  assert(Number.isSafeInteger(handle) && handle !== 0);
  evidence.fixture = { pid: fixture.pid, handle, arguments: ['--auto-hud', '--passive', '--seconds', '120'] };
  assert((await invoke('capture_windows')).some(window => window.handle === handle && window.title === 'MTC HUD fixture'));
  const batch = (scope = 'map-hud-smoke') => invoke('analyse_batch', { request: { handle, backend: 'wgc', config: structuredClone(config),
    calibration, scope, maxSpeed: 500, includePreview: false, fps: 3, automation: { autoHud: true, autoMap: true } } });
  await invoke('stop_capture');
  const first = await batch();
  evidence.frames.push(first);
  await check('First fresh frame proposes HUD regions without applying them', async () => {
    assert.equal(first.backend, 'wgc');
    assert.equal(first.automation.hudApplied, false);
    assert(first.analysis.markers.every(marker => marker.confidence < 0.85));
    assert(first.timestampMs <= Date.now() && Date.now() - first.timestampMs < 2000);
  });
  stable = await poll(async () => {
    const frame = await batch();
    evidence.frames.push(frame);
    return frame.automation?.hudApplied && frame.automation.weapon?.status === 'matched' && frame.targetMotion?.status === 'ready' ? frame : false;
  }, 'fresh stable HUD, map, exact ammunition and marker motion', 30000, 400);
  await check('Two-frame HUD stability reads exact vehicle/ammunition and numeric fields', async () => {
    for (const [kind, value] of [['timer', 1662], ['heading', 138], ['speed', 12], ['altitude', 42]]) {
      const observation = stable.analysis.numericObservations.find(item => item.kind === kind);
      assert.equal(observation?.value, value, kind);
      assert(observation.confidence >= 0.9, `${kind} should be stable`);
    }
    assert.equal(stable.automation.weapon.profileId, community.id);
    assert(stable.automation.weapon.confidence >= 0.85);
    assert(stable.analysis.textObservations.every(item => item.confidence >= 0.85));
    assert(stable.analysis.textObservations.some(item => item.kind === 'vehicleName' && item.text.includes('BM-21')));
    assert(stable.analysis.textObservations.some(item => item.kind === 'weaponName' && item.text.includes('9M22')));
    assert(stable.analysis.textObservations.some(item => item.kind === 'mapName' && item.text === 'Arctic Airbase'));
  });
  await check('Full client frame locates the known map and both markers in reference coordinates', async () => {
    assert.equal(stable.automation.map.status, 'matched');
    assert.equal(stable.automation.activeMap.id, map.id);
    const best = stable.automation.map.best;
    closeTo(best.transform.scale, 1, 0.03);
    closeTo(best.projectedMapBounds.x, 270, 3);
    closeTo(best.projectedMapBounds.y, 240, 3);
    assert.equal(stable.analysis.markers.length, 2);
    for (const [role, expected] of [['own', manifest.fixture.ownReferenceCenter], ['target', manifest.fixture.targetReferenceCenter]]) {
      const marker = stable.analysis.markers.find(item => item.role === role);
      assert(marker?.confidence >= 0.9);
      closeTo(marker.mapPosition.x, expected.x);
      closeTo(marker.mapPosition.y, expected.y);
    }
    assert.equal(stable.targetMotion.status, 'ready');
    assert.equal(stable.ownMotion.status, 'ready');
    for (const velocity of [...stable.targetMotion.velocity, ...stable.ownMotion.velocity]) closeTo(velocity, 0, 0.001);
  });
  await finalGate('stop_capture clears HUD application, marker confidence and tracker history', async () => {
    await invoke('stop_capture');
    const reset = await batch();
    evidence.afterStop = reset;
    assert.equal(reset.automation.hudApplied, false);
    assert(reset.analysis.markers.every(marker => marker.confidence < 0.85));
    assert.notEqual(reset.targetMotion?.status, 'ready');
  });
  await check('A changed source scope does not restore another source history', async () => {
    const changed = await batch('map-hud-other-source');
    evidence.changedScope = changed;
    assert.equal(changed.automation.hudApplied, false);
    assert(changed.analysis.markers.every(marker => marker.confidence < 0.85));
  });
  await finalGate('Wrong file SHA rejects loading and invalidates the previous runtime map', async () => {
    await rejection('load_automation_assets', { maps: [{ ...map, sha256: '0'.repeat(64) }], terrain: null }, 'wrong map fingerprint');
    const invalidated = await batch('map-hud-invalidated');
    evidence.wrongShaResult = invalidated;
    assert.equal(invalidated.automation.activeMap, null);
    assert.equal(invalidated.automation.map.status, 'missingReferences');
    assert.equal(invalidated.analysis.markers.length, 0);
  });
  await invoke('load_automation_assets', { maps: [map], terrain });
  await check('2×2 declared grayscale terrain gives known bilinear height differences', async () => {
    const full = await invoke('terrain_height', { mapId: map.id, unit: map.unit, own: { x: 0, y: 0 }, target: { x: 174.5, y: 99.5 } });
    closeTo(full, 50, 1e-9);
    const own = manifest.fixture.ownReferenceCenter;
    const target = manifest.fixture.targetReferenceCenter;
    const delta = await invoke('terrain_height', { mapId: map.id, unit: map.unit, own, target });
    closeTo(delta, manifest.terrainEncoding.expectedMarkerHeightDelta, 1e-9);
    evidence.terrain = { full, own, target, delta };
    await rejection('terrain_height', { mapId: 'different-map', unit: map.unit, own, target }, 'terrain wrong map');
    await rejection('terrain_height', { mapId: map.id, unit: 'game_units', own, target }, 'terrain wrong unit');
  });
  const input = { calibration, own: manifest.fixture.ownReferenceCenter, target: manifest.fixture.targetReferenceCenter,
    profile: enabledProfile, heightDelta: null, targetVelocity: null, ownVelocity: null, mapUnit: map.unit,
    terrainMapId: map.id, currentMapId: map.id, motion3d: null };
  await check('Native calculate uses imported terrain instead of an invented zero height', async () => {
    const result = await invoke('calculate', { input });
    assert(result.solution?.arcs.length > 0);
    const manual = await invoke('calculate', { input: { ...input, terrainMapId: null, heightDelta: evidence.terrain.delta } });
    assert.deepEqual(result.solution, manual.solution);
    evidence.terrainCalculation = result;
  });
  await finalGate('Terrain calculation requires matching current map and exact library calibration', async () => {
    await rejection('calculate', { input: { ...input, currentMapId: 'different-map' } }, 'terrain current map mismatch');
    await rejection('calculate', { input: { ...input, calibration: { ...calibration, northRotationDeg: 90 } } }, 'terrain library calibration mismatch');
    const missing = await invoke('calculate', { input: { ...input, terrainMapId: null, currentMapId: null, heightDelta: null } });
    assert.equal(missing.solution, null);
    assert(missing.bearing.distance > 0);
  });
  await invoke('stop_capture');
  await invoke('load_automation_assets', { maps: [map], terrain: null });
  await pause(650);
  const priorSettings = await invoke('load_settings');
  const uiSettings = { ...priorSettings, schemaVersion: 3, selected: community.id, mode: 'assisted', scale: 1, rotation: 0,
    uncertainty: 0, unit: 'm gry', mapUnit: map.unit, mapPath: map.path, mapName: map.name, mapWidth: 350, mapHeight: 200,
    own: null, target: null, fps: 3, captureBackend: 'wgc', captureReferenceWidth: 0, captureReferenceHeight: 0,
    overrides: { ...priorSettings.overrides, [community.id]: enabledProfile }, maps: [map], terrain: null,
    autoHud: true, autoMap: true, autoWeapon: true, markerConfig, hudRegions: priorSettings.hudRegions.map(region => ({ ...region, enabled: false })) };
  await check('Schema 3 automation flags and synthetic reference survive save/reload', async () => {
    await invoke('save_settings', { value: uiSettings });
    await page.reload();
    await page.waitForSelector('h1');
    const saved = await poll(async () => {
      const value = await invoke('load_settings');
      return value?.autoHud && value.autoMap && value.autoWeapon && value.maps?.[0]?.sha256 === map.sha256 ? value : false;
    }, 'schema 3 saved automation flags');
    evidence.persistedSettings = saved;
    assert.equal(saved.schemaVersion, 3);
    await page.getByRole('button', { name: 'Automatyka', exact: true }).click();
    for (const name of ['Wykrywaj pola HUD: czas, kompas oraz jawne etykiety pojazdu, amunicji i mapy',
      'Wybieraj konkretny profil z nazwy pojazdu i amunicji', 'Szukaj widocznej mapy w lokalnej bibliotece i przeliczaj jej pozycje']) {
      assert(await page.getByLabel(name, { exact: true }).isChecked());
    }
  });
  await check('Actual Automatic UI produces distance and trajectory from fresh map/HUD markers', async () => {
    await page.getByRole('button', { name: 'Profile i balistyka', exact: true }).click();
    await page.getByLabel('Cel wyżej niż stanowisko (jedn.)', { exact: true }).fill('0');
    await page.getByLabel('Potwierdzam ręcznie podaną różnicę wysokości celu i stanowiska', { exact: true }).check();
    await page.getByRole('button', { name: 'Analiza obrazu', exact: true }).click();
    await page.getByRole('button', { name: 'Odśwież okna', exact: true }).click();
    await poll(async () => (await page.locator('.capture-select select option').allTextContents()).includes('MTC HUD fixture'), 'fixture in window chooser');
    await page.locator('.capture-select select').selectOption(String(handle));
    await page.getByRole('button', { name: 'Automatic', exact: false }).click();
    await page.getByRole('button', { name: 'Rozpocznij OCR', exact: true }).click();
    const latest = await poll(async () => {
      const value = await invoke('latest');
      return value.mode === 'automatic' && value.result?.solution?.arcs?.length > 0 ? value : false;
    }, 'automatic UI result with native trajectory', 30000, 200);
    evidence.uiLatest = latest;
    closeTo(latest.result.bearing.distance, Math.hypot(190, 100));
    assert(latest.result.solution.arcs.every(arc => Number.isFinite(arc.elevationDeg)));
    assert(latest.positionsTimestamp <= Date.now() && Date.now() - latest.positionsTimestamp < 2000);
    await page.getByRole('button', { name: 'Automatyka', exact: true }).click();
    await page.waitForFunction(() => document.querySelector('.automation-status')?.textContent.includes('pola ustabilizowane')
      && document.querySelector('.automation-status')?.textContent.includes('jednoznaczne dopasowanie'));
    await page.screenshot({ path: resolve(output, 'map-hud-automatic.png'), fullPage: true });
  });
  await check('Closing the owned source window invalidates Automatic output', async () => {
    fixture.kill();
    await Promise.race([new Promise(accept => fixture.exitCode !== null ? accept() : fixture.once('exit', accept)), pause(5000)]);
    fixture = undefined;
    await poll(async () => !(await invoke('latest'))?.result, 'source close clears published result', 8000);
    await rejection('analyse_batch', { request: { handle, backend: 'wgc', config, calibration, scope: 'closed-source', maxSpeed: 500,
      includePreview: false, fps: 3, automation: { autoHud: true, autoMap: true } } }, 'closed owned HWND');
    await page.getByRole('button', { name: 'Analiza obrazu', exact: true }).click();
    await page.getByRole('button', { name: 'Zatrzymaj', exact: true }).click();
  });
  diagnostics.final = await invoke('diagnostics');
  assert.equal(webviewEvents.filter(event => event.event === 'crash' || event.event === 'pageerror').length, 0);
  passed = checks.every(item => item.status === 'passed');
} catch (error) {
  failure = String(error.stack ?? error);
  if (page && !page.isClosed()) {
    try { await page.screenshot({ path: resolve(output, 'map-hud-failure.png'), fullPage: true }); } catch {}
  }
} finally {
  const nativeExitBeforeCleanup = child?.exitCode;
  await browser?.close().catch(() => {});
  fixture?.kill();
  child?.kill();
  if (child) await Promise.race([new Promise(accept => child.exitCode !== null ? accept() : child.once('exit', accept)), pause(5000)]);
  const pending = checks.filter(item => item.status === 'pending-final-build').length;
  const report = { passed, preliminary: !!options.preliminary, status: failure ? 'failed' : pending ? 'pending-final-build' : 'passed',
    checks, evidence, diagnostics, error: failure, exe, exeSha256, fixtureExe, fixtureSha256, settingsDirectory, webviewDirectory,
    pid: child?.pid, nativeExitBeforeCleanup, stderr, fixtureStderr, webviewEvents, date: new Date().toISOString() };
  await writeFile(resolve(output, 'map-hud-smoke.json'), JSON.stringify(report, null, 2) + '\n');
  console.log(JSON.stringify({ passed, status: report.status, pending, checks, error: failure, exeSha256, fixtureSha256 }, null, 2));
  if (failure || (!options.preliminary && !passed)) process.exitCode = 1;
}
