// Export the retained GPU result, never a new screenshot of the live page.
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {existsSync, readFileSync, writeFileSync, appendFileSync, renameSync, unlinkSync} from 'node:fs';
import {crc32, deflateSync} from 'node:zlib';

function chunk(type, bytes) {
  const name = Buffer.from(type), size = Buffer.alloc(4), checksum = Buffer.alloc(4);
  size.writeUInt32BE(bytes.length); checksum.writeUInt32BE(crc32(Buffer.concat([name, bytes])));
  return Buffer.concat([size, name, bytes, checksum]);
}
export function png(width, height, format, pixels) {
  assert(Number.isSafeInteger(width) && width > 0 && Number.isSafeInteger(height) && height > 0);
  assert(['Rgba8UnormSrgb', 'Rgba8Unorm', 'Bgra8UnormSrgb', 'Bgra8Unorm'].includes(format), 'unsupported GPU pixel format');
  assert.equal(pixels.length, width * height * 4, 'incomplete GPU pixels');
  const rows = Buffer.alloc(height * (width * 4 + 1));
  for (let y = 0; y < height; y++) {
    const at = y * (width * 4 + 1) + 1;
    pixels.copy(rows, at, y * width * 4, (y + 1) * width * 4);
    if (format.startsWith('Bgra')) for (let x = 0; x < width; x++) {
      const i = at + x * 4; [rows[i], rows[i + 2]] = [rows[i + 2], rows[i]];
    }
  }
  const header = Buffer.alloc(13); header.writeUInt32BE(width); header.writeUInt32BE(height, 4);
  header[8] = 8; header[9] = 6;
  return Buffer.concat([Buffer.from([137,80,78,71,13,10,26,10]), chunk('IHDR',header),
    chunk('IDAT',deflateSync(rows)),chunk('IEND',Buffer.alloc(0))]);
}

// evaluate(expression) returns its awaited JSON value from the already-owned tab.
// A partial export may resume only against the same full pixel hash and receipt.
export async function exportRetainedFrame(evaluate, output, chunkBytes = 32768) {
  assert(Number.isSafeInteger(chunkBytes) && chunkBytes > 0 && chunkBytes <= 32768);
  const receipt = output + '.json', partial = output + '.rgba.part';
  const metadata = await evaluate(`(async () => {
    const r = window.__cyrusFrameCaptureResult;
    if (!r?.ok || !(r.pixels instanceof Uint8Array)) throw Error('no retained GPU pixels');
    const hash = [...new Uint8Array(await crypto.subtle.digest('SHA-256', r.pixels))]
      .map(x => x.toString(16).padStart(2,'0')).join('');
    if (window.__cyrusFrameCaptureResult !== r) throw Error('retained result replaced during export');
    window.__cyrusFrameExport = {result:r, pixels:r.pixels};
    return {id:r.id,boundary:r.boundary,width:r.width,height:r.height,format:r.format,
      bytes:r.pixels.length,sha256:hash};
  })()`);
  assert.equal(metadata.bytes, metadata.width * metadata.height * 4, 'incomplete GPU pixels');
  if (existsSync(receipt)) {
    const prior = JSON.parse(readFileSync(receipt));
    assert.deepEqual(prior.capture, metadata, 'retained capture changed; preserve partial export');
    if (prior.complete) {
      assert.equal(createHash('sha256').update(readFileSync(output)).digest('hex'), prior.png_sha256, 'completed PNG changed');
      return prior;
    }
  } else {
    assert(!existsSync(output) && !existsSync(partial), 'unowned export output exists');
    writeFileSync(receipt + '.part', JSON.stringify({capture:metadata,complete:false},null,2)+'\n');
    renameSync(receipt + '.part', receipt);
  }
  if (!existsSync(partial)) writeFileSync(partial, Buffer.alloc(0), {flag:'wx'});
  let offset = readFileSync(partial).length;
  assert(offset <= metadata.bytes, 'partial export exceeds retained frame');
  while (offset < metadata.bytes) {
    const length = Math.min(chunkBytes, metadata.bytes - offset);
    const data = await evaluate(`(() => {
      const s = window.__cyrusFrameExport;
      if (!s || window.__cyrusFrameCaptureResult !== s.result || s.result.pixels !== s.pixels)
        throw Error('retained result replaced or reset during export');
      return btoa(String.fromCharCode(...s.pixels.subarray(${offset},${offset + length})));
    })()`);
    const bytes = Buffer.from(data, 'base64');
    assert.equal(bytes.length, length, 'truncated retained pixel chunk');
    appendFileSync(partial, bytes); offset += length;
  }
  const pixels = readFileSync(partial);
  assert.equal(createHash('sha256').update(pixels).digest('hex'), metadata.sha256, 'GPU export hash mismatch');
  const image = png(metadata.width, metadata.height, metadata.format, pixels);
  if (existsSync(output)) {
    assert.deepEqual(readFileSync(output), image, 'PNG output differs from verified retained pixels');
  } else {
    writeFileSync(output + '.part', image); renameSync(output + '.part', output);
  }
  const result = {capture:metadata,complete:true,png_sha256:createHash('sha256').update(image).digest('hex')};
  writeFileSync(receipt + '.part', JSON.stringify(result,null,2)+'\n');
  renameSync(receipt + '.part', receipt); unlinkSync(partial);
  return result;
}
