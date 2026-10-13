import assert from 'node:assert/strict';
import { localInstall, directoryEntries } from '../web/local-install.js';

const paths = ['Redguard/RGFX.EXE', 'Redguard/fxart/TEXBSI.001', 'Redguard/maps/START.RGM',
  'Redguard/glide2x_emu.ovl', 'DOSBOX/3DfxSpl2.dll', 'DOSBOX/glide2x_emu.ovl', 'game.gog', 'game.ins', 'Manual.pdf'];
const entries = paths.map(path => [path, new File(['test'], path.split('/').at(-1))]);
for (const prefix of ['', 'GOG/', 'Games/Redguard/app/', 'İstanbul/Games/']) {
  const { files } = localInstall(entries.map(([path, file]) => [prefix + path, file]));
  assert.deepEqual([...files.keys()], ['RGFX.EXE', 'fxart/TEXBSI.001', 'maps/START.RGM', 'glide2x_emu.ovl',
    'disc/game.gog', 'disc/game.ins', 'splash/3DfxSpl2.dll', 'splash/glide2x_emu.ovl']);
  assert.equal(files.get('disc/game.gog'), entries[6][1], 'keep File handles, do not copy or upload bytes');
  assert.equal(files.get('splash/glide2x_emu.ovl'), entries[3][1], 'install overlay takes precedence');
}
assert.ok(localInstall(entries.map(([path, file]) => [path.toUpperCase(), file])).files.has('disc/game.gog'));
assert.throws(() => localInstall([]), /No Redguard/);
assert.throws(() => localInstall(entries.filter(([path]) => path.startsWith('Redguard/'))), /outer GOG folder/);
assert.throws(() => localInstall(entries.filter(([path]) => path !== 'game.gog')), /game.gog/);
assert.throws(() => localInstall([...entries, ['redguard/rgfx.exe', new File([], 'duplicate')]]), /Duplicate/);
assert.throws(() => localInstall([...entries, ['../secret', new File([], 'secret')]]), /Invalid/);
assert.throws(() => localInstall([...entries, ...entries.map(([path, file]) => ['Other/' + path, file])]), /just one/);
assert.throws(() => localInstall([...entries, ['Redguard/disc/game.gog', new File([], 'game.gog')]]), /Conflicting/);
const handle = { async *entries() {
  yield ['subfolder', { kind: 'directory', async *entries() { yield ['one', { kind: 'file', getFile: async () => entries[0][1] }]; } }];
} };
const progress = [];
assert.deepEqual(await directoryEntries(handle, '', (count, path) => progress.push([count, path])),
  [['subfolder/one', entries[0][1]]]);
assert.deepEqual(progress, [[0, 'subfolder'], [0, 'subfolder/one'], [1, 'subfolder/one']]);
await assert.rejects(directoryEntries({ async *entries() {
  yield ['locked.txt', { kind: 'file', getFile: async () => { throw new DOMException('Denied', 'NotAllowedError'); } }];
} }), { name: 'NotAllowedError' });
console.log('Local folder mapping, read-only handles, nested directories, companion files and invalid selections passed');
