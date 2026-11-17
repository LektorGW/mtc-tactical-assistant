/**
 * Reproducible Windows validation using @oai/sky for real keyboard input.
 * Import in node_repl after initializing sky; never substitute raw SendInput.
 * Every press is a separate exported step after observing the target window.
 */
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { createRequire } from 'node:module';
import { mkdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import net from 'node:net';

const execFileAsync = promisify(execFile);
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const helper = path.join(root, 'scripts/test-window-controls.py');
const exactMainTitle = 'MTC Tactical Assistant';

async function inspect(session) {
  const { stdout } = await execFileAsync(session.python, [helper, 'inspect', '--pid', String(session.pid)], { cwd: root, windowsHide: true });
  const value = JSON.parse(stdout);
  if (path.resolve(value.path ?? '').toLowerCase() !== path.resolve(session.exe).toLowerCase()) {
    throw new Error('Test process is no longer the exact selected executable');
  }
  return value;
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

async function invoke(session, command, args = {}) {
  return session.main.evaluate(({ command, args }) => window.__TAURI_INTERNALS__.invoke(command, args), { command, args });
}

export async function beginWindowTest(options) {
  const session = {
    python: options.python ?? 'python', pid: options.pid, exe: options.exe,
    outputDir: options.outputDir, port: options.port ?? 9224, checks: [], scales: [], nativeChecks: [],
    windowSizes: options.windowSizes ?? { main: { width: 1220, height: 900, minimumWidth: 820 }, overlay: { width: 390, height: 310 } },
  };
  assert(Number.isInteger(session.pid) && session.pid > 0, 'A launched, isolated test PID is required');
  const require = createRequire(import.meta.url);
  const { chromium } = require(path.join(root, 'apps/desktop/node_modules/playwright'));
  session.browser = await chromium.connectOverCDP(`http://127.0.0.1:${session.port}`);
  const pages = session.browser.contexts().flatMap(context => context.pages());
  session.main = pages.find(page => !page.url().includes('overlay=1'));
  session.overlay = pages.find(page => page.url().includes('overlay=1'));
  assert(session.main && session.overlay, 'Expected main and overlay WebViews in isolated CDP endpoint');
  await session.main.waitForFunction(() => !!window.__TAURI_INTERNALS__ && !!document.querySelector('button'));
  session.initial = { os: await inspect(session), native: await invoke(session, 'diagnostics') };
  session.initial.webviews = {
    main: await session.main.evaluate(() => ({ width: innerWidth, height: innerHeight, devicePixelRatio })),
    overlay: await session.overlay.evaluate(() => ({ width: innerWidth, height: innerHeight, devicePixelRatio })),
  };
  assert(session.initial.native.shortcutsRegistered.every(Boolean), 'One of the actual global shortcuts failed registration; close conflicting instances');
  const main = session.initial.os.windows.filter(window => window.title === exactMainTitle);
  assert(main.length === 1, 'Expected exactly one main HWND owned by the isolated test PID');
  session.mainHandle = main[0].handle;
  return session;
}

export async function observeWindowTest(session, sky) {
  const ownWindows = await sky.list_windows();
  const expectedApp = `process:${session.exe}`.toLowerCase();
  const main = ownWindows.filter(window => window.title === exactMainTitle && window.app.toLowerCase() === expectedApp);
  assert(main.length === 1, 'Computer Use must return exactly one window of the selected executable');
  session.target = await sky.get_window({ id: main[0].id, app: main[0].app });
  session.observation = await sky.get_window_state({ window: session.target, include_screenshot: false, include_text: true });
  session.observedOs = await inspect(session);
  return { os: session.observedOs, native: await invoke(session, 'diagnostics') };
}

/** Call only after inspecting observeWindowTest's result in the prior cell. */
export async function pressWindowTest(session, sky, key, options = {}) {
  assert(['O', 'I', 'M'].includes(key), 'Only the three authorized application shortcuts are allowed');
  assert(session.observation, 'Observe and inspect the main window before each input');
  const before = await inspect(session);
  const main = before.windows.find(window => window.handle === session.mainHandle);
  const overlay = before.windows.find(window => window.title === 'MTC Tactical Overlay');
  const recovery = options.recovery === true;
  if (recovery) {
    if (session.recoveryFixture) {
      const fixture = await inspect({ ...session, pid: session.recoveryFixture.pid, exe: session.recoveryFixture.exe });
      const fixtureWindow = fixture.windows.find(window => window.handle === session.recoveryFixture.handle);
      assert(key === 'M' && main?.minimized && fixtureWindow?.visible && fixtureWindow.foreground
        && before.foregroundPid === fixture.pid && before.foregroundHandle === fixtureWindow.handle,
      'Aborting recovery key injection: foreground is not the exact test-owned HUD fixture and main is not minimized');
    } else {
      assert(key === 'M' && main?.minimized && overlay?.visible && overlay.foreground
        && before.foregroundPid === session.pid && before.foregroundHandle === overlay.handle,
      'Aborting recovery key injection: minimized main and foreground overlay are not owned by this exact test process');
    }
  } else {
    assert(before.foregroundPid === session.pid && before.foregroundHandle === session.mainHandle && main?.foreground && !main.minimized && main.visible,
      'Aborting key injection: foreground is not the visible main window of this exact test process');
  }
  const previous = await invoke(session, 'diagnostics');
  const target = session.observation.window;
  session.observation = null;
  try {
    await sky.press_key({ window: target, key: `Control_L+Shift_L+${key.toLowerCase()}` });
  } catch (error) {
    session.inputBlockedReason = String(error.message ?? error);
    session.inputFailures ??= [];
    session.inputFailures.push({ shortcut: `Ctrl+Shift+${key}`, status: 'input outcome unconfirmed', error: session.inputBlockedReason });
    throw error;
  }
  session.observation = await sky.get_window_state({ window: target, include_screenshot: false, include_text: true });
  // The handler and WebView event delivery are asynchronous; poll state, never send
  // a second shortcut when the outcome of the first is unknown.
  let after;
  for (let attempt = 0; attempt < 20; attempt++) {
    after = await invoke(session, 'diagnostics');
    if (key === 'M') {
      const current = await inspect(session);
      if (current.foregroundPid === session.pid && current.foregroundHandle === session.mainHandle
        && !current.windows.find(window => window.handle === session.mainHandle)?.minimized) break;
    } else if ((key === 'O' && after.overlayVisible !== previous.overlayVisible) || (key === 'I' && after.clickThrough !== previous.clickThrough)) break;
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  // Window style changes can complete after the global shortcut callback. Check
  // the settled state so a cached-visibility regression cannot pass transiently.
  await new Promise(resolve => setTimeout(resolve, 250));
  after = await invoke(session, 'diagnostics');
  const os = await inspect(session);
  const overlayAfter = os.windows.find(window => window.title === 'MTC Tactical Overlay');
  if (key === 'O') {
    assert(after.overlayVisible !== previous.overlayVisible, 'Ctrl+Shift+O did not toggle native overlay visibility');
    assert(overlayAfter?.visible === after.overlayVisible, 'Native visibility disagrees with IsWindowVisible');
    if (options.requireFocusRetained) assert(os.foregroundHandle === before.foregroundHandle && os.foregroundPid === session.pid, 'Overlay visibility shortcut took foreground away from the main panel');
  } else if (key === 'I') {
    assert(after.clickThrough !== previous.clickThrough, 'Ctrl+Shift+I did not toggle click-through state');
    assert(overlayAfter?.flags.transparentInput === after.clickThrough, 'Click-through state disagrees with WS_EX_TRANSPARENT');
    if (after.overlayVisible !== previous.overlayVisible || overlayAfter?.visible !== previous.overlayVisible) {
      const failure = { shortcut: 'Ctrl+Shift+I', scenario: 'preserve_visibility', before: previous, after, beforeOs: before, os, passed: false, error: 'Ctrl+Shift+I changed overlay visibility while toggling click-through' };
      session.checks.push(failure);
      session.findings ??= [];
      session.findings.push(failure);
      throw new Error(failure.error);
    }
  } else {
    assert(os.foregroundPid === session.pid && os.foregroundHandle === session.mainHandle, 'Ctrl+Shift+M did not keep the main panel in foreground');
    if (recovery) assert(!os.windows.find(window => window.handle === session.mainHandle)?.minimized, 'Ctrl+Shift+M did not restore the minimized main panel');
  }
  const result = { shortcut: `Ctrl+Shift+${key}`, scenario: recovery ? session.recoveryFixture ? 'restore_minimized_main_from_owned_fixture' : 'restore_minimized_main_from_own_overlay' : 'foreground_main', before: previous, after, beforeOs: before, os, focusRetained: key === 'O' ? os.foregroundHandle === before.foregroundHandle : null, passed: true };
  session.checks.push(result);
  return result;
}

/** Click only the observed titlebar minimize button of the owned foreground main. */
export async function minimizeWindowTest(session, sky, elementIndex) {
  assert(session.observation, 'Observe the main window and inspect its minimize button index first');
  const tree = session.observation.accessibility?.tree ?? '';
  assert(new RegExp(`^\\s*${elementIndex}\\s+przycisk Minimalizuj`, 'm').test(tree), 'The supplied index is not the freshly observed titlebar minimize button');
  const before = await inspect(session);
  assert(before.foregroundPid === session.pid && before.foregroundHandle === session.mainHandle, 'Minimize input requires foreground of this exact main window');
  const observed = session.observation;
  session.observation = null;
  await sky.click({ window: observed.window, element_index: elementIndex });
  await sky.get_window_state({ window: observed.window, include_screenshot: false, include_text: true });
  const os = await inspect(session);
  assert(os.windows.find(window => window.handle === session.mainHandle)?.minimized, 'Main did not minimize');
  return os;
}

/** Revised recovery test scope: activate only the visible overlay of the same PID. */
export async function observeRecoveryWindowTest(session, sky) {
  const windows = (await sky.list_windows()).filter(window => window.title === 'MTC Tactical Overlay' && window.app.toLowerCase() === `process:${session.exe}`.toLowerCase());
  assert(windows.length === 1, 'Expected exactly one returned visible overlay of the test executable');
  const target = await sky.get_window({ id: windows[0].id, app: windows[0].app });
  await sky.activate_window({ window: target });
  session.observation = await sky.get_window_state({ window: target, include_screenshot: false, include_text: true });
  const os = await inspect(session);
  assert(os.foregroundPid === session.pid && os.windows.find(window => window.title === 'MTC Tactical Overlay')?.foreground, 'The owned overlay did not become foreground; abort recovery input');
  return { os, native: await invoke(session, 'diagnostics') };
}

/** Use only a fixture process launched by this test, never an arbitrary app/game. */
export async function observeFixtureRecoveryTest(session, sky, fixture) {
  assert(path.resolve(fixture.exe).toLowerCase() === path.join(root, 'target/debug/examples/hud_fixture.exe').toLowerCase(), 'Recovery fixture must be the known executable built from this repository');
  const owned = await inspect({ ...session, pid: fixture.pid, exe: fixture.exe });
  const fixtureWindows = owned.windows.filter(window => window.title === 'MTC HUD fixture');
  assert(fixtureWindows.length === 1 && fixtureWindows[0].visible, 'Expected one visible fixture HWND at the exact test PID');
  const windows = (await sky.list_windows()).filter(window => window.title === 'MTC HUD fixture' && window.app.toLowerCase() === `process:${fixture.exe}`.toLowerCase());
  assert(windows.length === 1, 'Computer Use must return exactly one known fixture window');
  const target = await sky.get_window({ id: windows[0].id, app: windows[0].app });
  await sky.activate_window({ window: target });
  session.observation = await sky.get_window_state({ window: target, include_screenshot: false, include_text: true });
  const fresh = await inspect({ ...session, pid: fixture.pid, exe: fixture.exe });
  assert(fresh.foregroundPid === fixture.pid && fresh.foregroundHandle === fixtureWindows[0].handle, 'Fixture is not foreground; abort recovery input');
  session.recoveryFixture = { pid: fixture.pid, exe: fixture.exe, handle: fixtureWindows[0].handle };
  return { fixture: fresh, app: await inspect(session), native: await invoke(session, 'diagnostics') };
}

/** Programmatic own-application controls; no physical keyboard/game input. */
export async function testNativeWindowState(session) {
  for (const [visible, clickThrough] of [[false, false], [true, false], [true, true], [true, false], [false, false]]) {
    await invoke(session, 'overlay_control', { visible, clickThrough });
    await new Promise(resolve => setTimeout(resolve, 250));
    const diagnostics = await invoke(session, 'diagnostics');
    const os = await inspect(session);
    const overlay = os.windows.find(window => window.title === 'MTC Tactical Overlay');
    assert(overlay && overlay.visible === visible, 'Overlay native API visibility does not match IsWindowVisible');
    assert(diagnostics.overlayVisible === visible && diagnostics.clickThrough === clickThrough, 'Overlay diagnostics did not update');
    assert(overlay.flags.topmost, 'Overlay lost WS_EX_TOPMOST');
    assert(overlay.flags.transparentInput === clickThrough, 'WS_EX_TRANSPARENT does not match click-through setting');
    session.nativeChecks.push({ visible, clickThrough, diagnostics, os, passed: true });
  }
  return session.nativeChecks;
}

/** CDP only: no OS DPI setting changes and no input injection. */
export async function testWindowScales(session) {
  for (const [name, page, physicalWidth, physicalHeight] of [
    ['main', session.main, session.windowSizes.main.width, session.windowSizes.main.height],
    ['overlay', session.overlay, session.windowSizes.overlay.width, session.windowSizes.overlay.height],
  ]) {
    const cdp = await page.context().newCDPSession(page);
    try {
      for (const scenario of ['fixed_logical_window', 'fixed_physical_stress']) {
      for (const factor of [1, 1.25, 1.5, 2]) {
        const width = Math.round(physicalWidth / (scenario === 'fixed_physical_stress' ? factor : 1));
        const height = Math.round(physicalHeight / (scenario === 'fixed_physical_stress' ? factor : 1));
        await cdp.send('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: factor, mobile: false });
        await page.waitForTimeout(100);
        const metrics = await page.evaluate(() => {
          const documentWidth = document.documentElement.clientWidth;
          const root = document.querySelector('#root');
          const card = document.querySelector('.overlay-card');
          const elements = [...document.querySelectorAll('button,input,select')].filter(el => getComputedStyle(el).display !== 'none' && el.getBoundingClientRect().width > 0);
          return {
            width: innerWidth, height: innerHeight, devicePixelRatio,
            rootWidth: root?.getBoundingClientRect().width ?? 0,
            documentWidth, scrollWidth: document.documentElement.scrollWidth,
            scrollHeight: document.documentElement.scrollHeight,
            horizontalOverflow: document.documentElement.scrollWidth > documentWidth + 1,
            verticalOverflow: document.documentElement.scrollHeight > innerHeight + 1,
            card: card ? { width: card.getBoundingClientRect().width, height: card.getBoundingClientRect().height } : null,
            controls: elements.length,
            clippedControls: elements.filter(el => { const r = el.getBoundingClientRect(); return r.right > documentWidth + 1 || r.left < -1; }).map(el => ({ label: el.textContent?.trim().slice(0, 70) || el.getAttribute('aria-label') || el.tagName, width: el.getBoundingClientRect().width, right: el.getBoundingClientRect().right })),
          };
        });
        await page.screenshot({ path: path.join(session.outputDir, `${name}-${scenario}-${Math.round(factor * 100)}.png`), fullPage: true });
        session.scales.push({ surface: name, scalePercent: factor * 100, layoutScenario: scenario, method: 'CDP device metrics emulation', belowConfiguredMinimumWidth: name === 'main' && width < session.windowSizes.main.minimumWidth, metrics });
      }
      }
    } finally {
      await cdp.send('Emulation.clearDeviceMetricsOverride');
      await cdp.detach();
    }
  }
  return session.scales;
}

export async function finishWindowTest(session, options = {}) {
  await mkdir(session.outputDir, { recursive: true });
  session.final = { os: await inspect(session), native: await invoke(session, 'diagnostics') };
  const report = {
    executable: session.exe, pid: session.pid, port: session.port, at: new Date().toISOString(),
    initial: session.initial, hotkeys: session.checks, physicalHotkeysTested: session.checks.length,
    inputBlock: session.inputBlockedReason ?? null, nativeStateChecks: session.nativeChecks,
    inputFailures: session.inputFailures ?? [], windowSizes: session.windowSizes,
    findings: session.findings ?? [],
    scales: session.scales, final: session.final,
    limitations: [
      ...(session.checks.some(check => check.scenario?.startsWith('restore_minimized_main_from_')) ? [] : ['Ctrl+Shift+M recovery from a minimized main panel has not been physically tested in this run.']),
      'Scale emulation does not validate native WM_DPICHANGED, mixed-monitor movement or compositor transparency.',
      session.checks.length ? 'Physical shortcut input was restricted to test-owned application windows or the verified local fixture; no game interaction or registry installation occurred.' : 'No physical keyboard input, game interaction or registry installation occurred in this run.',
    ],
  };
  if (options.installer) {
    const { stdout } = await execFileAsync(session.python, [helper, 'payload', '--installer', options.installer, '--exe', session.exe, '--output-dir', session.outputDir], { cwd: root, windowsHide: true });
    report.installer = JSON.parse(stdout);
  }
  await writeFile(path.join(session.outputDir, 'window-controls-report.json'), JSON.stringify(report, null, 2) + '\n');
  await session.browser.close();
  return report;
}

async function commandLineReadOnly() {
  const args = Object.fromEntries(Array.from({ length: Math.floor((process.argv.length - 2) / 2) }, (_, i) => [process.argv[2 + i * 2], process.argv[3 + i * 2]]));
  assert(args['--exe'], 'Usage: node scripts/test-window-controls.mjs --exe <portable.exe> [--installer <setup.exe>] [--output-dir <tests/path>] [--port 9224]');
  const exe = path.resolve(args['--exe']);
  const outputDir = path.resolve(args['--output-dir'] ?? path.join(root, 'tests/windows-output/read-only'));
  const port = Number(args['--port'] ?? 9224);
  const python = args['--python'] ?? 'python';
  assert(Number.isInteger(port) && port >= 1024 && port <= 65535, 'Invalid CDP port');
  await new Promise((resolve, reject) => {
    const socket = net.connect({ host: '127.0.0.1', port });
    socket.once('connect', () => { socket.destroy(); reject(new Error(`CDP port ${port} is already in use; aborting to avoid another session`)); });
    socket.once('error', error => error.code === 'ECONNREFUSED' ? resolve() : reject(error));
  });
  const { stdout } = await execFileAsync(python, [helper, 'launch', '--exe', exe, '--output-dir', outputDir, '--port', String(port)], { cwd: root, windowsHide: true });
  const launched = JSON.parse(stdout);
  let session;
  try {
    let startupError;
    for (let attempt = 0; attempt < 30; attempt++) {
      try {
        session = await beginWindowTest({ python, pid: launched.pid, exe, outputDir, port, windowSizes: {
          main: { width: Number(args['--main-width'] ?? 1220), height: Number(args['--main-height'] ?? 900), minimumWidth: 820 },
          overlay: { width: Number(args['--overlay-width'] ?? 390), height: Number(args['--overlay-height'] ?? 310) },
        } });
        break;
      } catch (error) {
        startupError = error;
        await new Promise(resolve => setTimeout(resolve, 200));
      }
    }
    if (!session) throw startupError;
    session.inputBlockedReason = args['--physical-note'] ?? 'Read-only CLI run intentionally performs no physical keyboard input; use the separately observed @oai/sky steps for actual hotkeys.';
    await testNativeWindowState(session);
    await testWindowScales(session);
    const report = await finishWindowTest(session, args['--installer'] ? { installer: path.resolve(args['--installer']) } : {});
    console.log(JSON.stringify({ report: path.join(outputDir, 'window-controls-report.json'), nativeStateCases: report.nativeStateChecks.length, scaleCases: report.scales.length, physicalHotkeysTested: report.physicalHotkeysTested, installerExecuted: report.installer?.installerExecuted ?? false }, null, 2));
  } finally {
    if (session?.browser?.isConnected()) await session.browser.close();
    await execFileAsync(python, [helper, 'stop', '--pid', String(launched.pid), '--exe', exe], { cwd: root, windowsHide: true });
  }
}

if (typeof process !== 'undefined' && process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  commandLineReadOnly().catch(error => { console.error(error.message); process.exitCode = 1; });
}
