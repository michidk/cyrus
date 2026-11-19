// Exercise the shared sky direction calculation in the browser's WebGL compiler.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { connectCdp } from './capture-cdp.mjs';

const source = readFileSync(new URL('../../crates/cyrus-render/src/sky.wesl', import.meta.url), 'utf8');
const calculation = source.match(/    let forward = -view\.world_from_view\[2\]\.xz;([\s\S]*?)    let tilt =/)[1]
  .replace(/\/\/[^\n]*/g, '')
  .replace(/let heading =/g, 'float heading =')
  .replace(/let horizontal =/g, 'vec2 horizontal =')
  .replace(/atan2\(/g, 'atan(');
const cdp = await connectCdp(process.argv[2] ?? 'http://127.0.0.1:9222');
try {
  await cdp.call('Runtime.enable');
  const result = await cdp.call('Runtime.evaluate', {
    expression: `(${function check(calculation) {
      const gl = document.createElement('canvas').getContext('webgl2');
      if (!gl || !gl.getExtension('EXT_color_buffer_float')) throw Error('float WebGL2 target unavailable');
      const compile = (type, text) => {
        const shader = gl.createShader(type);
        gl.shaderSource(shader, text); gl.compileShader(shader);
        if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) throw Error(gl.getShaderInfoLog(shader));
        return shader;
      };
      const program = gl.createProgram();
      gl.attachShader(program, compile(gl.VERTEX_SHADER, '#version 300 es\nvoid main(){gl_Position=vec4(gl_VertexID==1?3.0:-1.0,gl_VertexID==2?3.0:-1.0,0.0,1.0);}'));
      gl.attachShader(program, compile(gl.FRAGMENT_SHADER, '#version 300 es\nprecision highp float;\nuniform vec2 forward;out vec4 color;void main(){'+calculation+'color=vec4(horizontal,0.0,1.0);}'));
      gl.linkProgram(program);
      if (!gl.getProgramParameter(program, gl.LINK_STATUS)) throw Error(gl.getProgramInfoLog(program));
      gl.useProgram(program);
      const texture = gl.createTexture(); gl.bindTexture(gl.TEXTURE_2D, texture);
      gl.texStorage2D(gl.TEXTURE_2D, 1, gl.RGBA32F, 1, 1);
      const framebuffer = gl.createFramebuffer(); gl.bindFramebuffer(gl.FRAMEBUFFER, framebuffer);
      gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, texture, 0);
      if (gl.checkFramebufferStatus(gl.FRAMEBUFFER) !== gl.FRAMEBUFFER_COMPLETE) throw Error('incomplete target');
      gl.viewport(0, 0, 1, 1);
      return [[-0, 1], [0, 1], [-0, -1], [1, 0], [-1, 0], [0.3, 0.7], [-0.3, -0.7]].map(input => {
        gl.uniform2f(gl.getUniformLocation(program, 'forward'), ...input);
        gl.drawArrays(gl.TRIANGLES, 0, 3);
        const output = new Float32Array(4);
        gl.readPixels(0, 0, 1, 1, gl.RGBA, gl.FLOAT, output);
        if (gl.getError() !== gl.NO_ERROR) throw Error('WebGL error');
        return { input, output: Array.from(output).slice(0, 2) };
      });
    }.toString()})(${JSON.stringify(calculation)})`,
    returnByValue: true,
  });
  assert(!result.exceptionDetails, JSON.stringify(result.exceptionDetails));
  for (const { input, output } of result.result.value) {
    const length = Math.hypot(...input);
    input.forEach((value, index) => assert(Math.abs(output[index] - value / length) < 1e-5,
      `sky direction ${input}: ${output}`));
  }
  console.log('WebGL sky directions: 7 passed');
} finally {
  await cdp.close();
}
