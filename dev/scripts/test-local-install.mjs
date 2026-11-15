import assert from 'node:assert/strict';
import { localInstall } from '../web/local-install.js';

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
assert.ok(localInstall(entries.filter(([path]) => path.startsWith('Redguard/'))).files.has('RGFX.EXE'),
  'base install selection does not require optional GOG media');
assert.ok(localInstall(entries.filter(([path]) => path !== 'game.gog')).files.has('disc/game.ins'));
assert.throws(() => localInstall([...entries, ['redguard/rgfx.exe', new File([], 'duplicate')]]), /Duplicate/);
assert.throws(() => localInstall([...entries, ['../secret', new File([], 'secret')]]), /Invalid/);
assert.throws(() => localInstall([...entries, ...entries.map(([path, file]) => ['Other/' + path, file])]), /just one/);
assert.throws(() => localInstall([...entries, ['Redguard/disc/game.gog', new File([], 'game.gog')]]), /Conflicting/);
const nestedSupport = entries.map(([path, file]) => [path.replace('DOSBOX/', '__support/dosbox/'), file]);
assert.ok(localInstall(nestedSupport).files.has('splash/3DfxSpl2.dll'), 'discover nested GOG support files');
const flat = entries.map(([path, file]) => ['GOG/Redguard/' + path.replace(/^Redguard\//, ''), file]);
assert.ok(localInstall(flat).files.has('disc/game.gog'), 'disc files may live alongside the executable');
const scattered = entries.map(([path, file]) => [path.startsWith('DOSBOX/') ? 'tools/emulation/' + path : path, file]);
assert.ok(localInstall(scattered).files.has('splash/3DfxSpl2.dll'), 'search all selected descendants');
assert.throws(() => localInstall([...nestedSupport, ['backup/3DfxSpl2.dll', new File([], '3DfxSpl2.dll')]]),
  /Multiple copies/, 'do not silently choose among ambiguous support files');
console.log('Nested GOG support directories, flat installations and ambiguous companions passed');

const cd = ['Redguard/RGFX.EXE', 'Redguard/fxart/TEXBSI.001', 'Redguard/maps/START.RGM',
  'Redguard/SOUND/test.sfx', 'Redguard/ENGLISH.RTX', 'CDROM/INTRO.SMK',
  'Soundtrack/01 Track 1.mp3', 'Soundtrack/02 Track 2.MP3', 'Soundtrack/cover.jpg']
  .map(path => [path, new File(['test'], path.split('/').at(-1))]);
for (const prefix of ['', 'Games/CD/', 'İstanbul/']) {
  const { files } = localInstall(cd.map(([path, file]) => [prefix + path, file]));
  for (const [name, index] of [['disc/INTRO.SMK', 5], ['disc/track02.mp3', 6], ['disc/track03.mp3', 7]]) {
    assert.equal(files.get(name), cd[index][1], 'retain original media File handles');
  }
  assert.equal(files.get('SOUND/test.sfx'), cd[3][1], 'retain installed speech/effects');
  assert.ok(!files.has('disc/cover.jpg'));
  assert.ok(!files.has('disc/game.gog'), 'CD selection does not borrow GOG files');
}
assert.ok(localInstall(cd.map(([path, file]) => [path.toUpperCase(), file])).files.has('disc/track02.mp3'));
console.log('CD companion mapping and base installs without optional GOG media passed');

assert.throws(() => localInstall([...cd, ['Redguard/disc/INTRO.SMK', new File([], 'conflict')]]), /Conflicting/);
assert.throws(() => localInstall([...cd, ['Soundtrack/01 Other.mp3', new File([], 'ambiguous')]]), /Conflicting/);
const unrelated = ['backup/CDROM/OTHER.SMK', 'backup/Soundtrack/03 Track 3.mp3']
  .map(path => [path, new File([], path.split('/').at(-1))]);
assert.ok(!localInstall([...cd, ...unrelated]).files.has('disc/OTHER.SMK'));
assert.ok(!localInstall([...cd, ...unrelated]).files.has('disc/track04.mp3'));
