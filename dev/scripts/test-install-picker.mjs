// Import in a browser page with #loading, #label and #fill; uses synthetic files only.
import { chooseInstall } from '../web/install.js';

export async function testInstallPicker() {
  const check = (value, message) => { if (!value) throw new Error(message); };
  const until = async predicate => {
    for (let n = 0; n < 100; n++) {
      if (predicate()) return;
      await new Promise(resolve => setTimeout(resolve, 20));
    }
    throw new Error('Picker did not reach expected state');
  };
  const original = { fetch: globalThis.fetch, url: location.href, saved: localStorage.getItem('cyrus-install') };
  const paths = ['Redguard/RGFX.EXE', 'Redguard/fxart/test', 'Redguard/maps/test',
    'DOSBOX/3DfxSpl2.dll', 'game.gog', 'game.ins'];
  async function start(query = '') {
    document.body.innerHTML = '<div id="loading"><div id="label"></div><div id="bar"><div id="fill"></div></div></div>';
    history.replaceState(null, '', location.pathname + query);
    const result = chooseInstall();
    await until(() => document.querySelector('#install-picker button'));
    const button = document.querySelector('#install-picker button');
    const input = document.querySelector('#install-folder');
    let opened = false;
    input.click = () => { opened = true; };
    button.click();
    check(opened, 'Every browser must use the directory file input');
    input.dispatchEvent(new Event('cancel'));
    return { result, button, input };
  }
  function select(active, paths) {
    active.button.click();
    const transfer = new DataTransfer();
    for (const path of paths) {
      const file = new File(['test'], path.split('/').at(-1));
      Object.defineProperty(file, 'webkitRelativePath', { value: 'GOG/' + path });
      transfer.items.add(file);
    }
    active.input.files = transfer.files;
    active.input.dispatchEvent(new Event('change'));
  }
  const status = () => document.querySelector('[role=status]').textContent;
  const error = () => document.querySelector('[role=alert]').textContent;
  try {
    const requested = [];
    globalThis.fetch = async url => {
      requested.push(String(url));
      return new Response(JSON.stringify({ install: 'https://default.example/original/' }));
    };
    localStorage.setItem('cyrus-install', 'https://saved.example/original/');
    let active = await start();
    check(requested.every(url => url === './launch.json'), 'Defaults must not load game files');
    check(document.querySelector('#install-url').value === 'https://default.example/original/', 'Default only prefills the server field');
    check(!active.button.disabled && status().includes('No folder'), 'Cancellation must restore controls');
    select(active, ['wrong.txt']);
    await until(() => !active.button.disabled);
    check(error().includes('No Redguard'), 'Invalid selection must remain retryable');
    select(active, paths);
    check(status().includes('6 selected files'), 'Selection must show feedback');
    const source = await active.result;
    check(source.files.has('splash/3DfxSpl2.dll'), 'The DLL must reach the caller');
    check(source.files.has('disc/game.gog'), 'Disc files must reach the caller');
    check(!document.querySelector('#install-picker'), 'Accepted picker must close');
    check(document.querySelector('#label').textContent.includes('Preparing'), 'Success must replace selection instructions');

    requested.length = 0;
    active = await start('?install=https://explicit.example/original/');
    check(requested.every(url => url === './launch.json'), 'Explicit URLs must also show the picker');
    check(document.querySelector('#install-url').value === 'https://explicit.example/original/', 'Explicit URL prefills the server field');
    document.querySelector('details').open = true;
    globalThis.fetch = async () => new Response('{}', { status: 404 });
    document.querySelector('form').requestSubmit();
    await until(() => !active.button.disabled);
    check(error().includes('HTTP 404'), 'Server errors must remain visible and retryable');
    globalThis.fetch = async (url, options) => new Response(JSON.stringify(options?.method === 'HEAD' ? {} :
      { files: ['RGFX.EXE', 'fxart/test', 'maps/test'].map(path => ({ path })) }));
    document.querySelector('form').requestSubmit();
    check((await active.result).url === 'https://explicit.example/original/', 'Server selection must still work');
    return 'Passed: picker always shown, defaults only prefill, folder input, cancellation, invalid retry, DLL handoff and server connection.';
  } finally {
    globalThis.fetch = original.fetch;
    history.replaceState(null, '', original.url);
    if (original.saved === null) localStorage.removeItem('cyrus-install');
    else localStorage.setItem('cyrus-install', original.saved);
  }
}
