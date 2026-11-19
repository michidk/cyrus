import assert from 'node:assert/strict';
import test from 'node:test';
import { captureStoppedFrame, stopAtWorldEndpoint, stopAfterWorldProgress } from './capture-cdp.mjs';

test('slow screenshot readback retains the stopped endpoint instead of advancing gameplay', async () => {
  const frame = 1;
  const capture = await captureStoppedFrame({async call(method) {
    if (method === 'Runtime.evaluate') return {result:{value:{frame}}};
    if (method === 'Page.captureScreenshot') { await Promise.resolve(); return {data:'png'}; }
    throw Error(method);
  }}, 'publicEndpoint');
  assert.deepEqual(capture, {data:'png', observation:{frame:1}});
});

test('a changed endpoint during readback is rejected', async () => {
  let frame = 1;
  await assert.rejects(captureStoppedFrame({async call(method) {
    if (method === 'Runtime.evaluate') return {result:{value:{frame}}};
    if (method === 'Page.captureScreenshot') { frame++; return {data:'png'}; }
    throw Error(method);
  }}, 'publicEndpoint'), /endpoint advanced/);
});

test('the disposable page renders its first world update and drops later animation callbacks', () => {
  const callbacks = [];
  globalThis.window = {requestAnimationFrame: callback => callbacks.push(callback)};
  try {
    stopAtWorldEndpoint();
    let frames = 0;
    const update = () => {
      frames++;
      window.__cyrusStartup = {state:'InGame'};
      window.__cyrusCapture = {ready:true,scripts:{globals:{world_loop_frame:1}}};
      window.requestAnimationFrame(update);
    };
    window.requestAnimationFrame(update);
    callbacks.shift()(0);
    assert.equal(frames, 1);
    callbacks.shift()(1);
    assert.equal(frames, 1);
    assert.equal(window.__cyrusEndpointStopped, true);
  } finally { delete globalThis.window; }
});

test('a matched ordinary-update cutoff permits all four updates before stopping', () => {
  const callbacks = [];
  globalThis.window = {
    requestAnimationFrame: callback => callbacks.push(callback),
    __cyrusStartup: {state:'InGame'},
    __cyrusCapture: {ready:true,scripts:{globals:{world_loop_frame:0}}},
  };
  try {
    stopAtWorldEndpoint(4);
    const update = () => {
      window.__cyrusCapture.scripts.globals.world_loop_frame++;
      window.requestAnimationFrame(update);
    };
    window.requestAnimationFrame(update);
    for(let frame=0;frame<5;frame++) callbacks.shift()(frame);
    assert.equal(window.__cyrusCapture.scripts.globals.world_loop_frame,4);
    assert.equal(window.__cyrusEndpointStopped,true);
  } finally { delete globalThis.window; }
});

test('settling starts at an ordinary update even when the game clock already advanced', () => {
  const callbacks = [];
  globalThis.window = {
    requestAnimationFrame: callback => callbacks.push(callback),
    __cyrusStartup: {state:'InGame',clock:{game_seconds:4}},
    __cyrusCapture: {ready:true,world:'1',boundary:{generation:1,update:0}},
  };
  try {
    stopAfterWorldProgress(4, 2);
    let frames = 0;
    const update = () => { frames++; window.requestAnimationFrame(update); };
    window.requestAnimationFrame(update);
    callbacks.shift()(0);
    assert.equal(frames, 1);
    assert.equal(window.__cyrusEndpointStopped, undefined);
    window.__cyrusCapture.boundary.update = 1;
    window.__cyrusStartup.clock.game_seconds = 5;
    callbacks.shift()(1);
    window.__cyrusStartup.clock.game_seconds = 6.9;
    callbacks.shift()(2);
    assert.equal(frames, 3);
    window.__cyrusStartup.clock.game_seconds = 7;
    callbacks.shift()(3);
    assert.equal(frames, 3);
    assert.equal(window.__cyrusEndpointStopped, true);
  } finally { delete globalThis.window; }
});
