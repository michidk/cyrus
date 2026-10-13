import { test } from 'node:test';
import assert from 'node:assert/strict';
import { verifyNative } from './capture-provenance.mjs';

const record = { version: 1, artifacts: { 'target/debug/cyrus': 'expected-digest' } };
const container = { Id: 'container-id', State: { Running: true },
  NetworkSettings: { Ports: { '7117/tcp': [{ HostIp: '127.0.0.1', HostPort: '7118' }] } } };

test('native provenance identifies the executable serving the selected endpoint', () => {
  assert.equal(verifyNative(record, container, '127.0.0.1:7118', 'expected-digest').container_id, 'container-id');
});
test('a matching host binary cannot vouch for a different running executable', () => {
  assert.throws(() => verifyNative(record, container, '127.0.0.1:7118', 'old-digest'), /running native executable/);
});
test('a different control endpoint cannot borrow a container build identity', () => {
  assert.throws(() => verifyNative(record, container, '127.0.0.1:7117', 'expected-digest'), /control endpoint/);
  assert.throws(() => verifyNative(record, container, 'example.org:7118', 'expected-digest'), /local Docker/);
});
test('a stopped container cannot supply capture evidence', () => {
  assert.throws(() => verifyNative(record, { ...container, State: { Running: false } }, '127.0.0.1:7118', 'expected-digest'), /not running/);
});
