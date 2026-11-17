import { chromium } from '@playwright/test';
import { spawn } from 'node:child_process';
import { copyFile, mkdir, readFile, rm, stat, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { isAbsolute, relative, resolve } from 'node:path';
import net from 'node:net';
import assert from 'node:assert/strict';

const root = resolve(import.meta.dirname, '../../..');
const options = {};
const args = process.argv.slice(2);
let positionalExe;
for (let index = 0; index < args.length; index++) {
  if (args[index].startsWith('--')) {
    const key = args[index].slice(2);
    assert(['exe', 'output', 'port', 'cache-root', 'ids'].includes(key), `Unknown option ${key}`);
    assert(args[index + 1] && !args[index + 1].startsWith('--'), `Missing value for ${key}`);
    options[key] = args[++index];
  } else { assert(!positionalExe, 'Only one positional executable is accepted'); positionalExe = args[index]; }
}
const exe = resolve(options.exe ?? positionalExe ?? resolve(root, 'release/MTC-Tactical-Assistant-0.5.1.exe'));
const output = resolve(root, options.output ?? 'tests/output/v0.5.1');
const port = Number(options.port ?? 9227);
assert(Number.isInteger(port) && port >= 1024 && port <= 65535, 'Invalid isolated CDP port');
const mapIds = (options.ids ?? 'normandy,chernobyl_v2').split(',');
assert(mapIds.length === 2 && new Set(mapIds).size === 2 && mapIds.every(id => /^[a-z0-9_]{1,80}$/.test(id)), 'This test requires two distinct catalog IDs with cached terrain');
const runId = `${Date.now()}-${process.pid}`;
const settingsDirectory = resolve(output, `settings-resources-${runId}`);
const webviewDirectory = resolve(output, `webview-resources-${runId}`);
const cacheDirectory = resolve(settingsDirectory, 'community-assets-v1');
const endpoint = `http://127.0.0.1:${port}`;
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
const pause = milliseconds => new Promise(accept => setTimeout(accept, milliseconds));
const closeTo = (actual, expected, tolerance = 1e-6) => assert(Math.abs(actual - expected) < tolerance, `${actual} differs from ${expected} by more than ${tolerance}`);
function within(path, parent) {
  const normalize = value => resolve(value.replace(/^\\\\\?\\/, ''));
  const difference = relative(normalize(parent), normalize(path));
  return difference.length > 0 && difference !== '..' && !difference.startsWith('..\\') && !difference.startsWith('../') && !isAbsolute(difference);
}
async function waitForNative(check, label, timeout = 30000) {
  const start = Date.now(); let lastError;
  while (Date.now() - start < timeout) {
    try { const value = await check(); if (value) return value; } catch (error) { lastError = error; }
    await pause(100);
  }
  throw new Error(`Timeout waiting for ${label}${lastError ? `: ${lastError}` : ''}`);
}

// Tauri's invoke property is read-only. Observe only these two ordinary IPC
// fetches and consume a Response clone; the application's response is intact.
function installIpcObserver() {
  if (window.__mtcResourceSmoke?.observerInstalled) return;
  window.__mtcResourceSmoke = { observerInstalled: true, syncCalls: [], simulations: [] };
  const original = window.fetch;
  window.fetch = function (input, init) {
    const url = typeof input === 'string' ? input : input?.url;
    const collection = url === 'http://ipc.localhost/sync_resources' ? window.__mtcResourceSmoke.syncCalls
      : url === 'http://ipc.localhost/projectile_simulation' ? window.__mtcResourceSmoke.simulations : null;
    if (!collection) return original.call(this, input, init);
    let args;
    try { args = JSON.parse(init?.body ?? '{}'); } catch { args = null; }
    const record = { args, startedAtMs: Date.now(), completedAtMs: null, ok: null, result: null, error: null };
    collection.push(record);
    return original.call(this, input, init).then(response => {
      void response.clone().text().then(body => {
        record.ok = response.headers.get('Tauri-Response') === 'ok';
        if (record.ok) record.result = JSON.parse(body);
        else record.error = body;
        record.completedAtMs = Date.now();
      }).catch(error => { record.error = String(error); record.completedAtMs = Date.now(); });
      return response;
    }, error => { record.error = String(error); record.completedAtMs = Date.now(); throw error; });
  };
}

let child, browser, page, exeSha256, sourceCacheRoot;
let childExited = false;
let stdout = '', stderr = '', processError;
const checks = [], evidence = {}, webviewEvents = [];
let passed = false, failure;
let cacheCopyOwned = false;
const copied = [];
const original = new Map();
const cleanup = { processClosed: false, copiedRasterCacheRemoved: false };
await mkdir(output, { recursive: true });

try {
  await new Promise((accept, reject) => {
    const probe = net.createServer();
    probe.once('error', () => reject(new Error(`CDP port ${port} is occupied`)));
    probe.listen(port, '127.0.0.1', () => probe.close(accept));
  });
  exeSha256 = sha256(await readFile(exe));
  let summary;
  try { summary = JSON.parse(await readFile(resolve(root, 'tests/output/v0.5.1/assets-summary.json'), 'utf8')); } catch {}
  const candidates = [options['cache-root'], summary?.cacheRoot,
    process.env.LOCALAPPDATA ? resolve(process.env.LOCALAPPDATA, 'MTC-Tactical-Assistant/community-assets-v1') : undefined].filter(Boolean);
  for (const candidate of candidates) {
    try { if ((await stat(resolve(candidate, `entries/${mapIds[0]}-map.json`))).isFile()) { sourceCacheRoot = candidate; break; } } catch {}
  }
  assert(sourceCacheRoot, 'No existing local cache found; use --cache-root after explicit resource setup');
  await mkdir(settingsDirectory); // Fresh, uniquely named test directory only.
  cacheCopyOwned = true;
  await mkdir(resolve(cacheDirectory, 'entries'), { recursive: true });
  await mkdir(resolve(cacheDirectory, 'blobs'), { recursive: true });
  for (const id of mapIds) {
    const records = {};
    for (const [suffix, kind] of [['map', 'mapImage'], ['heightmap', 'heightmap'], ['prepared-heightmap', 'preparedHeightmap']]) {
      const entryPath = resolve(sourceCacheRoot, `entries/${id}-${suffix}.json`);
      const record = JSON.parse(await readFile(entryPath, 'utf8'));
      assert.equal(record.id, id); assert.equal(record.kind, kind); assert.equal(record.schemaVersion, 1);
      assert.match(record.sha256, /^[a-f0-9]{64}$/);
      assert.equal(record.sourceMetadata?.commit, '39a7af7689524e57ca2326f3b998cf123df95426');
      const blobPath = resolve(sourceCacheRoot, `blobs/${record.sha256}.png`);
      assert(within(blobPath, sourceCacheRoot));
      const bytes = await readFile(blobPath);
      assert.equal(sha256(bytes), record.sha256, 'Source cache bytes must match their immutable record');
      assert.equal(bytes.length, record.byteLength);
      records[kind] = record;
      if (kind !== 'preparedHeightmap') {
        await copyFile(entryPath, resolve(cacheDirectory, `entries/${id}-${suffix}.json`));
        await copyFile(blobPath, resolve(cacheDirectory, `blobs/${record.sha256}.png`));
        copied.push({ id, kind, sha256: record.sha256, byteLength: bytes.length, url: record.url });
      }
    }
    assert.equal(records.preparedHeightmap.sourceSha256, records.heightmap.sha256, 'Original prepared terrain must be derived from the current source hash');
    original.set(id, records);
  }
  evidence.cacheCopy = { sourceCacheRoot, isolatedCache: cacheDirectory, copied, preparedRastersCopied: 0 };
  checks.push('Copies only two map and two source-terrain cache entries/blobs into a fresh private test directory, preserving actual SHA256');
  child = spawn(exe, ['--test-data-dir', settingsDirectory], {
    windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'],
    env: { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`, WEBVIEW2_USER_DATA_FOLDER: webviewDirectory },
  });
  child.once('error', error => { processError = error; });
  child.once('exit', () => { childExited = true; });
  child.stdout.on('data', bytes => { stdout = (stdout + bytes.toString()).slice(-64000); });
  child.stderr.on('data', bytes => { stderr = (stderr + bytes.toString()).slice(-64000); });
  await waitForNative(async () => {
    if (processError) throw processError;
    assert.equal(child.exitCode, null, `App exited before CDP: ${stderr}`);
    try { return (await fetch(`${endpoint}/json/version`)).ok; } catch { return false; }
  }, 'own application CDP endpoint');
  browser = await chromium.connectOverCDP(endpoint);
  const context = browser.contexts()[0];
  const observe = target => {
    target.on('crash', () => webviewEvents.push({ event: 'crash', url: target.url() }));
    target.on('pageerror', error => webviewEvents.push({ event: 'pageerror', error: String(error) }));
  };
  context.pages().forEach(observe); context.on('page', observe);
  page = await waitForNative(async () => {
    for (const candidate of context.pages()) {
      if (candidate.url().startsWith('http://tauri.localhost/') && !candidate.url().includes('overlay=1')) {
        try { if (await candidate.locator('h1').count()) return candidate; } catch {}
      }
    }
    return false;
  }, 'embedded main WebView');
  const invoke = (command, args = {}) => page.evaluate(({ command, args }) => window.__TAURI_INTERNALS__.invoke(command, args), { command, args });
  await waitForNative(() => invoke('diagnostics'), 'native IPC');
  const rejects = async (command, args, label) => {
    let error;
    try { await invoke(command, args); } catch (caught) { error = String(caught); }
    assert(error, `Expected native rejection: ${label}`);
    (evidence.rejections ??= []).push({ label, error }); checks.push(label);
  };
  const catalog = await invoke('resource_catalog');
  assert.equal(catalog.length, 25); assert.equal(catalog.filter(map => map.heightmapUrl && map.heightRangeGameMeters).length, 21);
  assert(catalog.every(map => map.status === 'experimental' && map.sourceMetadata.commit === '39a7af7689524e57ca2326f3b998cf123df95426'));
  checks.push('Native catalog contains 25 experimental maps and 21 available heightmaps with pinned source lineage');
  await rejects('sync_resources', { ids: ['unknown_test_map'], includeTerrain: false, online: false, sourceAcknowledged: true }, 'Unknown map IDs cannot produce a resource URL');
  await rejects('sync_resources', { ids: mapIds, includeTerrain: true, online: false, sourceAcknowledged: false }, 'Resource synchronization rejects missing source acknowledgement');
  const library = await invoke('sync_resources', { ids: mapIds, includeTerrain: true, online: false, sourceAcknowledged: true });
  assert.equal(library.maps.length, 2); assert.equal(library.terrains.length, 2); assert.equal(library.warnings.length, 0);
  assert.equal(library.report.mode, 'offline'); assert.equal(library.report.results.length, 4);
  assert(library.report.results.every(result => result.status === 'cached' && !result.error));
  for (const map of library.maps) {
    assert(within(map.path, cacheDirectory));
    assert.equal(map.sha256, original.get(map.id).mapImage.sha256);
    assert.equal(sha256(await readFile(map.path)), map.sha256);
  }
  for (const terrain of library.terrains) {
    assert(within(terrain.path, cacheDirectory));
    assert.equal(terrain.sha256, original.get(terrain.mapId).preparedHeightmap.sha256, 'Native preparation reproduces the actual original prepared SHA');
    assert.equal(sha256(await readFile(terrain.path)), terrain.sha256);
  }
  evidence.library = library;
  checks.push('Offline sync validates two real cached maps and recreates both prepared terrains with deterministic paths and matching native SHA256');
  assert.equal(await invoke('load_automation_assets', { maps: library.maps, terrains: library.terrains, terrain: null }), 2);
  const terrainEvidence = [];
  for (const terrain of library.terrains) {
    const map = library.maps.find(map => map.id === terrain.mapId);
    const dataUrl = `data:image/png;base64,${(await readFile(terrain.path)).toString('base64')}`;
    const pixels = await page.evaluate(async dataUrl => {
      const image = new Image(); image.src = dataUrl; await image.decode();
      const canvas = document.createElement('canvas'); canvas.width = image.naturalWidth; canvas.height = image.naturalHeight;
      const context = canvas.getContext('2d', { willReadFrequently: true }); context.drawImage(image, 0, 0);
      const data = context.getImageData(0, 0, canvas.width, canvas.height).data;
      let low, high;
      for (let index = 0; index < data.length; index += 4) {
        if (data[index + 3] !== 255) continue;
        const pixel = { x: index / 4 % canvas.width, y: Math.floor(index / 4 / canvas.width), red: data[index] };
        if (!low || pixel.red < low.red) low = pixel;
        if (!high || pixel.red > high.red) high = pixel;
      }
      return { low, high, width: canvas.width, height: canvas.height };
    }, dataUrl);
    assert(pixels.low && pixels.high && pixels.high.red > pixels.low.red, 'Two fixture terrains must have observed nonconstant opaque red samples');
    const toMap = pixel => ({ x: pixel.x / (pixels.width - 1) * (map.width - 1), y: pixel.y / (pixels.height - 1) * (map.height - 1) });
    const own = toMap(pixels.low), target = toMap(pixels.high);
    const expected = (pixels.high.red - pixels.low.red) / 255 * (terrain.maxHeight - terrain.minHeight);
    const actual = await invoke('terrain_height', { mapId: map.id, unit: map.unit, own, target });
    closeTo(actual, expected, 1e-5);
    terrainEvidence.push({ mapId: map.id, own, target, pixels, expected, actual });
  }
  evidence.terrain = terrainEvidence;
  checks.push('Both terrain lookups agree with independently browser-decoded opaque RED8 samples in normalized reference-map coordinates');
  const first = library.terrains[0], firstSample = terrainEvidence.find(item => item.mapId === first.mapId);
  const secondSample = terrainEvidence.find(item => item.mapId !== first.mapId);
  const manual = { ...first, maxHeight: first.minHeight + 2 * (first.maxHeight - first.minHeight), source: 'TEST SYNTHETIC manual height-scale override; not observed MTC' };
  await invoke('load_automation_assets', { maps: library.maps, terrains: library.terrains, terrain: manual });
  closeTo(await invoke('terrain_height', { mapId: first.mapId, unit: first.unit, own: firstSample.own, target: firstSample.target }), firstSample.expected * 2, 1e-5);
  closeTo(await invoke('terrain_height', { mapId: secondSample.mapId, unit: first.unit, own: secondSample.own, target: secondSample.target }), secondSample.expected, 1e-5);
  checks.push('Manual terrain override replaces only its matching map while the second map keeps its own terrain');

  const profiles = await invoke('profiles');
  const template = profiles.find(profile => profile.id === 'tube'); assert(template);
  const base = { ...template, speed: 100, gravity: 10, calibrated: true, status: 'beta', source: 'TEST SYNTHETIC v100 g10; no current MTC calibration', gameVersion: 'synthetic .4 integration', unit: 'game_units', minElevationDeg: -10, maxElevationDeg: 89, dynamics: null };
  const firstMap = library.maps.find(map => map.id === first.mapId);
  const terrainInput = { calibration: firstMap.calibration, own: firstSample.own, target: firstSample.target,
    profile: { ...base, speed: 1000, unit: 'game_meters', source: 'TEST SYNTHETIC v1000 g10; terrain-lookup integration only' }, heightDelta: -999,
    targetVelocity: null, ownVelocity: null, mapUnit: 'game_meters', terrainMapId: first.mapId, currentMapId: first.mapId, motion3d: null };
  const terrainResult = await invoke('calculate', { input: terrainInput });
  const manualExpected = await invoke('calculate', { input: { ...terrainInput, terrainMapId: null, heightDelta: firstSample.expected * 2 } });
  assert(terrainResult.solution?.arcs.length > 0 && manualExpected.solution?.arcs.length > 0);
  closeTo(terrainResult.solution.arcs[0].elevationDeg, manualExpected.solution.arcs[0].elevationDeg);
  await rejects('calculate', { input: { ...terrainInput, currentMapId: secondSample.mapId } }, 'Terrain calculation blocks a mismatched current map instead of applying another terrain');
  await rejects('terrain_height', { mapId: first.mapId, unit: 'game_units', own: firstSample.own, target: firstSample.target }, 'Terrain lookup blocks incompatible game units');
  checks.push('Calculate uses the matching terrain/manual override instead of the deliberately incorrect manual height');
  await invoke('load_automation_assets', { maps: library.maps, terrains: library.terrains, terrain: null });

  const drag = (linear = 0, quadratic = 0) => ({ model: 'drag', unit: 'game_units', linearDrag: linear, quadraticDrag: quadratic,
    rocketAcceleration: null, burnTime: null, thrustDirectionFixedLaunch: null, calibrated: true, status: 'beta', source: 'TEST SYNTHETIC explicit dynamics; not measured MTC' });
  const profileZero = { ...base, dynamics: drag() };
  const calibration = { unitsPerPixel: 1, northRotationDeg: 0, originPixel: { x: 0, y: 0 }, originWorld: { x: 0, y: 0 }, uncertaintyUnits: 1 };
  const calculation = { calibration, own: { x: 100, y: 200 }, target: { x: 400, y: 200 }, profile: profileZero, heightDelta: 30,
    targetVelocity: null, ownVelocity: null, mapUnit: 'game_units', terrainMapId: null, currentMapId: null, motion3d: null };
  const vacuum = await invoke('calculate', { input: { ...calculation, profile: base } });
  const numeric = await invoke('calculate', { input: calculation });
  assert.equal(numeric.dynamicSolution.arcs.length, 2); assert.equal(numeric.solution.arcs.length, 2);
  numeric.dynamicSolution.arcs.forEach((arc, index) => {
    closeTo(arc.elevationDeg, vacuum.solution.arcs[index].elevationDeg, 0.02);
    assert(arc.missDistance < 0.05);
  });
  assert.equal(numeric.lead, null); assert.equal(numeric.intercept3d, null);
  evidence.numericZero = numeric;
  checks.push('Native calculate dispatches explicit zero-drag dynamics and agrees with both vacuum roots without fabricating motion lead');
  const nonzero = { ...base, dynamics: drag(0.06, 0.0002) };
  const changed = await invoke('calculate', { input: { ...calculation, profile: nonzero } });
  assert(changed.dynamicSolution?.arcs.length > 0);
  assert(Math.abs(changed.dynamicSolution.arcs[0].elevationDeg - vacuum.solution.arcs[0].elevationDeg) > 0.5);
  for (const arc of changed.dynamicSolution.arcs) {
    const flight = await invoke('projectile_simulation', { profile: nonzero, input: { elevationDeg: arc.elevationDeg, azimuthDeg: 90, unit: 'game_units', duration: arc.flightTime } });
    const end = flight.points.at(-1);
    assert(Math.hypot(end.position[0] - 300, end.position[1], end.position[2] - 30) < 0.06);
  }
  evidence.numericDrag = changed;
  checks.push('Nonzero drag changes the firing angle and every returned dynamic arc is reconstructed by the actual native simulation');
  const rocket = { ...base, category: 'rocket_artillery', dynamics: { ...drag(), model: 'rocket', rocketAcceleration: 20, burnTime: 1.27, thrustDirectionFixedLaunch: true } };
  const rocketInput = { elevationDeg: 31, azimuthDeg: 73, unit: 'game_units', duration: 5, options: { stepSeconds: 0.03, maxSteps: 50000 } };
  const rocketFlight = await invoke('projectile_simulation', { profile: rocket, input: rocketInput });
  assert(rocketFlight.points.some(point => Math.abs(point.time - 1.27) < 1e-12));
  const angle = 31 * Math.PI / 180, azimuth = 73 * Math.PI / 180;
  const direction = [Math.cos(angle) * Math.sin(azimuth), Math.cos(angle) * Math.cos(azimuth), Math.sin(angle)];
  const extra = 20 * (1.27 * 5 - 0.5 * 1.27 ** 2);
  rocketFlight.points.at(-1).position.forEach((value, index) => closeTo(value, direction[index] * (500 + extra) - (index === 2 ? 125 : 0), 1e-6));
  evidence.rocket = { input: rocketInput, end: rocketFlight.points.at(-1) };
  checks.push('Rocket preview preserves fixed launch direction and splits exactly at declared burnout, matching the synthetic closed form');
  const dynamicInput = { distance: 300, heightDelta: 30, azimuthDeg: 90, unit: 'game_units' };
  await rejects('dynamic_solve', { profile: { ...base, dynamics: { ...drag(), linearDrag: null } }, input: dynamicInput }, 'Dynamic solve rejects absent drag coefficients instead of inventing zero');
  await rejects('dynamic_solve', { profile: nonzero, input: { ...dynamicInput, unit: 'game_meters' } }, 'Dynamic solve rejects a unit mismatch');
  await rejects('dynamic_solve', { profile: { ...nonzero, calibrated: false, sourceAcknowledged: true }, input: dynamicInput }, 'Source acknowledgement cannot enable uncalibrated local dynamics');
  await rejects('projectile_simulation', { profile: { ...nonzero, dynamics: { ...drag(), source: '' } }, input: rocketInput }, 'Dynamics without observation provenance are rejected');
  const moving = await invoke('calculate', { input: { ...calculation, profile: nonzero, targetVelocity: [1, 0] } });
  assert.equal(moving.dynamicSolution, null); assert.equal(moving.solution, null); assert.equal(moving.lead, null); assert.equal(moving.intercept3d, null);
  assert(moving.ballisticWarning.includes('nieruchomego')); assert(moving.leadWarning.includes('nie obsługuje'));
  checks.push('Moving-target dynamics block numerical static trajectory and legacy constant-speed interception, preserving explicit warnings');
  // Since 0.5 the closed-form (vacuum) 3D intercept is skipped for drag/rocket profiles:
  // calculate keeps the bearing and points to the iterative dynamic_intercept instead of
  // failing the whole calculation. It must still never fall back to a vacuum answer.
  const motion3d = await invoke('calculate', { input: { ...calculation, profile: nonzero, targetVelocity: [0, 0], motion3d: { targetVerticalVelocity: 0, ownVerticalVelocity: 0, inheritOwnVelocity: false, maxTime: 30 } } });
  assert.equal(motion3d.solution, null); assert.equal(motion3d.dynamicSolution, null);
  assert.equal(motion3d.intercept3d, null); assert.equal(motion3d.lead, null);
  closeTo(motion3d.bearing.distance, 300, 1e-9);
  assert(motion3d.ballisticWarning.includes('dynamic_intercept'), motion3d.ballisticWarning);
  evidence.motion3dDynamics = motion3d;
  checks.push('A vacuum 3D interception cannot silently ignore active dynamics: calculate keeps the bearing and refers to dynamic_intercept');
  const interceptInput = { relative: [300, 0, 30], targetVelocity: [0, 2, 0], ownVelocity: [0, 0, 0], inheritOwnVelocity: false, unit: 'game_units', maxTime: 30 };
  const intercept = await invoke('dynamic_intercept', { profile: nonzero, input: interceptInput });
  assert(intercept.arcs.length > 0, 'Moving target with drag should have an iterative intercept');
  assert.equal(intercept.status, 'beta');
  for (const arc of intercept.arcs) {
    assert(arc.missDistance <= 0.05, `miss ${arc.missDistance}`);
    const expectedTarget = [300, 2 * arc.flightTime, 30];
    arc.targetPositionAtImpact.forEach((value, index) => closeTo(value, expectedTarget[index], 1e-6));
    const flight = await invoke('projectile_simulation', { profile: nonzero, input: { elevationDeg: arc.elevationDeg, azimuthDeg: arc.azimuthDeg, unit: 'game_units', duration: arc.flightTime, options: { stepSeconds: 0.02, maxSteps: 50000 } } });
    const end = flight.points.at(-1).position;
    assert(Math.hypot(end[0] - expectedTarget[0], end[1] - expectedTarget[1], end[2] - expectedTarget[2]) < 0.1, `simulated end ${end} vs ${expectedTarget}`);
  }
  await rejects('dynamic_intercept', { profile: nonzero, input: { ...interceptInput, ownVelocity: [5, 0, 0], inheritOwnVelocity: null } }, 'Dynamic intercept rejects nonzero platform motion without an explicit inheritance choice');
  evidence.dynamicIntercept = intercept;
  checks.push('Beta dynamic intercept (drag) hits a moving target and every arc is reconstructed by the native simulation');
  const missingHeight = await invoke('calculate', { input: { ...calculation, heightDelta: null } });
  assert.equal(missingHeight.dynamicSolution, null); assert.equal(missingHeight.solution, null);
  checks.push('Dynamic calculate blocks missing observed height instead of substituting zero');

  // Install now and before all following reloads, observing actual native fetch
  // completion rather than replacing invoke or its return values.
  await page.addInitScript(installIpcObserver);
  await page.evaluate(installIpcObserver);
  await page.getByRole('button', { name: 'Automatyka', exact: true }).click();
  const resources = page.locator('.map-resources-panel');
  await resources.locator('input[name=resourceMap]').first().waitFor();
  assert.equal(await resources.locator('input[name=resourceMap]').count(), 25);
  assert.equal(await resources.locator('input[name=resourceMap]:checked').count(), 0);
  assert.equal(await resources.locator('input[name=resourceOnline]').isChecked(), false);
  assert.equal(await page.evaluate(() => window.__mtcResourceSmoke.syncCalls.length), 0);
  checks.push('Actual resource panel mount reads catalog metadata without downloading or preselecting maps');
  for (const id of mapIds) await resources.locator(`input[name=resourceMap][value="${id}"]`).check();
  await resources.locator('input[name=resourceTerrain]').check();
  await resources.locator('input[name=resourceSourceAcknowledged]').check();
  await resources.getByRole('button', { name: 'Dodaj wybrane mapy z cache', exact: true }).click();
  const syncResponse = await waitForNative(async () => {
    const response = await page.evaluate(() => window.__mtcResourceSmoke.syncCalls.at(-1));
    return response?.completedAtMs !== null && response?.completedAtMs !== undefined ? response : false;
  }, 'resource panel actual native sync completion');
  assert.equal(syncResponse.ok, true, syncResponse.error ?? 'Native sync response must be successful');
  assert.equal(syncResponse.error, null);
  assert.equal(syncResponse.result.maps.length, 2);
  assert.equal(syncResponse.result.terrains.length, 2);
  await resources.getByText('Dodano do biblioteki: 2 map, 2 map wysokości.', { exact: true }).waitFor();
  await waitForNative(async () => await resources.locator('form').getAttribute('aria-busy') === 'false', 'resource panel awaited onInstall completion');
  const installed = await waitForNative(async () => {
    const value = await invoke('load_settings');
    return value?.maps?.length === 2 && value?.terrains?.length === 2
      && value.maps.every(map => syncResponse.result.maps.some(ready => ready.id === map.id && ready.sha256 === map.sha256))
      && value.terrains.every(terrain => syncResponse.result.terrains.some(ready => ready.mapId === terrain.mapId && ready.sha256 === terrain.sha256)) ? value : false;
  }, 'resource panel persisted map and terrain installation');
  assert.equal(installed.schemaVersion, 4);
  for (const terrain of installed.terrains) assert.equal(terrain.sha256, library.terrains.find(t => t.mapId === terrain.mapId).sha256);
  const requests = await page.evaluate(() => window.__mtcResourceSmoke.syncCalls);
  assert.equal(requests.length, 1); assert.equal(requests[0].args.online, false); assert.equal(requests[0].args.sourceAcknowledged, true);
  assert.deepEqual([...requests[0].args.ids].sort(), [...mapIds].sort());
  evidence.uiResourceRequest = requests[0].args;
  evidence.uiResourceSync = { startedAtMs: syncResponse.startedAtMs, completedAtMs: syncResponse.completedAtMs, ok: syncResponse.ok, maps: syncResponse.result.maps.length, terrains: syncResponse.result.terrains.length };
  await page.screenshot({ path: resolve(output, 'resources-panel.png'), fullPage: true });
  checks.push('Actual offline resource button synchronizes only selected maps and onInstall persists two maps plus two terrain records');

  await pause(650);
  const stored = await invoke('load_settings');
  await invoke('save_settings', { value: { ...stored, selected: 'tube', mode: 'manual', scale: 1, rotation: 0, own: { x: 100, y: 200 }, target: { x: 400, y: 200 },
    mapWidth: 800, mapHeight: 600, mapPath: null, mapName: 'TEST SYNTHETIC dynamic UI grid', mapUnit: 'game_units', unit: 'test game units',
    autoHud: false, autoMap: false, autoWeapon: false, terrain: null,
    overrides: { ...stored.overrides, tube: base }, markerConfig: { ...stored.markerConfig, enabled: false, coordinateCalibrationConfirmed: false, viewSignature: null } } });
  await page.reload(); await page.waitForSelector('h1');
  await waitForNative(() => invoke('diagnostics'), 'reloaded dynamic UI');
  await page.getByRole('button', { name: 'Profile i balistyka', exact: true }).click();
  const panel = page.locator('.dynamics-panel');
  await panel.getByRole('radio', { name: 'Opór powietrza', exact: true }).check();
  assert.equal(await panel.locator('input[name=linearDrag]').inputValue(), '');
  assert.equal(await panel.locator('input[name=quadraticDrag]').inputValue(), '');
  await panel.locator('input[name=linearDrag]').fill('0.06');
  await panel.locator('input[name=quadraticDrag]').fill('0.0002');
  await panel.locator('input[name=dynamicsSource]').fill('TEST SYNTHETIC UI declared drag; not measured MTC');
  await panel.locator('input[name=localDynamicsConfirmed]').check();
  await panel.getByRole('button', { name: 'Zastosuj model lotu', exact: true }).click();
  const applied = await waitForNative(async () => {
    const value = await invoke('load_settings');
    return value?.overrides?.tube?.dynamics?.linearDrag === 0.06 ? value : false;
  }, 'UI explicit drag beta persistence');
  assert.equal(applied.overrides.tube.source, base.source);
  assert.equal(applied.overrides.tube.dynamics.status, 'beta');
  assert.equal(applied.overrides.tube.dynamics.quadraticDrag, 0.0002);
  checks.push('Actual DynamicsPanel keeps empty coefficients until entry and applies explicit local beta without overwriting speed/gravity provenance');
  await panel.locator('input[name=previewElevation]').fill('31');
  await panel.locator('input[name=previewAzimuth]').fill('73');
  await panel.locator('input[name=previewDuration]').fill('5');
  await panel.locator('input[name=localDynamicsConfirmed]').check();
  await panel.getByRole('button', { name: 'Oblicz podgląd trajektorii', exact: true }).click();
  await panel.getByTestId('dynamics-preview').waitFor();
  const previewResponse = await waitForNative(async () => {
    const response = await page.evaluate(() => window.__mtcResourceSmoke.simulations.at(-1));
    return response?.completedAtMs !== null && response?.completedAtMs !== undefined ? response : false;
  }, 'UI native simulation response after reload');
  assert.equal(previewResponse.ok, true, previewResponse.error ?? 'Simulation response must be successful');
  assert.equal(previewResponse.error, null);
  assert.equal(previewResponse.args.profile.dynamics.linearDrag, 0.06);
  assert.equal(previewResponse.args.profile.dynamics.quadraticDrag, 0.0002);
  const previewEnd = previewResponse.result.points.at(-1);
  const previewText = await panel.getByTestId('dynamics-preview').textContent();
  assert(previewText.includes('Po 5.000 s'));
  for (const value of previewEnd.position) assert(previewText.includes(value.toFixed(3)), 'Endpoint display must use the actual native response');
  evidence.uiSimulation = { args: previewResponse.args, end: previewEnd, completedAtMs: previewResponse.completedAtMs };
  await page.screenshot({ path: resolve(output, 'dynamics-panel.png'), fullPage: true });
  checks.push('Actual DynamicsPanel requests native simulation and displays its trajectory endpoint');
  await page.locator('.sidebar select').selectOption('bm21');
  await page.getByTestId('dynamics-preview').waitFor({ state: 'detached' });
  await panel.getByRole('radio', { name: 'Opór powietrza', exact: true }).check();
  await panel.locator('input[name=linearDrag]').fill('0'); await panel.locator('input[name=quadraticDrag]').fill('0');
  await panel.locator('input[name=dynamicsSource]').fill('TEST SYNTHETIC blocked uncalibrated profile');
  await panel.locator('input[name=localDynamicsConfirmed]').check();
  await panel.getByRole('button', { name: 'Zastosuj model lotu', exact: true }).click();
  await panel.getByRole('alert').filter({ hasText: 'Najpierw przygotuj lokalną kalibrację beta' }).waitFor();
  assert.equal((await invoke('load_settings')).overrides.bm21, undefined);
  checks.push('Changing the selected profile immediately removes old preview and blocks dynamics on an uncalibrated weapon');
  assert.equal(webviewEvents.filter(event => event.event === 'crash' || event.event === 'pageerror').length, 0);
  passed = true;
} catch (error) {
  failure = String(error.stack ?? error);
  if (page && !page.isClosed()) { try { await page.screenshot({ path: resolve(output, 'resources-dynamics-failure.png'), fullPage: true }); } catch {} }
} finally {
  await browser?.close().catch(() => {});
  if (child) {
    if (!childExited && child.exitCode === null && child.signalCode === null) child.kill();
    await Promise.race([new Promise(accept => childExited || child.exitCode !== null || child.signalCode !== null ? accept() : child.once('exit', accept)), pause(5000)]);
    cleanup.processClosed = childExited || child.exitCode !== null || child.signalCode !== null;
    cleanup.exitCode = child.exitCode;
    cleanup.signalCode = child.signalCode;
    if (!cleanup.processClosed) { passed = false; failure ??= 'Own test process did not close; private copied raster cache retained until process is released'; }
  }
  // Never delete the original user cache. Remove only this fresh test's copy,
  // after process cleanup, so unconfirmed-license rasters cannot enter release evidence.
  if (cacheCopyOwned && (!child || cleanup.processClosed)) {
    assert.equal(cacheDirectory, resolve(settingsDirectory, 'community-assets-v1'));
    assert(within(cacheDirectory, settingsDirectory) && within(settingsDirectory, output));
    assert(!sourceCacheRoot || !within(sourceCacheRoot, settingsDirectory));
    try { await rm(cacheDirectory, { recursive: true, force: true }); cleanup.copiedRasterCacheRemoved = true; }
    catch (error) { passed = false; failure ??= `Failed to remove own copied raster cache: ${error}`; }
  }
  const report = { passed, checks, evidence, error: failure ?? null, exe, exeSha256, pid: child?.pid, port, settingsDirectory, webviewDirectory,
    sourceCacheRoot, cleanup, physicalHotkeysTested: 0, onlineRequestsRequested: 0,
    dynamicsDataKind: 'explicit synthetic integration models; not measurements of live MTC', terrainDataKind: 'private experimental community cache; alignment with live game unverified',
    stdout, stderr, webviewEvents, date: new Date().toISOString() };
  await writeFile(resolve(output, 'resources-dynamics-smoke.json'), JSON.stringify(report, null, 2) + '\n');
  console.log(JSON.stringify({ passed, checks: checks.length, exeSha256, cleanup, physicalHotkeysTested: 0, error: failure ?? null, output: resolve(output, 'resources-dynamics-smoke.json') }, null, 2));
}
if (!passed) process.exitCode = 1;
