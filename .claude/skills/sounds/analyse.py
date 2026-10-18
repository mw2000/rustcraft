"""Looks at the sounds in assets/sounds/ for what can't be heard here: levels, peaks, clipping,
loop seams and spectra, printed as a table, and spectrograms drawn into sheets to read.

    python analyse.py out/              # every sound, sheets into out/
    python analyse.py out/ step/stone   # only names starting with these
"""

import argparse
import pathlib

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np
import soundfile as sf

from synth import OUT, loudness

ROWS = 4


def seam(x, sr):
    """How much a loop jumps where its end meets its start: the step between the last sample and
    the first, against the typical step between neighbours, and the level either side."""
    mono = x if x.ndim == 1 else x.mean(axis=1)
    steps = np.abs(np.diff(mono))
    jump = abs(mono[0] - mono[-1]) / (np.percentile(steps, 99) + 1e-12)
    w = sr // 20
    ends = 20 * np.log10(np.sqrt(np.mean(mono[-w:] ** 2)) / np.sqrt(np.mean(mono[:w] ** 2)))
    return jump, ends


def centroid(x, sr):
    mono = x if x.ndim == 1 else x.mean(axis=1)
    power = np.abs(np.fft.rfft(mono)) ** 2
    freqs = np.fft.rfftfreq(len(mono), 1 / sr)
    return np.sum(freqs * power) / np.sum(power)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("out", type=pathlib.Path)
    parser.add_argument("only", nargs="*")
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    files = sorted(OUT.rglob("*.ogg"))
    files = [f for f in files if not args.only or any(str(f.relative_to(OUT)).startswith(o) for o in args.only)]
    print(f"{'sound':28} {'secs':>6} {'peak dB':>8} {'loud dB':>8} {'clipped':>7} {'centroid':>8}  seam")
    groups = {}
    for file in files:
        name = str(file.relative_to(OUT).with_suffix(""))
        x, sr = sf.read(file)
        long = len(x) / sr > 5
        peak = 20 * np.log10(np.max(np.abs(x)) + 1e-12)
        loud = loudness(x, window=len(x) / sr if long else 0.05)
        clipped = int(np.sum(np.abs(x) >= 0.999))
        line = f"{name:28} {len(x) / sr:6.2f} {peak:8.1f} {loud:8.1f} {clipped:7} {centroid(x, sr):8.0f}"
        if name.startswith("ambience/"):
            jump, ends = seam(x, sr)
            line += f"  jump {jump:.2f} of a big step, ends {ends:+.1f} dB"
        print(line)
        groups.setdefault(name.split("/")[0] if "/" in name else "misc", []).append((name, x, sr))
    for group, sounds in groups.items():
        cols = int(np.ceil(len(sounds) / ROWS)) if len(sounds) > ROWS else 1
        rows = min(ROWS, len(sounds)) if cols == 1 else ROWS
        fig, axes = plt.subplots(rows, cols, figsize=(4 * cols + 2, 2.2 * rows), squeeze=False)
        for ax in axes.flat:
            ax.axis("off")
        for ax, (name, x, sr) in zip(axes.T.flat, sounds):
            mono = x if x.ndim == 1 else x.mean(axis=1)
            ax.axis("on")
            # 70 dB down from each one's loudest, about what's heard over a quiet room
            mono = mono / (np.max(np.abs(mono)) + 1e-12)
            ax.specgram(mono, NFFT=1024 if len(mono) > sr * 5 else 256, Fs=sr, noverlap=128 if len(mono) <= sr * 5 else 512, cmap="magma", vmin=-110, vmax=-40)
            ax.set_title(name, fontsize=8)
            ax.set_ylim(0, 12000)
            ax.tick_params(labelsize=6)
        fig.tight_layout()
        fig.savefig(args.out / f"{group.replace('/', '-')}.png", dpi=80)
        plt.close(fig)


if __name__ == "__main__":
    main()
