import assert from 'node:assert/strict';

// Compare independently observed source outputs with independently observed mix.
// A prefix before a source tap joined is excluded explicitly, never repaired by
// shifting the waveform or by filling missing generation quanta with silence.
export function compareSourceGeneration({sources, graph}, boundaries = []) {
  assert(!graph.error, 'mixed graph rejected capture');
  assert(graph.blocks.length, 'mixed graph has no quanta');
  const rate = graph.rate;
  assert(Number.isFinite(rate) && rate > 0, 'mixed graph has no sample rate');
  let start = graph.blocks[0].frame;
  for (const source of sources) {
    let previous;
    for (const block of source.blocks) {
      assert(!previous || block.frame === previous.frame + 128 && block.sequence === previous.sequence + 1,
        'discontinuous source graph quantum');
      previous = block;
    }
    if (source.observed) {
      assert(source.blocks.length, 'source has no observed quanta');
      start = Math.max(start, source.blocks[0].frame);
    } else {
      assert(Number.isInteger(source.retiredFrame), 'unobserved source has no acknowledged retirement');
      start = Math.max(start, Math.ceil(source.retiredFrame / 128) * 128);
    }
  }
  for (const boundary of boundaries) if (!boundary.attributed) {
    assert(Number.isInteger(boundary.retiredFrame), 'unattributed source has no acknowledged retirement');
    start = Math.max(start, Math.ceil(boundary.retiredFrame / 128) * 128);
  }
  const selected = graph.blocks.filter(b => b.frame >= start);
  assert(selected.length, 'no complete paired generation interval');
  const maps = sources.map(s => new Map(s.blocks.map(b => [b.frame, b])));
  let next = start, sequence = selected[0].sequence, energy = 0, error = 0;
  for (const block of selected) {
    assert(!block.error && block.frame === next && block.sequence === sequence
      && block.left.length === 128 && block.right.length === 128, 'discontinuous mixed graph quantum');
    const inputs = sources.map((s, i) => {
      const input = maps[i].get(block.frame);
      if (!input) {
        assert(Number.isInteger(s.retiredFrame) && s.retiredFrame <= block.frame,
          'missing source quantum before acknowledged retirement');
      } else assert(!input.error && (input.zero === true
        || input.left.length === 128 && input.right.length === 128), 'invalid source quantum');
      return input;
    });
    for (let i = 0; i < 128; i++) {
      let left = 0, right = 0;
      for (const input of inputs) if (input && !input.zero) {
        left += input.left[i]; right += input.right[i];
      }
      energy += block.left[i] ** 2 + block.right[i] ** 2;
      error += (left - block.left[i]) ** 2 + (right - block.right[i]) ** 2;
    }
    next += 128;sequence++;
  }
  assert(energy > 0.001, 'paired generation interval has no nonzero waveform evidence');
  assert(error / energy < 1e-6, 'independently observed source/mix PCM differs');
  return {start,end:next,frames:next-start,rate,energy,error,ratio:error/energy};
}
