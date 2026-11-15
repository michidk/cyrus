#!/usr/bin/env python3
"""Exercise the production framebuffer oracle through its command-line interface."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from PIL import Image

SCRIPT = Path(__file__).with_name('check-book-framebuffer.py')


class FramebufferTest(unittest.TestCase):
    def test_capture_rejects_unquantized_channels_and_wrong_dither_phase(self):
        # Independently enumerated output for encoded RGB (120,100,80).
        red = ((115,123,115,123),(123,115,123,115),(115,123,115,123),(123,123,123,115))
        green = ((97,101,97,101),(101,97,101,101),(97,101,97,101),(101,101,101,101))
        blue = ((74,82,74,82),(82,74,82,82),(74,82,74,82),(82,82,82,82))
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'capture.png'
            expected = Image.new('RGB', (4,4))
            expected.putdata([(red[y][x],green[y][x],blue[y][x])
                              for y in range(4) for x in range(4)])
            for image, accepted in [(expected, True),
                                     (Image.new('RGB',(4,4),(120,100,80)),False),
                                     (expected.transpose(Image.Transpose.FLIP_TOP_BOTTOM),False)]:
                image.save(path)
                run = subprocess.run([sys.executable,str(SCRIPT),'check',str(path),
                                      '--colour','120','100','80','--rectangle','0','0','4','4'],
                                     capture_output=True,text=True,check=False)
                self.assertEqual(run.returncode == 0,accepted,run.stderr)

    def test_fixture_replaces_only_texture_pixels(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source,target = root/'source.menu',root/'target.menu'
            source.write_text(json.dumps({'models':[1], 'textures':[
                {'width':2,'height':1,'rgba':[0]*8,'indices':[1,2]}]}))
            subprocess.run([sys.executable,str(SCRIPT),'prepare',str(source),str(target),
                            '--colour','120','100','80'],check=True)
            result = json.loads(target.read_text())
            self.assertEqual(result['models'],[1])
            self.assertEqual(result['textures'][0]['indices'],[1,2])
            self.assertEqual(result['textures'][0]['rgba'],[120,100,80,255]*2)


if __name__ == '__main__':
    unittest.main()
