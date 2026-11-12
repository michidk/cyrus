#!/usr/bin/env python3
"""Measure an executed recording boundary transfer using the same cue waveforms.

The source is played at explicit unity through the target sink. Each cue must
independently admit continuity on both sides. Ratios describe this executed
transfer, not an assumed generic resampler or a production volume adjustment.
"""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path


def load_module(filename):
    spec=importlib.util.spec_from_file_location(filename,Path(__file__).with_name(filename+'.py'))
    value=importlib.util.module_from_spec(spec);spec.loader.exec_module(value);return value


def compare(before, after):
    if not before['success'] or not after['success']:
        raise ValueError('both transfer boundaries must admit cue continuity')
    if len(before['references']) != len(after['references']):
        raise ValueError('transfer reference sets differ')
    ratios=[]
    for source,target in zip(before['references'],after['references']):
        if source['sha256'] != target['sha256'] or not source['median_gain'] > 0:
            raise ValueError('transfer references or source gain differ')
        ratios.append(dict(reference_sha256=source['sha256'],
            source_gain=source['median_gain'],target_gain=target['median_gain'],
            gain_ratio=target['median_gain']/source['median_gain']))
    return ratios


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('capture',type=Path)
    parser.add_argument('source',type=Path)
    parser.add_argument('output',type=Path)
    parser.add_argument('--reference',action='append',required=True,type=Path)
    args=parser.parse_args()
    if args.output.exists():parser.error('output exists; preserve evidence')
    capture=json.loads((args.capture/'result.json').read_text())
    digest=hashlib.sha256(args.source.read_bytes()).hexdigest()
    if not capture['success'] or capture['playback_volume']!=65536 or capture['reference_sha256']!=digest:
        parser.error('transfer requires admitted explicit-unity playback of the same source hash')
    continuity=load_module('book-audio-continuity')
    reports=[]
    for path in [args.source,args.capture/'monitor.wav']:
        cues=[]
        for reference in args.reference:
            cue=continuity.normalize(continuity.load(path),continuity.load(reference))
            cue['sha256']=hashlib.sha256(reference.read_bytes()).hexdigest();cues.append(cue)
        reports.append(dict(success=all(c['success'] for c in cues),references=cues))
    result=dict(success=True,source_sha256=digest,source=reports[0],target=reports[1],
                transfer=compare(*reports))
    args.output.write_text(json.dumps(result,indent=2)+'\n')

if __name__=='__main__':main()
