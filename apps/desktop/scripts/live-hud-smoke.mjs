import { chromium } from '@playwright/test';
import { spawn } from 'node:child_process';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import net from 'node:net';
import { resolve } from 'node:path';
import assert from 'node:assert/strict';

// Exercises the MTC weapon-panel, full-map (M) and Loadout readers of the exact
// release EXE on the real game captures in tests/fixtures/live-2026-10-15.
// Expected values come from that directory's manifest.json. Only our own isolated
// process is controlled: no game window, no desktop input, no global keys.
const root = resolve(import.meta.dirname, '../../..');
const options = {};
for (let index = 2; index < process.argv.length; index++) {
  const arg = process.argv[index];
  assert(arg.startsWith('--'), `Unexpected argument ${arg}`);
  options[arg.slice(2)] = process.argv[++index];
}
const exe = resolve(options.exe ?? resolve(root, 'release/MTC-Tactical-Assistant-0.5.1.exe'));
const output = resolve(root, options.output ?? 'tests/output/v0.5.1');
const fixtures = resolve(root, 'tests/fixtures/live-2026-10-15');
const fixtures16 = resolve(root, 'tests/fixtures/live-2026-10-16');
const port = Number(options.port ?? 9230);
assert(Number.isInteger(port) && port >= 1024 && port <= 65535, 'Invalid CDP port');
const endpoint = `http://127.0.0.1:${port}`;
const runId = `${Date.now()}-${process.pid}`;
const settingsDirectory = resolve(output, `live-hud-settings-${runId}`);
const webviewDirectory = resolve(output, `live-hud-webview-${runId}`);
// Embedded frames are derived from game captures; they stay in this run directory
// and are never copied into the release ZIP.
const framesDirectory = resolve(output, `live-hud-frames-${runId}`);
const pause = milliseconds => new Promise(accept => setTimeout(accept, milliseconds));
const sha = bytes => createHash('sha256').update(bytes).digest('hex');
await mkdir(framesDirectory, { recursive: true });
await new Promise((accept, reject) => {
  const probe = net.createServer();
  probe.once('error', () => reject(new Error(`Port ${port} is occupied by another test`)));
  probe.listen(port, '127.0.0.1', () => probe.close(accept));
});
const manifest = JSON.parse(await readFile(resolve(fixtures, 'manifest.json'), 'utf8'));
const manifest16 = JSON.parse(await readFile(resolve(fixtures16, 'manifest.json'), 'utf8'));
const exeSha256 = sha(await readFile(exe));
const checks = [];
const evidence = { manifest: { capturedAt: manifest.capturedAt, game: manifest.game, source: manifest.source }, manifest16: { capturedAt: manifest16.capturedAt, source: manifest16.source }, panels: [], map: null, loadout: [], ownPosition: [] };
const webviewEvents = [];
let child;
let browser;
let page;
let invoke;
let passed = false;
let failure;
let stderr = '';
let processError;

async function poll(task, label, timeout = 15000, interval = 100) {
  const started = Date.now();
  let lastError;
  while (Date.now() - started < timeout) {
    try { const value = await task(); if (value) return value; } catch (error) { lastError = String(error); }
    await pause(interval);
  }
  throw new Error(`Timed out: ${label}${lastError ? `; ${lastError}` : ''}`);
}
async function check(label, task) {
  try { await task(); checks.push({ label, status: 'passed' }); console.log(`PASS ${label}`); }
  catch (error) { checks.push({ label, status: 'failed', error: String(error) }); throw error; }
}
const closeTo = (actual, expected, tolerance, label) => assert(Number.isFinite(actual) && Math.abs(actual - expected) <= tolerance,
  `${label}: ${actual} should be within ${tolerance} of ${expected}`);

// Same composition as crates/vision/tests/mtc_live_fixtures.rs::embed: the crop is put
// back at its recorded client position in a neutral 1920×1080 frame.
async function embed(entry, directory = fixtures) {
  const [x, y] = entry.cropInClient1920x1080;
  const source = await readFile(resolve(directory, entry.file));
  const base64 = await page.evaluate(async ({ data, x, y }) => {
    const bytes = Uint8Array.from(atob(data), c => c.charCodeAt(0));
    const bitmap = await createImageBitmap(new Blob([bytes], { type: 'image/png' }), { colorSpaceConversion: 'none', premultiplyAlpha: 'none' });
    const canvas = new OffscreenCanvas(1920, 1080);
    const context = canvas.getContext('2d', { colorSpace: 'srgb' });
    context.fillStyle = 'rgb(96, 104, 92)';
    context.fillRect(0, 0, 1920, 1080);
    context.drawImage(bitmap, x, y);
    const png = new Uint8Array(await (await canvas.convertToBlob({ type: 'image/png' })).arrayBuffer());
    let text = '';
    for (let offset = 0; offset < png.length; offset += 0x8000) text += String.fromCharCode(...png.subarray(offset, offset + 0x8000));
    return btoa(text);
  }, { data: source.toString('base64'), x, y });
  const path = resolve(framesDirectory, `frame-${entry.file}`);
  const bytes = Buffer.from(base64, 'base64');
  await writeFile(path, bytes);
  return { path, sourceSha256: sha(source), frameSha256: sha(bytes) };
}

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
  await poll(() => invoke('diagnostics'), 'native diagnostics');

  const panelEntries = manifest.fixtures.filter(entry => entry.file.startsWith('weapon-panel-'));
  assert.equal(panelEntries.length, 8, 'manifest should list eight weapon-panel captures');
  for (const entry of panelEntries) {
    const expected = entry.expected;
    await check(`read_weapon_panel_file ${entry.file}: ${expected.weapon} / ${expected.ammo} (${expected.weapon === "UB-32" ? "profile community-ub32-1" : "no profile"}), ELEVATION ${expected.elevationDeg ?? '—'}, AZIMUTH ${expected.azimuthDeg ?? '—'}`, async () => {
      const frame = await embed(entry);
      const analysis = await invoke('read_weapon_panel_file', { path: frame.path });
      const panel = analysis.panel;
      evidence.panels.push({ file: entry.file, ...frame, backend: analysis.backend, status: panel.status, weapon: panel.weapon?.text,
        ammo: panel.ammo?.text, elevationDeg: panel.elevationDeg?.value ?? null, azimuthDeg: panel.azimuthDeg?.value ?? null,
        rangeMeters: panel.rangeMeters, rangeText: panel.rangeText, profile: analysis.profile?.status ?? null, profileId: analysis.profile?.profileId ?? null });
      assert.equal(analysis.backend, 'file');
      assert.equal(panel.frameWidth, 1920);
      assert.equal(panel.frameHeight, 1080);
      assert.equal(panel.weapon?.text, expected.weapon, 'weapon name');
      assert.equal(panel.ammo?.text, expected.ammo, 'ammunition name');
      for (const [field, value] of [['elevationDeg', expected.elevationDeg], ['azimuthDeg', expected.azimuthDeg]]) {
        if (value === null) assert.equal(panel[field] ?? null, null, `${field} must be absent`);
        else closeTo(panel[field]?.value, value, 0.001, field);
      }
      // RANGE "0000" means no measurement and must not become 0 m.
      assert.equal(panel.rangeMeters ?? null, expected.rangeMeters);
      assert.equal(panel.status, expected.elevationDeg === null ? 'partial' : 'ready');
      // Only UB-32 / S-5K1 exists in the community catalog; other panel weapons must not
      // be mapped to a guessed profile.
      if (expected.weapon === 'UB-32') {
        assert.equal(analysis.profile?.status, 'matched');
        assert.equal(analysis.profile?.profileId, 'community-ub32-1');
      } else assert.notEqual(analysis.profile?.status, 'matched', 'unknown weapon must not resolve to a profile');
      // A single file frame never reaches the two-frame confidence.
      if (panel.elevationDeg) assert(panel.elevationDeg.confidence < 0.85, 'single-frame confidence must stay below Automatic');
    });
  }

  const mapEntry = manifest.fixtures.find(entry => entry.file === 'full-map-zone11-frontlines.png');
  let mapReading;
  await check('read_mtc_map_file on the Zone 11 full map: Grid size 324 m, zone_11, 9×9 grid lines within 3 px', async () => {
    const expected = mapEntry.expected;
    mapReading = await invoke('read_mtc_map_file', { path: resolve(fixtures, mapEntry.file) });
    evidence.map = { file: mapEntry.file, status: mapReading.status, mapId: mapReading.mapId, gridSizeMeters: mapReading.gridSizeMeters,
      gridSizeOcr: mapReading.gridSizeOcr, grid: mapReading.grid, mapWidthMeters: mapReading.mapWidthMeters,
      mapHeightMeters: mapReading.mapHeightMeters, candidates: mapReading.candidates, warnings: mapReading.warnings };
    assert.equal(mapReading.status, 'ready');
    assert.equal(mapReading.mapId, expected.map);
    assert.equal(mapReading.gridSizeMeters, Number(expected.gridSizeText.match(/(\d+)/)[1]));
    assert.equal(mapReading.grid.columns, expected.columns.split(' ').length);
    assert.equal(mapReading.grid.rows, 9);
    for (const side of ['left', 'top', 'right', 'bottom']) closeTo(mapReading.grid[side], expected.gridLinesInCrop[side], 3, `grid ${side}`);
    closeTo(mapReading.grid.cellWidth, 64.1, 0.5, 'cell width');
    assert(mapReading.mapWidthMeters > 0 && mapReading.mapHeightMeters > 0);
  });
  await check('mtc_map_point maps objective pixels to sectors A5 and G5 (columns A–H, J; rows 1–9 top-down)', async () => {
    const a = await invoke('mtc_map_point', { reading: mapReading, point: { x: 86, y: 380 } });
    const g = await invoke('mtc_map_point', { reading: mapReading, point: { x: 460, y: 380 } });
    const j9 = await invoke('mtc_map_point', { reading: mapReading, point: { x: mapReading.grid.right - 5, y: mapReading.grid.bottom - 5 } });
    evidence.mapPoints = { a, g, j9 };
    assert.equal(a.sector, 'A5');
    assert.equal(g.sector, 'G5');
    assert.equal(j9.sector, 'J9');
    closeTo(g.world.x, mapReading.mapWidthMeters * (460 - mapReading.grid.left) / (mapReading.grid.right - mapReading.grid.left), 1e-6, 'world x');
    let outside;
    try { await invoke('mtc_map_point', { reading: mapReading, point: { x: 5, y: 5 } }); } catch (error) { outside = String(error); }
    assert(outside?.includes('poza siatką'), 'a point outside the grid must be rejected, not extrapolated');
  });
  await check('Weapon panel reader finds no angles on the full-map frame; map reader finds no grid on a panel frame', async () => {
    const mapFrame = await embed(mapEntry);
    const panel = await invoke('read_weapon_panel_file', { path: mapFrame.path });
    assert.equal(panel.panel.elevationDeg ?? null, null);
    assert.equal(panel.panel.azimuthDeg ?? null, null);
    const noMap = await invoke('read_mtc_map_file', { path: resolve(fixtures, 'weapon-panel-ub32-a.png') });
    evidence.negative = { panelOnMap: panel.panel.status, mapOnPanel: noMap.status };
    assert.notEqual(noMap.status, 'ready');
    assert.equal(noMap.mapId ?? null, null);
  });
  for (const entry of manifest.fixtures.filter(item => item.file.startsWith('loadout-'))) {
    const expected = entry.expected;
    await check(`read_loadout_specs_file ${entry.file}: ${expected.muzzleVelocityMps} m/s, damage ${expected.damage}, ${expected.penetrationMm} mm`, async () => {
      const specs = await invoke('read_loadout_specs_file', { path: resolve(fixtures, entry.file) });
      evidence.loadout.push({ file: entry.file, status: specs.status, slot: specs.slot, muzzleVelocityMps: specs.muzzleVelocityMps?.value,
        damage: specs.damage?.value, penetrationMm: specs.penetrationMm?.value, weaponName: specs.weaponName });
      assert.equal(specs.status, 'ready');
      assert.equal(specs.muzzleVelocityMps?.value, expected.muzzleVelocityMps);
      assert.equal(specs.damage?.value, expected.damage);
      assert.equal(specs.penetrationMm?.value, expected.penetrationMm);
      assert.equal(specs.projectileType, expected.type);
      assert.equal(specs.weaponName ?? null, expected.weaponName, 'weapon name must not be inferred');
    });
  }
  // 2026-10-16 (Rohkstov): M109 panel resolved through vehicle-aliases.json and the
  // exact ammunition; minimap player arrow (beta own position) on embedded crops.
  const m109 = manifest16.fixtures.find(entry => entry.file === 'weapon-panel-m109.png');
  await check(`read_weapon_panel_file weapon-panel-m109.png: ${m109.expected.weapon} / ${m109.expected.ammo} → ${m109.expected.profileId} (equivalent ${m109.expected.equivalentProfileIds.join(', ')})`, async () => {
    const expected = m109.expected;
    const frame = await embed(m109, fixtures16);
    const analysis = await invoke('read_weapon_panel_file', { path: frame.path });
    const panel = analysis.panel;
    evidence.panels.push({ file: m109.file, ...frame, status: panel.status, weapon: panel.weapon?.text, ammo: panel.ammo?.text,
      elevationDeg: panel.elevationDeg?.value ?? null, azimuthDeg: panel.azimuthDeg?.value ?? null, rangeMeters: panel.rangeMeters,
      profile: analysis.profile?.status ?? null, profileId: analysis.profile?.profileId ?? null,
      equivalents: (analysis.profile?.equivalents ?? []).map(e => e.profileId), evidenceText: analysis.profile?.evidence ?? [] });
    assert.equal(panel.weapon?.text, expected.weapon);
    assert.equal(panel.ammo?.text, expected.ammo);
    closeTo(panel.elevationDeg?.value, expected.elevationDeg, 0.001, 'elevationDeg');
    closeTo(panel.azimuthDeg?.value, expected.azimuthDeg, 0.001, 'azimuthDeg');
    assert.equal(panel.rangeMeters ?? null, null);
    assert.equal(panel.status, 'ready');
    assert.equal(analysis.profile?.status, 'matched');
    assert.equal(analysis.profile?.profileId, expected.profileId);
    assert.deepEqual((analysis.profile?.equivalents ?? []).map(e => e.profileId), expected.equivalentProfileIds);
  });
  const own = manifest16.fixtures.filter(entry => ['minimap-own-158.png', 'minimap-own-194.png', 'minimap-own-228.png'].includes(entry.file));
  assert.equal(own.length, 3);
  for (const entry of own) {
    await check(`read_own_position_file ${entry.file}: player arrow within 3 px of the label, Rohkstov world meters, no compass → unconfirmed`, async () => {
      const [x0, y0] = entry.cropInClient1920x1080;
      const frame = await embed(entry, fixtures16);
      const reading = await invoke('read_own_position_file', { path: frame.path, mapId: 'rohkstov' });
      const marker = reading.marker;
      evidence.ownPosition.push({ file: entry.file, ...frame, status: reading.status, mapId: reading.mapId, marker, warnings: reading.warnings });
      assert(marker, `no arrow found: ${reading.status} ${reading.warnings}`);
      const label = { x: entry.expected.arrowInCrop.x + x0, y: entry.expected.arrowInCrop.y + y0 };
      assert(Math.hypot(marker.pixel.x - label.x, marker.pixel.y - label.y) <= 3, `arrow ${JSON.stringify(marker.pixel)} vs label ${JSON.stringify(label)}`);
      assert.equal(reading.mapId, 'rohkstov');
      // The synthetic frame has no compass, so the heading cannot be confirmed.
      assert.equal(reading.status, 'unconfirmed');
      assert.equal(marker.compassDeg ?? null, null);
      assert(marker.confidence < 0.85, 'a single unconfirmed frame stays below Automatic');
      closeTo(marker.world.x, marker.fraction.x * 5398.56, 1e-6, 'world x');
      closeTo(marker.world.y, (1 - marker.fraction.y) * 5398.56, 1e-6, 'world y');
      const unknownMap = await invoke('read_own_position_file', { path: frame.path, mapId: null });
      assert.equal(unknownMap.marker?.world ?? null, null, 'without a chosen map there are no world coordinates');
    });
  }
  await check('read_own_position_file minimap-own-152.png (dimmed minimap, labelled "none") reports no position', async () => {
    const entry = manifest16.fixtures.find(item => item.file === 'minimap-own-152.png');
    assert.equal(entry.expected.status, 'none');
    const frame = await embed(entry, fixtures16);
    const reading = await invoke('read_own_position_file', { path: frame.path, mapId: 'rohkstov' });
    evidence.ownPosition.push({ file: entry.file, ...frame, status: reading.status, marker: reading.marker, warnings: reading.warnings });
    assert.equal(reading.marker ?? null, null);
    assert(['notFound', 'blocked'].includes(reading.status), reading.status);
  });
  await check('Celowanie (gra na żywo) tab renders panel, correction and map sections and blocks reading without a window', async () => {
    await page.getByRole('button', { name: 'Celowanie (gra na żywo)', exact: true }).click();
    for (const heading of ['Gra na żywo (MTC)', 'Korekta celowania', 'Mapa z gry']) {
      assert(await page.getByRole('heading', { name: heading, exact: true }).isVisible(), heading);
    }
    assert(await page.getByRole('button', { name: 'Czytaj panel broni (2 Hz)', exact: true }).isDisabled());
    assert(await page.getByRole('button', { name: 'Rozpoznaj mapę z gry (otwórz mapę klawiszem M)', exact: true }).isDisabled());
    assert.equal(await page.locator('.build-tag').textContent(), 'WERSJA 0.5');
    await page.screenshot({ path: resolve(output, 'live-aim-panel.png'), fullPage: true });
  });
  assert.equal(webviewEvents.length, 0, `WebView errors: ${JSON.stringify(webviewEvents)}`);
  passed = checks.every(item => item.status === 'passed');
} catch (error) {
  failure = String(error.stack ?? error);
  if (page && !page.isClosed()) {
    try { await page.screenshot({ path: resolve(output, 'live-hud-failure.png'), fullPage: true }); } catch {}
  }
} finally {
  await browser?.close().catch(() => {});
  child?.kill();
  if (child) await Promise.race([new Promise(accept => child.exitCode !== null ? accept() : child.once('exit', accept)), pause(5000)]);
  const report = { passed, status: failure ? 'failed' : 'passed', checks, evidence, error: failure, exe, exeSha256,
    fixtureDirectory: fixtures, fixtureDirectory16: fixtures16, framesDirectory, settingsDirectory, webviewDirectory, pid: child?.pid, stderr, webviewEvents,
    note: 'Embedded frames are derived from user-consented game captures and stay outside the release ZIP.', date: new Date().toISOString() };
  await writeFile(resolve(output, 'live-hud-smoke.json'), JSON.stringify(report, null, 2) + '\n');
  console.log(JSON.stringify({ passed, checks: checks.length, failed: checks.filter(c => c.status !== 'passed'), error: failure, exeSha256 }, null, 2));
  if (!passed) process.exitCode = 1;
}
