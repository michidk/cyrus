// Retain exact installed references before connecting to a runtime or mutating its cache.
import {createHash} from 'node:crypto';
import {mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import {join, resolve} from 'node:path';

export function pinBookAudioReferences(sources, output) {
  const references = sources.map(source => {
    const bytes = readFileSync(source);
    return {source: resolve(source), bytes, sha256: createHash('sha256').update(bytes).digest('hex')};
  });
  mkdirSync(output, {recursive: true});
  const records = references.map(({source, bytes, sha256}, index) => {
    const path = resolve(join(output, `reference-${index}.wav`));
    writeFileSync(path, bytes, {flag: 'wx'});
    return {source, path, sha256};
  });
  writeFileSync(join(output, 'references.json'), JSON.stringify(records, null, 2), {flag: 'wx'});
  return records.map(record => record.path);
}
