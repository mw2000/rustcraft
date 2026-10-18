"""Levels recordings to the sounds' targets, once, as they're added: the ambience loops to
`LOOP_DB` over their length and the owl to a creature call's -21, written as `synth.py` writes
loops (stereo kept, Vorbis at its loops' quality). The files are cut and looped already (see
assets/CREDITS.md); run it on the originals, not on files it has written, which would only lose
a little more to encoding.

    python recordings.py SOURCE_DIR       # SOURCE_DIR/<name>.ogg as named in RECORDINGS
"""

import argparse
import pathlib

import numpy as np
import soundfile as sf

from synth import LOOP_DB, OUT, level

# The source files' names, and where each goes under assets/sounds with its loudness, and
# whether it's a loop, levelled over its length.
RECORDINGS = {
    "waves": ("ambience/sea", LOOP_DB, True),
    "forest": ("ambience/birds", LOOP_DB, True),
    "crickets": ("ambience/crickets", LOOP_DB, True),
    "underwater": ("ambience/underwater", LOOP_DB, True),
    "desert": ("ambience/desert", LOOP_DB, True),
    "leaves": ("ambience/leaves", LOOP_DB, True),
    "owl": ("owl", -21, False),
}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("source", type=pathlib.Path)
    args = parser.parse_args()
    for name, (path, target, loop) in RECORDINGS.items():
        x, sr = sf.read(args.source / f"{name}.ogg")
        x = level(x, target, window=len(x) / sr) if loop else level(x, target)
        x = np.ascontiguousarray(x.astype(np.float32))
        file = OUT / f"{path}.ogg"
        # in blocks, as libsndfile's Vorbis encoder crashes given a long stereo file at once
        quality = 0.6 if loop else 0.4
        with sf.SoundFile(file, "w", sr, x.shape[1] if x.ndim > 1 else 1, format="OGG", subtype="VORBIS", compression_level=quality) as out:
            for start in range(0, len(x), sr):
                out.write(x[start : start + sr])
        print(f"{path}: {len(x) / sr:.2f}s")


if __name__ == "__main__":
    main()
