// Exercise the production service-worker fetch handler with synthetic original bytes.
import assert from 'node:assert/strict';
import vm from 'node:vm';
import { readFileSync } from 'node:fs';

const handlers = {}, stored = new Map();
let calls = 0;
vm.runInNewContext(readFileSync(new URL('../web/sw.js', import.meta.url), 'utf8'), {
  self: {
    location: { href: 'https://game.test/sw.js' },
    addEventListener: (name, handler) => { handlers[name] = handler; },
  },
  URL, Request, Response, Headers, console,
  fetch: async request => {
    calls++;
    const range = new URL(request.url).searchParams.get('range');
    return new Response('abcd', {
      status: 206,
      headers: { 'Content-Range': `bytes ${range === '8-11' ? '0-3' : range}/12` },
    });
  },
  caches: { open: async () => ({
    match: async key => stored.get(key.url)?.clone(),
    put: async (key, value) => { stored.set(key.url, value); },
  }) },
});

async function read(query, range, digest = 'a'.repeat(64)) {
  let response;
  const waits = [];
  handlers.fetch({
    request: new Request(`https://game.test/original/FILE?v=${digest}${query}`, {
      headers: { Range: range },
    }),
    respondWith: result => { response = result; },
    waitUntil: result => { waits.push(result); },
  });
  if (!response) return null;
  const result = await response;
  await Promise.all(waits);
  return result;
}

assert.equal(await read('', 'bytes=4-7'), null, 'ordinary Range requests bypass the cache');
assert.equal(await read('&range=4-7', 'bytes=4-7', 'mutable'), null, 'mutable sources bypass');
assert.equal(await read('&range=4-7', 'bytes=8-11'), null, 'mismatched headers bypass');
const first = await read('&range=4-7', 'bytes=4-7');
assert.ok(first, 'digest-addressed explicit ranges should use the cache');
assert.equal(await first.text(), 'abcd');
const second = await read('&range=4-7', 'bytes=4-7');
assert.equal(await second.text(), 'abcd');
assert.equal(second.headers.get('Content-Range'), 'bytes 4-7/12');
assert.equal(calls, 1, 'repeat reads reuse immutable bytes');
await read('&range=4-7', 'bytes=4-7', 'b'.repeat(64));
assert.equal(calls, 2, 'a changed source digest must fetch fresh bytes');
await read('&range=0-3', 'bytes=0-3');
assert.equal(calls, 3, 'different ranges have independent cache entries');
await read('&range=8-11', 'bytes=8-11');
await read('&range=8-11', 'bytes=8-11');
assert.equal(calls, 5, 'a response with the wrong Content-Range must never be cached');
console.log('original range cache: reuse, source/range isolation, response validation and bypass passed');
