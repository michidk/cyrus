// Capture checks the live executable, not merely the file supplied to Docker.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';

function run(program, args) {
  const result = spawnSync(program, args, { encoding: 'utf8', maxBuffer: 16 * 1024 * 1024 });
  assert.equal(result.status, 0, `${program}: ${result.error || result.stderr}`);
  return result.stdout.trim();
}

export function verifyNative(record, container, address, executableHash) {
  assert.equal(record.version, 1, 'native provenance version');
  assert(container.State?.Running, 'native capture container is not running');
  const [host, port] = address.split(':');
  assert.equal(host, '127.0.0.1', 'native capture requires the local Docker control endpoint at 127.0.0.1');
  const bindings = container.NetworkSettings?.Ports?.['7117/tcp'] || [];
  assert(bindings.some(binding => ['127.0.0.1', '0.0.0.0'].includes(binding.HostIp) && binding.HostPort === port),
    'native control endpoint does not belong to the capture container');
  assert.equal(executableHash, record.artifacts['target/debug/cyrus'], 'running native executable differs from recorded build');
  return { build: record, container_id: container.Id, image_id: container.Image, executable_sha256: executableHash };
}

export function nativeProvenance() {
  const filename = 'target/debug/cyrus.provenance.json';
  run('python3', ['dev/scripts/build-provenance.py', 'verify', filename]);
  const record = JSON.parse(readFileSync(filename, 'utf8'));
  const name = process.env.CYRUS_CAPTURE_NATIVE_CONTAINER || 'cyrus-dev';
  const [container] = JSON.parse(run('docker', ['inspect', name]));
  const hash = run('docker', ['exec', container.Id, 'sha256sum', '/proc/1/exe']).split(/\s+/)[0];
  return verifyNative(record, container, process.env.CYRUS_CONTROL_ADDR || '127.0.0.1:7117', hash);
}
