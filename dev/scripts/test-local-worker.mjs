// Node supplies read-only File/FileReaderSync stand-ins; browser checks use real handles.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const packageDir = path.resolve(process.argv[2] || 'target/web/pkg');
const reads = [];
let truncate = false, unreadable = false;
globalThis.File = class {
  constructor(bytes) { this.bytes = bytes; this.size = bytes.length; }
  slice(start, end) {
    assert.ok(end - start <= 1024 * 1024, 'local reads stay bounded');
    reads.push([start, end]);
    return { bytes: this.bytes.subarray(start, end) };
  }
};
globalThis.FileReaderSync = class {
  readAsArrayBuffer(blob) {
    if (unreadable) throw new Error('File no longer readable');
    const bytes = truncate ? blob.bytes.subarray(1) : blob.bytes;
    return bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.length);
  }
};
globalThis.XMLHttpRequest = class { constructor() { assert.fail('Local files must never use HTTP'); } };
globalThis.fetch = () => assert.fail('Local files must never fetch');
const { default: init, AssetLibrary, GameLibrary } = await import(pathToFileURL(path.join(packageDir, 'cyrus-assets-worker.js')));
await init({ module_or_path: fs.readFileSync(path.join(packageDir, 'cyrus-assets-worker_bg.wasm')) });
const bytes = Buffer.from('TITLE=local\n' + ' '.repeat(2 * 1024 * 1024));
const files = new Map([['test.ini', new File(bytes)]]);
const library = new AssetLibrary(files, '', () => {});
const entry = JSON.parse(library.catalog()).entries.find(entry => entry.path === 'test.ini');
assert.ok(entry, 'local files appear in viewer catalog');
assert.ok(reads.length >= 3, 'content hashes use bounded reads');
const preview = JSON.parse(library.preview(entry.id).preview);
assert.ok(JSON.stringify(preview).includes('local'), 'viewer decodes original local bytes');
library.free();
assert.throws(() => new AssetLibrary(new Map([['../escape.ini', new File(bytes)]]), '', () => {}), /path/);
assert.throws(() => new AssetLibrary(new Map([['a.ini', new File(bytes)], ['A.ini', new File(bytes)]]), '', () => {}), /duplicate/);
truncate = true;
assert.throws(() => new AssetLibrary(files, '', () => {}), /test.ini.*expected/s);
truncate = false;
const game = new GameLibrary(new Map([['WORLD.INI', new File(Buffer.from('[Worlds]\n'))]]), '', () => {});
unreadable = true;
assert.throws(() => game.worlds(), /WORLD.INI.*File no longer readable/s);
game.free();
console.log('Local WASM viewer decoding, bounded reads, validation and contextual errors passed with network access forbidden');
