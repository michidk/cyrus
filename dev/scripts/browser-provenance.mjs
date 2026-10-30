// CDP observes executed code, including worker WASM, instead of refetching URLs.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';

const digest = bytes => createHash('sha256').update(bytes).digest('hex');
const required = [
  'index.html', 'loading.css', 'loader.js', 'install.js', 'local-install.js',
  'game-assets.js', 'asset-worker.js', 'pkg/cyrus-web.js', 'pkg/cyrus-web_bg.wasm',
  'pkg/cyrus-assets-worker.js', 'pkg/cyrus-assets-worker_bg.wasm',
];

export function verifyBrowserArtifacts(build, observed) {
  assert.equal(build.version, 1, 'browser provenance version');
  for (const name of required) {
    const key = 'target/web/' + name;
    assert(observed[key], `browser did not observe loaded artifact: ${name}`);
  }
  for (const [name, value] of Object.entries(observed)) {
    assert.equal(value.sha256, build.artifacts[name], `loaded browser artifact differs from build: ${name}`);
  }
  return { build, observed };
}

export async function observeBrowserBuild(cdp, build, pageUrl) {
  const base = new URL('./', pageUrl);
  const observed = {}, pending = new Set(), failures = [];
  const resources = new Map();
  function pathFor(url) {
    const parsed = new URL(url);
    if (parsed.origin !== base.origin || !parsed.pathname.startsWith(base.pathname)) return null;
    return 'target/web/' + (parsed.pathname.slice(base.pathname.length) || 'index.html');
  }
  function record(name, bytes, method) {
    const sha256 = digest(bytes);
    assert.equal(sha256, build.artifacts[name], `loaded browser artifact differs from build: ${name}`);
    observed[name] = { sha256, method };
  }
  function track(task) {
    const promise = task().catch(error => failures.push(error)).finally(() => pending.delete(promise));
    pending.add(promise);
  }
  const stop = cdp.onEvent(message => {
    const { method, params, sessionId } = message;
    if (method === 'Target.attachedToTarget') {
      track(async () => {
        await cdp.call('Debugger.enable', {}, params.sessionId);
        await cdp.call('Runtime.runIfWaitingForDebugger', {}, params.sessionId);
      });
    } else if (method === 'Debugger.scriptParsed') {
      const wasm = params.scriptLanguage === 'WebAssembly';
      const name = params.url && pathFor(params.url);
      if (!wasm && (!name || !name.endsWith('.js'))) return;
      track(async () => {
        const source = await cdp.call('Debugger.getScriptSource', { scriptId: params.scriptId }, sessionId);
        if (wasm) {
          assert(source.bytecode, 'browser did not expose compiled WASM bytes');
          const bytes = Buffer.from(source.bytecode, 'base64');
          const sha256 = digest(bytes);
          const matches = Object.entries(build.artifacts).filter(([key, hash]) => key.endsWith('.wasm') && hash === sha256);
          assert.equal(matches.length, 1, 'loaded WASM does not identify one recorded build artifact');
          record(matches[0][0], bytes, 'Debugger.getScriptSource bytecode');
        } else record(name, Buffer.from(source.scriptSource), 'Debugger.getScriptSource');
      });
    } else if (method === 'Network.responseReceived' && ['Document', 'Stylesheet'].includes(params.type)) {
      const name = pathFor(params.response.url);
      if (name) resources.set(params.requestId, name);
    } else if (method === 'Network.loadingFinished' && resources.has(params.requestId)) {
      const name = resources.get(params.requestId);
      resources.delete(params.requestId);
      track(async () => {
        const response = await cdp.call('Network.getResponseBody', { requestId: params.requestId }, sessionId);
        record(name, Buffer.from(response.body, response.base64Encoded ? 'base64' : 'utf8'), 'Network.getResponseBody');
      });
    }
  });
  await cdp.call('Network.enable');
  await cdp.call('Network.setCacheDisabled', { cacheDisabled: true });
  await cdp.call('Debugger.enable');
  // Pausing a new service worker prevents registration readiness during application startup.
  await cdp.call('Target.setAutoAttach', {
    autoAttach: true, waitForDebuggerOnStart: true, flatten: true,
    filter: [{type: 'service_worker', exclude: true}, {}],
  });
  return {
    async verify() {
      while (pending.size) await Promise.all([...pending]);
      if (failures.length) throw failures[0];
      return verifyBrowserArtifacts(build, observed);
    },
    stop,
  };
}
