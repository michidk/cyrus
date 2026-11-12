import assert from 'node:assert/strict';
import {mkdtempSync, readFileSync, rmSync, writeFileSync, existsSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {pinBookAudioReferences} from './book-audio-references.mjs';

const root = mkdtempSync(join(tmpdir(), 'book-references-'));
try {
  const source = join(root, 'source.wav'), output = join(root, 'evidence');
  const bytes = Buffer.from('retained exact installed reference');
  writeFileSync(source, bytes);
  const [pinned] = pinBookAudioReferences([source], output);
  rmSync(source);
  assert.deepEqual(readFileSync(pinned), bytes);
  const [record] = JSON.parse(readFileSync(join(output, 'references.json')));
  assert.equal(record.source, source);
  assert.equal(record.sha256.length, 64);
  assert.throws(() => pinBookAudioReferences([pinned], output), /EEXIST/);
  assert.deepEqual(readFileSync(pinned), bytes);
  const missingOutput = join(root, 'missing');
  assert.throws(() => pinBookAudioReferences([pinned, source], missingOutput), /ENOENT/);
  assert(!existsSync(missingOutput), 'all references must preflight before output creation');
  console.log('reference retention, missing preflight and overwrite controls passed');
} finally {rmSync(root, {recursive: true, force: true});}
