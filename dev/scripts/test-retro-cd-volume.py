#!/usr/bin/env python3
"""Exercise the reference emulator's CD control operation before and after its patch.

The ChannelControl fixture is from DOSBox-X 59915c19, GPL-2.0-or-later:
https://github.com/joncampbell123/dosbox-x/blob/59915c19e52afc30f152d210bc5b2e06e7c320c6/src/dos/cdrom_image.cpp
No game or game executable is used.
"""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
CONTROL = '''void CDROM_Interface_Image::ChannelControl(TCtrl ctrl)
{
\t// Guard: Bail if our mixer channel hasn't been allocated
\tif (!player.channel)
\t\treturn;

\tplayer.ctrlUsed = (ctrl.out[0]!=0 || ctrl.out[1]!=1 || ctrl.vol[0]<0xfe || ctrl.vol[1]<0xfe);
\tplayer.ctrlData = ctrl;

\t// Adjust the volume of our mixer channel as defined by the application
\tplayer.channel->SetScale(static_cast<float>(ctrl.vol[0]),  // left vol
\t                         static_cast<float>(ctrl.vol[1])); // right vol
}

bool CDROM_Interface_Image::ReadSectors(PhysPt buffer, bool raw, unsigned long sector, unsigned long num)
'''
PREFIX = '''#include <algorithm>
#include <iostream>
struct TCtrl { unsigned char out[2], vol[2]; };
struct Mixer {
    float scale[2] = {1, 1};
    void SetScale(float l, float r) {
        scale[0] = std::clamp(l, 0.f, 4.f);
        scale[1] = std::clamp(r, 0.f, 4.f);
    }
};
struct CDROM_Interface_Image {
    struct Player { Mixer* channel; bool ctrlUsed; TCtrl ctrlData; } player;
    void ChannelControl(TCtrl ctrl);
};
'''
SUFFIX = '''
int main() {
    Mixer mixer;
    CDROM_Interface_Image cd{{&mixer, false, {}}};
    for (int volume : {0, 64, 128, 200, 255}) {
        cd.ChannelControl({{0, 1}, {(unsigned char)volume, (unsigned char)volume}});
        // The existing decoder scales signed PCM by ctrlData.vol / 255 before
        // submitting it to MixerChannel, whose scale is then applied.
        int sample = cd.player.ctrlUsed ? (int)(1000 * volume / 255.0) : 1000;
        std::cout << (int)(sample * mixer.scale[0]) << " ";
    }
}
'''


def output(directory, source):
    path = directory / 'probe.cpp'
    path.write_text(PREFIX + source.split('\nbool CDROM_Interface_Image::ReadSectors')[0] + SUFFIX)
    binary = directory / 'probe'
    subprocess.run(['c++', '-std=c++17', '-Wall', '-Wextra', '-Werror', str(path), '-o', str(binary)], check=True)
    return subprocess.check_output([str(binary)], text=True).split()


with tempfile.TemporaryDirectory() as temporary:
    directory = Path(temporary)
    source = directory / 'src/dos/cdrom_image.cpp'
    source.parent.mkdir(parents=True)
    source.write_text(CONTROL)
    before = output(directory, CONTROL)
    assert before == ['0', '1000', '2004', '3136', '4000'], before
    subprocess.run(['patch', '--batch', '--fuzz=0', '-p1', '-i', str(ROOT / 'dev/docker/retro/cd-volume.patch')], cwd=directory, check=True)
    after = output(directory, source.read_text())
    assert after == ['0', '250', '501', '784', '1000'], after
    repeated = subprocess.run(['patch', '--batch', '--forward', '--fuzz=0', '-p1', '-i', str(ROOT / 'dev/docker/retro/cd-volume.patch')], cwd=directory, capture_output=True)
    assert repeated.returncode != 0, 'an already patched or changed source must not be silently patched again'
    print(f'CD control: failed-before {before}; single-volume {after}; repeated patch rejected')
