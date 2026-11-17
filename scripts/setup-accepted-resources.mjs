// Add explicitly accepted community assets from the normal offline cache.
// No game window is captured and no test-data-dir or online sync is used.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { copyFile, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import net from 'node:net';
import { isAbsolute, join, relative, resolve } from 'node:path';
import { runInNewContext } from 'node:vm';

const root = resolve(import.meta.dirname, '..');
const require = createRequire(join(root, 'apps/desktop/package.json'));
const pause = milliseconds => new Promise(accept => setTimeout(accept, milliseconds));
const sha256 = value => createHash('sha256').update(value).digest('hex');

async function compile(path, dependencies = {}) {
  const ts = require('typescript');
  const output = ts.transpileModule(await readFile(path, 'utf8'), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
  }).outputText;
  const module = { exports: {} };
  runInNewContext(output, {
    module, exports: module.exports, TextEncoder,
    require: name => {
      if (!(name in dependencies)) throw new Error(`Unexpected validator dependency: ${name}`);
      return dependencies[name];
    },
  });
  return module.exports;
}

async function validators() {
  const types = await compile(join(root, 'apps/desktop/src/types.ts'));
  const dynamics = await compile(join(root, 'apps/desktop/src/dynamics-validation.ts'));
  const salvo = await compile(join(root, 'apps/desktop/src/salvo.ts'), { './types': types });
  const validation = await compile(join(root, 'apps/desktop/src/validation.ts'), {
    './types': types, './dynamics-validation': dynamics, './salvo': salvo,
  });
  return { ...validation, defaultSettings: types.defaultSettings };
}

export function mergeMissingResources(current, library) {
  const maps = [...current.maps];
  const terrains = [...current.terrains];
  const addedMaps = [], addedTerrains = [], skippedTerrains = [];
  for (const candidate of library.maps) {
    if (maps.some(map => map.id === candidate.id)) continue;
    maps.push(candidate); addedMaps.push(candidate.id);
  }
  for (const candidate of library.terrains) {
    if (terrains.some(terrain => terrain.mapId === candidate.mapId) || current.terrain?.mapId === candidate.mapId) continue;
    const map = maps.find(map => map.id === candidate.mapId);
    const sourceMap = library.maps.find(map => map.id === candidate.mapId);
    if (!map || map.width !== candidate.mapWidth || map.height !== candidate.mapHeight || map.unit !== candidate.unit
      || !sourceMap || map.sha256 !== sourceMap.sha256) {
      skippedTerrains.push({ id: candidate.mapId, reason: 'Existing map raster, dimensions or units differ from the source terrain alignment' });
      continue;
    }
    terrains.push(candidate); addedTerrains.push(candidate.mapId);
  }
  return { settings: { ...current, maps, terrains }, addedMaps, addedTerrains, skippedTerrains };
}

async function selfTest() {
  const { validateSettings, defaultSettings } = await validators();
  const map = id => ({ id, name: id, path: `C:/synthetic/${id}.png`, sha256: 'a'.repeat(64), width: 100, height: 100,
    unit: 'game_meters', source: 'SYNTHETIC regression only', confirmed: true,
    calibration: { unitsPerPixel: 1, northRotationDeg: 0, originPixel: { x: 0, y: 0 }, originWorld: { x: 0, y: 0 }, uncertaintyUnits: 0 } });
  const terrain = id => ({ mapId: id, path: `C:/synthetic/${id}-height.png`, sha256: 'b'.repeat(64), mapWidth: 100, mapHeight: 100,
    minHeight: 0, maxHeight: 100, unit: 'game_meters', source: 'SYNTHETIC regression only', confirmed: true, coordinateAlignmentConfirmed: true });
  const preserved = { ...map('existing'), calibration: { ...map('existing').calibration, unitsPerPixel: 2, northRotationDeg: 27 } };
  const current = validateSettings({ ...defaultSettings, maps: [preserved], terrains: [terrain('existing')] });
  const merged = mergeMissingResources(current, { maps: [map('existing'), map('new')], terrains: [terrain('existing'), terrain('new')] });
  assert.deepEqual(merged.settings.maps[0], preserved);
  assert.equal(merged.settings.terrains[0], current.terrains[0]);
  assert.deepEqual(merged.addedMaps, ['new']); assert.deepEqual(merged.addedTerrains, ['new']);
  assert.deepEqual(validateSettings(merged.settings).overrides, current.overrides);
  assert.equal(mergeMissingResources(merged.settings, { maps: [map('new')], terrains: [terrain('new')] }).addedMaps.length, 0);
  for (const patch of [{ sha256: 'c'.repeat(64) }, { width: 101 }, { unit: 'game_units' }]) {
    const mismatch = mergeMissingResources({ ...current, maps: [{ ...preserved, ...patch }], terrains: [] }, {
      maps: [map('existing')], terrains: [terrain('existing')],
    });
    assert.equal(mismatch.addedTerrains.length, 0); assert.equal(mismatch.skippedTerrains.length, 1);
  }
  assert.equal(mergeMissingResources({ ...current, terrain: terrain('new') }, {
    maps: [map('new')], terrains: [terrain('new')],
  }).addedTerrains.length, 0);
  console.log(JSON.stringify({ selfTest: 'passed', appProcessesStarted: 0, normalSettingsWritten: false }));
}

async function poll(check, label, timeoutMs = 30000) {
  const started = Date.now(); let lastError;
  while (Date.now() - started < timeoutMs) {
    try { const result = await check(); if (result) return result; } catch (error) { lastError = error; }
    await pause(100);
  }
  throw new Error(`Timeout waiting for ${label}${lastError ? `: ${lastError}` : ''}`);
}

async function freePort(port) {
  await new Promise((accept, reject) => {
    const probe = net.createServer();
    probe.once('error', reject);
    probe.listen(port, '127.0.0.1', () => probe.close(accept));
  });
}

function within(path, directory) {
  const difference = relative(resolve(directory), resolve(path));
  return !!difference && difference !== '..' && !difference.startsWith(`..${process.platform === 'win32' ? '\\' : '/'}`) && !isAbsolute(difference);
}

async function setup(options) {
  const { chromium } = require('@playwright/test');
  const { validateSettings } = await validators();
  const exe = resolve(root, options.exe);
  const output = resolve(root, options.output ?? 'tests/output/v0.5.1');
  const reportPath = join(output, 'normal-resources-setup.json');
  const port = Number(options.port ?? 9229);
  assert(Number.isInteger(port) && port >= 1024 && port <= 65535, 'Invalid CDP port');
  const runId = `${Date.now()}-${process.pid}`;
  const webviewDirectory = join(output, `webview-normal-resources-${runId}`);
  const endpoint = `http://127.0.0.1:${port}`;
  const report = { schemaVersion: 1, passed: false, date: new Date().toISOString(), acceptedByUser: true, exe, exeSha256: null,
    port, webviewDirectory, normalSettings: true, testDataDirUsed: false, onlineRequestsRequested: 0, framesCaptured: 0,
    settingsPath: null, migratedSettingsPath: null, backupPath: null, cacheRoot: null, cacheShaReport: [], errors: [], warnings: [],
    checks: [], cleanup: { processClosed: false, browserClosed: false, portReleased: false, privateWebviewRemoved: false } };
  let child, browser, page, processError;
  let stdout = '', stderr = '';
  const webviewErrors = [];
  await mkdir(output, { recursive: true });
  try {
    await freePort(port);
    report.exeSha256 = sha256(await readFile(exe));
    child = spawn(exe, [], {
      windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'],
      env: { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`, WEBVIEW2_USER_DATA_FOLDER: webviewDirectory },
    });
    report.pid = child.pid;
    child.once('error', error => { processError = error; });
    child.stdout.on('data', bytes => { stdout = (stdout + bytes.toString()).slice(-16000); });
    child.stderr.on('data', bytes => { stderr = (stderr + bytes.toString()).slice(-16000); });
    await poll(async () => {
      if (processError) throw processError;
      assert.equal(child.exitCode, null, `App exited before CDP: ${stderr}`);
      return (await fetch(`${endpoint}/json/version`, { signal: AbortSignal.timeout(1000) })).ok;
    }, 'own normal application CDP');
    browser = await chromium.connectOverCDP(endpoint);
    const context = browser.contexts()[0];
    page = await poll(async () => {
      for (const candidate of context.pages()) {
        if (candidate.url().startsWith('http://tauri.localhost/') && !candidate.url().includes('overlay=1')
          && await candidate.locator('h1').count()) return candidate;
      }
      return false;
    }, 'normal main WebView');
    page.on('pageerror', error => webviewErrors.push(String(error)));
    page.on('crash', () => webviewErrors.push('main WebView crashed'));
    const invoke = (command, args = {}) => page.evaluate(({ command, args }) => window.__TAURI_INTERNALS__.invoke(command, args), { command, args });
    await poll(async () => (await page.locator('.sidebar select').first().locator('option').count()) > 0, 'initial profile/settings load');
    await pause(1500); // Let the UI's initial 500 ms settings autosave finish.
    const [appData, localData] = await Promise.all([
      invoke('plugin:path|resolve_directory', { directory: 14 }), // @tauri-apps/api/path BaseDirectory.AppData
      invoke('plugin:path|resolve_directory', { directory: 5 }), // BaseDirectory.LocalData, matching native resources.rs
    ]);
    report.settingsPath = join(appData, 'settings-v4.json');
    report.migratedSettingsPath = report.settingsPath;
    report.cacheRoot = join(localData, 'MTC-Tactical-Assistant/community-assets-v1');
    const catalog = await invoke('resource_catalog');
    assert.equal(catalog.length, 25, 'Expected the pinned 25-map catalog');
    const expectedTerrain = catalog.filter(map => map.heightmapUrl !== null).length;
    assert.equal(expectedTerrain, 21, 'Expected 21 source heightmaps');
    const library = await invoke('sync_resources', { ids: catalog.map(map => map.id), includeTerrain: true, online: false, sourceAcknowledged: true });
    report.warnings = library.warnings;
    report.cacheShaReport = library.report.results;
    report.syncErrors = library.report.results.filter(result => result.error !== null);
    assert.equal(library.report.mode, 'offline');
    assert.equal(report.syncErrors.length, 0, 'Normal offline cache has failed or missing resources');
    assert.equal(library.maps.length, 25); assert.equal(library.terrains.length, 21);
    assert.equal(library.report.results.length, 46);
    report.checks.push('Native offline sync validates all 25 map and 21 source terrain cache hashes without network');
    // Read after the long cache check so the merge starts from the latest
    // normal settings, never from an earlier captured UI state.
    const current = validateSettings(await invoke('load_settings'));
    const beforeHash = sha256(JSON.stringify(current));
    const merged = mergeMissingResources(current, library);
    const settings = validateSettings(merged.settings);
    assert(Buffer.byteLength(JSON.stringify(settings)) <= 512000, 'Merged normal settings exceed the native size limit');
    report.before = { schemaVersion: current.schemaVersion, maps: current.maps.length, terrains: current.terrains.length,
      legacyTerrain: current.terrain?.mapId ?? null, settingsSha256: beforeHash, acceptedProfiles: Object.values(current.overrides).filter(profile => profile.sourceAcknowledged).length };
    report.addedMaps = merged.addedMaps; report.addedTerrains = merged.addedTerrains; report.skippedTerrains = merged.skippedTerrains;
    report.backupPath = join(appData, `settings-before-resource-setup-${runId}.json`);
    await copyFile(report.settingsPath, report.backupPath);
    // Observe only startup loader metadata. No raster bytes or frames enter
    // this instrumentation or the report. Tauri's invoke property is read-only;
    // observe its ordinary fetch response without replacing the native API.
    await page.addInitScript(() => {
      window.__MTC_RESOURCE_SETUP_LOADS__ = [];
      const original = window.fetch;
      window.fetch = function(input, init) {
        const url = typeof input === 'string' ? input : input?.url;
        if (url !== 'http://ipc.localhost/load_automation_assets') return original.call(this, input, init);
        let args = {};
        try { args = JSON.parse(init?.body ?? '{}'); } catch {}
        const record = { startedAtMs: Date.now(), maps: args.maps?.length ?? 0, terrains: args.terrains?.length ?? 0, result: null, error: null };
        window.__MTC_RESOURCE_SETUP_LOADS__.push(record);
        return original.call(this, input, init).then(response => {
          // Read a clone: the application's original Response remains intact.
          void response.clone().text().then(body => {
            if (response.headers.get('Tauri-Response') === 'ok') record.result = JSON.parse(body);
            else record.error = body;
            record.completedAtMs = Date.now();
          }).catch(error => { record.error = String(error); record.completedAtMs = Date.now(); });
          return response;
        }, error => { record.error = String(error); record.completedAtMs = Date.now(); throw error; });
      };
    });
    await invoke('save_settings', { value: settings });
    await page.reload({ waitUntil: 'domcontentloaded' }); // Prevent old UI state from autosaving over the just-written settings.
    const preload = await poll(async () => {
      const loads = await page.evaluate(() => window.__MTC_RESOURCE_SETUP_LOADS__ ?? []);
      const latest = loads.at(-1);
      if (latest?.error) throw new Error(latest.error);
      return latest?.result === settings.maps.length ? latest : false;
    }, 'normal startup native asset preload', 10000);
    report.startupPreload = preload;
    await pause(1500); // Include the reloaded UI's normal schema-v4 autosave.
    const reread = validateSettings(await invoke('load_settings'));
    assert.deepEqual(reread.maps, settings.maps); assert.deepEqual(reread.terrains, settings.terrains);
    assert.deepEqual(reread.terrain, current.terrain); assert.deepEqual(reread.overrides, current.overrides);
    for (const entry of current.maps) assert.deepEqual(reread.maps.find(map => map.id === entry.id), entry);
    for (const entry of current.terrains) assert.deepEqual(reread.terrains.find(terrain => terrain.mapId === entry.mapId), entry);
    const omitLibraries = value => { const { maps, terrains, ...rest } = value; return rest; };
    assert.deepEqual(omitLibraries(reread), omitLibraries(current), 'Setup must preserve all unrelated normal preferences');
    const diskSettings = validateSettings(JSON.parse(await readFile(report.settingsPath, 'utf8')));
    assert.deepEqual(diskSettings.maps, settings.maps); assert.deepEqual(diskSettings.terrains, settings.terrains);
    assert.equal(webviewErrors.length, 0, `WebView errors: ${webviewErrors.join('; ')}`);
    report.after = { schemaVersion: reread.schemaVersion, maps: reread.maps.length, terrains: reread.terrains.length,
      legacyTerrain: reread.terrain?.mapId ?? null, settingsSha256: sha256(JSON.stringify(reread)), mapIds: reread.maps.map(map => map.id), terrainIds: reread.terrains.map(terrain => terrain.mapId) };
    report.checks.push('Missing IDs merged without replacing existing maps, calibrations, terrains, profile acknowledgements or unrelated settings',
      'Immediate reload starts normal native asset loading successfully within 10 seconds', 'Native settings reread and disk persistence agree after the normal UI autosave');
    report.passed = true;
  } catch (error) {
    report.errors.push(String(error.stack ?? error));
  } finally {
    if (child) {
      if (child.exitCode === null && child.signalCode === null) child.kill(); // Only the exact process spawned above.
      await Promise.race([new Promise(accept => child.exitCode !== null || child.signalCode !== null ? accept() : child.once('exit', accept)), pause(5000)]);
      report.cleanup.processClosed = child.exitCode !== null || child.signalCode !== null;
    } else report.cleanup.processClosed = true;
    if (browser) {
      try { await browser.close(); report.cleanup.browserClosed = true; } catch (error) { report.errors.push(`CDP cleanup: ${error}`); }
    } else report.cleanup.browserClosed = true;
    try { await poll(async () => { await freePort(port); return true; }, 'own CDP port release', 5000); report.cleanup.portReleased = true; }
    catch (error) { report.errors.push(String(error)); }
    if (report.cleanup.processClosed && report.cleanup.portReleased) {
      assert(within(webviewDirectory, output) && webviewDirectory.includes(`webview-normal-resources-${runId}`));
      try {
        // WebView descendants can briefly retain LOCK files after the exact
        // app PID and CDP port close. Retry only this run's private directory.
        await rm(webviewDirectory, { recursive: true, force: true, maxRetries: 10, retryDelay: 500 });
        report.cleanup.privateWebviewRemoved = true;
      }
      catch (error) { report.errors.push(`Private WebView cleanup: ${error}`); }
    }
    report.stdout = stdout; report.stderr = stderr; report.webviewErrors = webviewErrors;
    if (report.errors.length || !Object.values(report.cleanup).every(Boolean)) report.passed = false;
    await writeFile(reportPath, JSON.stringify(report, null, 2) + '\n');
    console.log(JSON.stringify({ passed: report.passed, exeSha256: report.exeSha256, before: report.before, after: report.after,
      addedMaps: report.addedMaps?.length, addedTerrains: report.addedTerrains?.length, cleanup: report.cleanup, errors: report.errors, output: reportPath }, null, 2));
    if (!report.passed) process.exitCode = 1;
  }
}

const args = process.argv.slice(2);
if (args.includes('--self-test')) {
  assert.deepEqual(args, ['--self-test'], 'Self-test accepts no application/setup options');
  await selfTest();
} else if (args.includes('--help')) {
  console.log('node scripts/setup-accepted-resources.mjs --accepted-by-user --exe release/MTC-Tactical-Assistant-0.5.1.exe [--output tests/output/v0.5.1] [--port 9229]');
} else {
  assert(args.includes('--accepted-by-user'), 'Run only after explicit user source acceptance: --accepted-by-user');
  const options = {};
  for (let index = 0; index < args.length; index++) {
    if (args[index] === '--accepted-by-user') continue;
    const key = args[index].replace(/^--/, '');
    assert(['exe', 'output', 'port'].includes(key) && args[index].startsWith('--'), `Unknown option ${args[index]}`);
    assert(args[index + 1] && !args[index + 1].startsWith('--') && !(key in options), `Missing or repeated option ${key}`);
    options[key] = args[++index];
  }
  assert(options.exe, 'An explicit final --exe is required');
  await setup(options);
}
