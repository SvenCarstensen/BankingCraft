#!/usr/bin/env python3
"""Generate BankingCraft's original, quiet notification chime (no external asset).
PCM16 mono, 44.1 kHz, 0.8 s; soft attack/release, peak below -20 dBFS.
The OS still controls notification volume, mute and Focus behavior.
"""
import math
from pathlib import Path
import struct
import wave

rate = 44100
samples = []
for i in range(int(rate * .8)):
    t = i / rate
    envelope = min(1., t / .06) * min(1., (.8 - t) / .18) * math.exp(-2.8 * t)
    signal = .06 * envelope * (math.sin(2 * math.pi * 523.25 * t) + .35 * math.sin(2 * math.pi * 659.25 * t))
    samples.append(round(32767 * signal))
path = Path(__file__).resolve().parents[1] / 'macos/bankingcraft-gentle.wav'
with wave.open(str(path), 'wb') as out:
    out.setparams((1, 2, rate, len(samples), 'NONE', 'not compressed'))
    out.writeframes(struct.pack('<' + 'h' * len(samples), *samples))
assert max(abs(v) for v in samples) < 3277
print(f'Generated {path.name}: 0.8 s, quiet PCM chime')
