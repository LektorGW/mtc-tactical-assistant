import { chromium } from '@playwright/test';
import { spawn } from 'node:child_process';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import net from 'node:net';
import { resolve } from 'node:path';
import assert from 'node:assert/strict';

const root = resolve(import.meta.dirname, '../../..');
const argumentsList = process.argv.slice(2);
let positionalExe;
const options = {};
for (let index = 0; index < argumentsList.length; index++) {
  const value = argumentsList[index];
  if (value.startsWith('--')) options[value.slice(2)] = argumentsList[++index];
  else if (!positionalExe) positionalExe = value;
  else throw new Error(`Unexpected argument ${value}`);
}
const exe = resolve(options.exe ?? positionalExe ?? resolve(root, 'release/MTC-Tactical-Assistant-0.3.0.exe'));
const output = resolve(root, options.output ?? 'tests/output/v0.3.0');
const port = Number(options.port ?? 9225);
assert(Number.isInteger(port) && port >= 1024 && port <= 65535, 'Invalid CDP port');
const endpoint = `http://127.0.0.1:${port}`;
const runId = `${Date.now()}-${process.pid}`;
const settingsDirectory = resolve(output, `settings-${runId}`);
const webviewDirectory = resolve(output, `webview-${runId}`);
await mkdir(output, { recursive: true });
await new Promise((accept, reject) => {
  const probe = net.createServer();
  probe.once('error', () => reject(new Error(`Port ${port} is occupied; close the isolated test using it`)));
  probe.listen(port, '127.0.0.1', () => probe.close(accept));
});
const exeSha256 = createHash('sha256').update(await readFile(exe)).digest('hex');
const child = spawn(exe, ['--test-data-dir', settingsDirectory], {
  windowsHide: true,
  env: {
    ...process.env,
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`,
    WEBVIEW2_USER_DATA_FOLDER: webviewDirectory,
  },
  stdio: ['ignore', 'pipe', 'pipe'],
});
let stderr = '';
let stdout = '';
let processError;
child.once('error', error => { processError = error; });
child.stderr.on('data', data => { stderr = (stderr + data.toString()).slice(-64000); });
child.stdout.on('data', data => { stdout = (stdout + data.toString()).slice(-64000); });
const checks = [];
const evidence = {};
const webviewEvents = [];
let browser;
let page;
let passed = false;
let failure;
const pause = milliseconds => new Promise(accept => setTimeout(accept, milliseconds));
// Poll awaited native IPC in Node. Playwright's waitForFunction checks the
// truthiness of an async predicate's Promise before its boolean result.
async function waitForNative(check, label, timeout = 15000) {
  const started = Date.now();
  let lastError;
  while (Date.now() - started < timeout) {
    try {
      const value = await check();
      if (value) return value;
    } catch (error) { lastError = error; }
    await pause(100);
  }
  throw new Error(`Timed out waiting for ${label}${lastError ? `: ${lastError}` : ''}`);
}
const norm = vector => Math.hypot(...vector);
const closeTo = (actual, expected, tolerance = 1e-7) => assert(Math.abs(actual - expected) < tolerance, `${actual} should be within ${tolerance} of ${expected}`);

// Synthetic observations used only in this isolated test. They are not MTC
// measurements and must never be shipped as verified weapon coefficients.
const measurementSource = 'TEST SYNTHETIC: v100 g10; no calibration of live MTC';
const samples = [[12, 2], [30, 4], [55, 7], [65, 9]].map(([angle, flightTime]) => ({
  range: 100 * Math.cos(angle * Math.PI / 180) * flightTime,
  elevationDeg: angle,
  flightTime,
  heightDelta: 100 * Math.sin(angle * Math.PI / 180) * flightTime - 5 * flightTime ** 2,
  unit: 'game_units', source: measurementSource,
}));
const fitOptions = { unit: 'game_units', maxResidualUnits: 0.000001, maxRelativeResidual: 0.00000001, minSamples: 3, source: measurementSource, gameVersion: 'synthetic integration fixture' };

function reconstruct(interception, relative, targetVelocity, speed = 100, gravity = 10) {
  assert(interception?.arcs.length > 0, 'Expected complete ballistic interception');
  for (const arc of interception.arcs) {
    closeTo(norm(arc.launchVelocity), speed, 0.00001);
    const t = arc.flightTime;
    const shot = arc.worldInitialVelocity.map(value => value * t);
    shot[2] -= 0.5 * gravity * t ** 2;
    for (let axis = 0; axis < 3; axis++) closeTo(shot[axis], relative[axis] + targetVelocity[axis] * t, 0.00001);
    assert(arc.missDistance < 0.00001, 'The actual calibrated-speed projectile should meet the target');
    assert(Number.isFinite(arc.elevationDeg) && Number.isFinite(arc.azimuthDeg));
  }
}

try {
  let connected = false;
  for (let attempt = 0; attempt < 100; attempt++) {
    if (processError) throw processError;
    assert(child.exitCode === null, `Test process exited before CDP (${child.exitCode}); ${stderr}`);
    try { if ((await fetch(`${endpoint}/json/version`)).ok) { connected = true; break; } } catch {}
    await pause(200);
  }
  assert(connected, 'The isolated application should expose a CDP diagnostic endpoint');
  browser = await chromium.connectOverCDP(endpoint);
  const context = browser.contexts()[0];
  const observePage = target => {
    target.on('close', () => webviewEvents.push({ event: 'close', url: target.url(), at: Date.now() }));
    target.on('crash', () => webviewEvents.push({ event: 'crash', url: target.url(), at: Date.now() }));
    target.on('pageerror', error => webviewEvents.push({ event: 'pageerror', error: String(error), at: Date.now() }));
  };
  context.pages().forEach(observePage);
  context.on('page', observePage);
  for (let attempt = 0; attempt < 100 && !page; attempt++) {
    for (const candidate of context.pages()) {
      if (candidate.url().startsWith('http://tauri.localhost/') && !candidate.url().includes('overlay=1')) {
        try { if (await candidate.locator('h1').count()) { page = candidate; break; } } catch {}
      }
    }
    if (!page) await pause(100);
  }
  assert(page, 'Embedded main application WebView should be available');
  const invoke = (command, args = {}) => page.evaluate(({ command, args }) => window.__TAURI_INTERNALS__.invoke(command, args), { command, args });
  await waitForNative(() => invoke('diagnostics'), 'native diagnostics');
  const rejects = async (command, args, label) => {
    let error;
    try { await invoke(command, args); } catch (caught) { error = String(caught); }
    assert(error, `Expected rejection: ${label}`);
    evidence.rejections ??= [];
    evidence.rejections.push({ label, error });
    checks.push(label);
  };
  const profiles = await invoke('profiles');
  const community = profiles.filter(profile => profile.sourceMetadata);
  assert.equal(profiles.length, 160);
  assert.equal(community.length, 153);
  assert(community.every(profile => !profile.calibrated && !profile.sourceAcknowledged));
  checks.push('Loads the unchanged 153 experimental projectiles and seven local templates without implicit acknowledgement');
  for (const profile of community) {
    const resolved = await invoke('resolve_weapon', { vehicle: { text: profile.name, confidence: 0.97 }, ammunition: null });
    assert.equal(resolved.status, 'matched', profile.name);
    assert.equal(resolved.profileId, profile.id);
  }
  checks.push('Native resolver matches every one of the 153 exact full projectile names');
  const vehicleOnly = await invoke('resolve_weapon', { vehicle: { text: 'BM-21 Grad', confidence: 0.96 }, ammunition: null });
  assert.equal(vehicleOnly.status, 'ambiguous');
  assert.equal(vehicleOnly.profileId, null);
  assert.equal(vehicleOnly.candidates.length, 1);
  checks.push('Vehicle-only text blocks ammunition selection even when the current catalog has one variant');
  const identified = await invoke('resolve_weapon', { vehicle: { text: 'BM-21 Grad', confidence: 0.96 }, ammunition: { text: '9M22/M21 HE-Frag', confidence: 0.94 } });
  assert.equal(identified.status, 'matched');
  assert.equal(identified.profileId, 'community-bm_21_grad-1');
  closeTo(identified.confidence, 0.94);
  const conflict = await invoke('resolve_weapon', { vehicle: { text: 'BM-21 Grad', confidence: 0.96 }, ammunition: { text: 'M107 Low Charge', confidence: 0.95 } });
  assert.equal(conflict.status, 'no_match');
  const shared = await invoke('resolve_weapon', { vehicle: null, ammunition: { text: 'VOG-17M', confidence: 0.95 } });
  assert.equal(shared.status, 'ambiguous');
  evidence.resolver = { vehicleOnly, identified, conflict, shared };
  checks.push('Exact vehicle plus ammunition narrows the profile; contradictory and shared ammunition labels do not guess');

  const template = profiles.find(profile => profile.id === 'tube');
  assert(template);
  const fit = await invoke('fit_weapon', { profile: template, samples, options: fitOptions });
  closeTo(fit.fittedProfile.speed, 100);
  closeTo(fit.fittedProfile.gravity, 10);
  assert.equal(fit.fittedProfile.status, 'beta');
  assert.equal(fit.fittedProfile.calibrated, true);
  assert.equal(fit.fittedProfile.sourceAcknowledged, false);
  assert.equal(fit.fittedProfile.gameVersion, fitOptions.gameVersion);
  assert(fit.maxResidualUnits < 0.000001);
  evidence.fit = { samples, options: fitOptions, report: fit };
  checks.push('Native fit recovers synthetic v100/g10 from four different observations, reports residuals and keeps the calibration beta');
  await rejects('fit_weapon', { profile: template, samples: samples.slice(0, 2), options: fitOptions }, 'Fit rejects insufficient independent observations');
  await rejects('fit_weapon', { profile: template, samples: samples.map((sample, index) => index === 1 ? { ...sample, unit: 'game_meters' } : sample), options: fitOptions }, 'Fit rejects mixed measurement units');
  await rejects('fit_weapon', { profile: template, samples, options: { ...fitOptions, source: '' } }, 'Fit rejects missing dataset evidence');
  await rejects('fit_weapon', { profile: template, samples: samples.map((sample, index) => index === 0 ? { ...sample, source: '' } : sample), options: fitOptions }, 'Fit rejects an individual sample without provenance');
  await rejects('fit_weapon', { profile: template, samples: samples.map((sample, index) => index === 1 ? { ...sample, range: sample.range + 40 } : sample), options: fitOptions }, 'Fit rejects inconsistent observations instead of silently dropping them');

  const calibration = { unitsPerPixel: 1, northRotationDeg: 0, originPixel: { x: 0, y: 0 }, originWorld: { x: 0, y: 0 }, uncertaintyUnits: 1 };
  const calculation = { calibration, own: { x: 100, y: 200 }, target: { x: 500, y: 100 }, profile: fit.fittedProfile,
    heightDelta: 70, targetVelocity: [5, -2], ownVelocity: [10, 1], mapUnit: 'game_units', maxLeadTime: 30,
    terrainMapId: null, motion3d: { targetVerticalVelocity: 3, ownVerticalVelocity: -1, inheritOwnVelocity: true, maxTime: 30 } };
  const result3d = await invoke('calculate', { input: calculation });
  reconstruct(result3d.intercept3d, [400, 100, 70], [5, -2, 3]);
  assert.equal(result3d.intercept3d.arcs.length, 2);
  evidence.intercept3d = { input: calculation, result: result3d };
  checks.push('Native calculate reconstructs both gravity-aware intersections of a moving 3D target and platform');
  await rejects('calculate', { input: { ...calculation, heightDelta: null } }, '3D calculation rejects missing height instead of substituting zero');
  await rejects('calculate', { input: { ...calculation, targetVelocity: null } }, '3D calculation rejects missing horizontal target motion');
  await rejects('calculate', { input: { ...calculation, motion3d: { ...calculation.motion3d, targetVerticalVelocity: null } } }, '3D calculation rejects missing vertical target motion');
  await rejects('calculate', { input: { ...calculation, mapUnit: 'game_meters' } }, '3D calculation enforces consistent game units');
  await rejects('calculate', { input: { ...calculation, motion3d: { ...calculation.motion3d, maxTime: 0.1 } } }, '3D calculation blocks solutions beyond the observation horizon');
  await rejects('calculate', { input: { ...calculation, terrainMapId: 'not-loaded' } }, 'Terrain-dependent calculation blocks when its raster is absent');
  const guided = community.find(profile => profile.category === 'guided_missile');
  assert(guided);
  await rejects('fit_weapon', { profile: guided, samples, options: fitOptions }, 'Guided mechanics cannot be fitted with a fictional vacuum calibration');
  await rejects('calculate', { input: { ...calculation, profile: { ...guided, sourceAcknowledged: true }, mapUnit: 'game_meters' } }, 'Guided weapons remain without an interception model even after community acknowledgement');

  // Actual React form path: CSV -> Rust fit -> local beta profile, entirely
  // inside this test's WebView and settings directory.
  await page.locator('.sidebar select').selectOption('tube');
  await page.getByRole('button', { name: 'Automatyka', exact: true }).click();
  // React's controlled textarea can contribute its value to label.textContent;
  // anchor the visible label prefix so a second fill still targets this field.
  const csvField = page.locator('label.field').filter({ hasText: /^Pomiary \(game_units\)/ }).locator('textarea');
  await csvField.fill('100;10;2\n200;20;3\n300;30;4');
  await page.getByLabel('Źródło pomiarów', { exact: true }).fill(measurementSource);
  await page.getByRole('button', { name: 'Dopasuj i sprawdź model', exact: true }).click();
  await page.waitForFunction(() => document.querySelector('.notice')?.textContent.includes('wymagane cztery liczby'));
  assert.equal(await page.locator('.fit-result').count(), 0);
  checks.push('Actual CSV form rejects malformed rows before creating a calibration');
  const csv = samples.map(sample => [sample.range, sample.elevationDeg, sample.flightTime, sample.heightDelta].join(';')).join('\n');
  await csvField.fill(csv);
  await page.getByLabel('Wersja / data gry', { exact: true }).fill(fitOptions.gameVersion);
  await page.getByRole('button', { name: 'Dopasuj i sprawdź model', exact: true }).click();
  await page.waitForFunction(() => document.querySelector('.fit-result')?.textContent.includes('100.000') && document.querySelector('.fit-result')?.textContent.includes('10.000'));
  await page.screenshot({ path: resolve(output, 'automation-fit.png'), fullPage: true });
  await page.getByRole('button', { name: 'Zastosuj lokalną kalibrację beta', exact: true }).click();
  const stored = await waitForNative(async () => {
    const value = await invoke('load_settings');
    return value?.overrides?.tube?.status === 'beta' ? value : false;
  }, 'persisted beta calibration');
  assert((await page.locator('.profile-status').textContent()).includes('Beta'));
  evidence.persistedFit = stored;
  closeTo(stored.overrides.tube.speed, 100);
  closeTo(stored.overrides.tube.gravity, 10);
  assert.equal(stored.overrides.tube.sourceAcknowledged, false);
  // Wait beyond the app's 500 ms debounce and require the persisted fit to
  // survive a second read; this catches a stale settings write after applying.
  await pause(650);
  const rereadStored = await invoke('load_settings');
  evidence.persistedFitReread = rereadStored;
  assert.deepEqual(rereadStored?.overrides?.tube, stored.overrides.tube, 'Applied calibration must remain persisted after pending settings writes');
  checks.push('Actual CSV form invokes Rust, previews v100/g10 and persists an explicitly applied beta profile');

  // Inject only geometry/settings for an explicit stationary synthetic target.
  // Heights and all velocity components are confirmed through the actual form.
  await invoke('save_settings', { value: { ...stored, schemaVersion: 3, selected: 'tube', mode: 'manual', scale: 1,
    own: { x: 100, y: 200 }, target: { x: 500, y: 200 }, mapWidth: 800, mapHeight: 600,
    mapName: 'Synthetic automation test map', mapPath: null, mapUnit: 'game_units', unit: 'test game units',
    autoHud: false, autoMap: false, autoWeapon: false, maps: [], terrain: null,
    markerConfig: { ...stored.markerConfig, enabled: false, coordinateCalibrationConfirmed: false, viewSignature: null } } });
  await page.reload();
  await page.waitForSelector('h1');
  await page.getByRole('button', { name: 'Profile i balistyka', exact: true }).click();
  await page.getByLabel('Obliczaj przechwycenie 3D z grawitacją (wymaga wysokości i prędkości)', { exact: true }).check();
  await page.waitForFunction(() => document.querySelector('.warning-strip')?.textContent.includes('potwierdzonej różnicy wysokości'));
  await waitForNative(async () => !(await invoke('latest'))?.result, 'missing-height result invalidation');
  checks.push('Actual 3D form blocks output until observed height and complete motion are confirmed');
  for (const label of ['Cel wyżej niż stanowisko (jedn.)', 'Prędkość celu na wschód (jedn./s)', 'Prędkość celu na północ (jedn./s)',
    'Prędkość celu w górę (jedn./s)', 'Własna prędkość na wschód (jedn./s)', 'Własna prędkość na północ (jedn./s)', 'Własna prędkość w górę (jedn./s)']) {
    await page.getByLabel(label, { exact: true }).fill('0');
  }
  await page.getByLabel('Model wyprzedzenia dziedziczy prędkość stanowiska (przybliżenie, wymaga walidacji)', { exact: true }).check();
  await page.getByLabel('Potwierdzam ręcznie podaną różnicę wysokości celu i stanowiska', { exact: true }).check();
  await waitForNative(async () => (await invoke('latest'))?.result?.intercept3d?.arcs?.length > 0, 'published UI 3D interception');
  await page.getByText('Rozwiązanie 3D 1', { exact: true }).waitFor();
  const uiResult = (await invoke('latest')).result;
  reconstruct(uiResult.intercept3d, [400, 0, 0], [0, 0, 0]);
  evidence.uiIntercept3d = uiResult;
  await page.screenshot({ path: resolve(output, 'automation-3d.png'), fullPage: true });
  checks.push('Actual 3D form displays reconstructed ballistic arcs after explicit manual zero height and zero velocities');
  await page.getByLabel('Potwierdzam ręcznie podaną różnicę wysokości celu i stanowiska', { exact: true }).uncheck();
  await waitForNative(async () => !(await invoke('latest'))?.result, 'removed-height result invalidation');
  assert.equal(await page.getByText('Rozwiązanie 3D 1', { exact: true }).count(), 0);
  checks.push('Removing height confirmation clears previously computed 3D output');
  assert.equal(webviewEvents.filter(event => event.event === 'crash' || event.event === 'pageerror').length, 0, 'No WebView crash or unexpected frontend exception');
  passed = true;
} catch (error) {
  failure = String(error.stack ?? error);
  if (page && !page.isClosed()) {
    try { await page.screenshot({ path: resolve(output, 'automation-failure.png'), fullPage: true }); } catch {}
  }
} finally {
  await browser?.close().catch(() => {});
  child.kill();
  await Promise.race([new Promise(accept => child.exitCode !== null ? accept() : child.once('exit', accept)), pause(5000)]);
  const report = { passed, checks, evidence, error: failure ?? null, exe, exeSha256, pid: child.pid, port,
    settingsDirectory, webviewDirectory, physicalHotkeysTested: 0, dataKind: 'explicit synthetic integration fixtures; not observed MTC calibration',
    stderr, stdout, webviewEvents, date: new Date().toISOString() };
  await writeFile(resolve(output, 'automation-smoke.json'), JSON.stringify(report, null, 2) + '\n');
  console.log(JSON.stringify({ passed, checks: checks.length, output: resolve(output, 'automation-smoke.json'), exeSha256, physicalHotkeysTested: 0, error: failure ?? null }, null, 2));
}
if (!passed) process.exitCode = 1;
