// Import in a browser page with #loading, #label and #fill; uses synthetic files only.
import { chooseInstall } from '../web/install.js';

export async function testInstallPicker() {
  const check = (value, message) => { if (!value) throw new Error(message); };
  const tick = () => new Promise(resolve => setTimeout(resolve, 20));
  const until = async predicate => {
    for (let n = 0; n < 100; n++) { if (predicate()) return; await tick(); }
    throw new Error('Picker did not reach expected state');
  };
  const original = { fetch: globalThis.fetch, picker: globalThis.showDirectoryPicker,
    url: location.href, saved: localStorage.getItem('cyrus-install') };
  const paths = ['Redguard/RGFX.EXE', 'Redguard/fxart/test', 'Redguard/maps/test',
    'DOSBOX/3DfxSpl2.dll', 'game.gog', 'game.ins'];
  function directory(paths) {
    return { name: 'GOG', async *entries() {
      const groups = Map.groupBy(paths, path => path.split('/')[0]);
      for (const [name, children] of groups) {
        yield [name, children[0].includes('/')
          ? { kind: 'directory', ...directory(children.map(path => path.slice(name.length + 1))) }
          : { kind: 'file', getFile: async () => { await tick(); return new File(['test'], name); } }];
      }
    } };
  }
  async function start(picker) {
    document.body.innerHTML = '<div id="loading"><div id="label"></div><div id="bar"><div id="fill"></div></div></div>';
    history.replaceState(null, '', '?install=local');
    globalThis.showDirectoryPicker = picker;
    const result = chooseInstall();
    await until(() => document.querySelector('#install-picker button'));
    return { result, button: document.querySelector('#install-picker button') };
  }
  const status = () => document.querySelector('[role=status]').textContent;
  const error = () => document.querySelector('[role=alert]').textContent;
  try {
    globalThis.fetch = async () => new Response('{}');
    let active = await start(async () => { throw new DOMException('Cancelled', 'AbortError'); });
    active.button.click();
    await until(() => !active.button.disabled);
    check(status().includes('No folder selected'), 'Cancellation must explain how to continue');

    globalThis.showDirectoryPicker = async () => { throw new DOMException('Denied', 'NotAllowedError'); };
    active.button.click();
    await until(() => !active.button.disabled);
    check(error().includes('allow read access'), 'Permission denial must be actionable');

    globalThis.showDirectoryPicker = async () => directory(['wrong.txt']);
    active.button.click();
    await until(() => !active.button.disabled);
    check(error().includes('No Redguard'), 'Invalid selection must remain retryable');

    globalThis.showDirectoryPicker = async () => directory(paths);
    active.button.click();
    await until(() => status().includes('files found'));
    check(active.button.disabled, 'Selection must prevent duplicate operations');
    const source = await active.result;
    check(source.files.has('disc/game.gog'), 'Valid folder must reach the caller');
    check(!document.querySelector('#install-picker'), 'Accepted picker must close');
    check(document.querySelector('#label').textContent.includes('Preparing'), 'Success must replace the selection instructions');
    check(document.querySelector('#fill').classList.contains('indeterminate'), 'Worker startup must show activity');

    active = await start(undefined);
    const input = document.querySelector('#install-folder');
    input.click = () => {};
    active.button.click();
    input.dispatchEvent(new Event('cancel'));
    check(!active.button.disabled && status().includes('No folder'), 'Fallback cancellation must restore controls');
    const transfer = new DataTransfer();
    for (const path of paths) {
      const file = new File(['test'], path.split('/').at(-1));
      Object.defineProperty(file, 'webkitRelativePath', { value: 'GOG/' + path });
      transfer.items.add(file);
    }
    active.button.click();
    input.files = transfer.files;
    input.dispatchEvent(new Event('change'));
    check(status().includes('6 selected files'), 'Fallback must report selection progress');
    check((await active.result).files.has('RGFX.EXE'), 'Fallback must accept valid files');

    active = await start(undefined);
    document.querySelector('details').open = true;
    document.querySelector('#install-url').value = 'https://files.example/original/';
    globalThis.fetch = async () => new Response('{}', { status: 404 });
    document.querySelector('form').requestSubmit();
    await until(() => !active.button.disabled);
    check(error().includes('HTTP 404'), 'Server errors must remain visible and retryable');
    globalThis.fetch = async (url, options) => new Response(JSON.stringify(options?.method === 'HEAD' ? {} :
      { files: ['RGFX.EXE', 'fxart/test', 'maps/test'].map(path => ({ path })) }));
    document.querySelector('form').requestSubmit();
    check((await active.result).url === 'https://files.example/original/', 'Server selection must still work');
    return 'Passed: cancellation, permission denial, invalid folder retry, live progress, local handoff, fallback input, server failure and success.';
  } finally {
    globalThis.fetch = original.fetch;
    globalThis.showDirectoryPicker = original.picker;
    history.replaceState(null, '', original.url);
    if (original.saved === null) localStorage.removeItem('cyrus-install');
    else localStorage.setItem('cyrus-install', original.saved);
  }
}
