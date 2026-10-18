"""Synthesises every sound rustcraft plays into assets/sounds/, from filtered noise, struck modes,
bubbles, chirps and plucked strings, so each is ours to ship (INV-8).

    python synth.py                 # everything
    python synth.py step break      # only names starting with these
    python synth.py --wav out/      # also write WAVs there, for listening

Each sound is seeded, so a run makes the same files. Levels are set per kind by a short-term
loudness (`loudness`), so footsteps match each other and sit under breaking, and nothing peaks
over -1 dBFS. Ambience loops are made periodic (noise shaped in the frequency domain, LFOs with
whole cycles in the loop, events and reverb wrapped round), so they loop without a seam.
"""

import argparse
import pathlib
import time

import numpy as np
import soundfile as sf
from scipy import signal

SR = 44100
OUT = pathlib.Path(__file__).resolve().parents[3] / "assets" / "sounds"

# How many takes of each one-shot sound, which `src/client/sound/` picks between at random.
# Keep in step with `VARIATIONS` there.
VARIATIONS = 4

# Footstep and breaking surfaces, as `Surface` in `src/client/sound/surface.rs` names them.
SURFACES = ["grass", "dirt", "sand", "gravel", "stone", "wood", "snow", "leaves", "ice"]


# --- building blocks ---------------------------------------------------------------------------


def samples(seconds):
    return int(round(seconds * SR))


def times(n):
    return np.arange(n) / SR


def sos(kind, freq, order=2):
    return signal.butter(order, freq, kind, fs=SR, output="sos")


def lowpass(x, freq, order=2):
    return signal.sosfilt(sos("lowpass", freq, order), x)


def highpass(x, freq, order=2):
    return signal.sosfilt(sos("highpass", freq, order), x)


def bandpass(x, lo, hi, order=2):
    return signal.sosfilt(sos("bandpass", [lo, hi], order), x)


def resonator(x, freq, q):
    """A two-pole resonance at `freq` with quality `q`, for whistles and bodies."""
    b, a = signal.iirpeak(freq, q, fs=SR)
    return signal.lfilter(b, a, x)


def around(f, x):
    """`f`, a filter, run round a loop: over it twice, keeping the second, so it starts settled
    where it ends."""
    return f(np.concatenate([x, x]))[len(x) :]


def coloured(n, rng, slope, lo=20.0, hi=None):
    """Noise whose power goes as frequency to the `slope` (0 white, -1 pink, -2 brown), kept
    between `lo` and `hi`. Made in the frequency domain, so it's periodic over its length."""
    spectrum = np.fft.rfft(rng.standard_normal(n))
    freqs = np.fft.rfftfreq(n, 1 / SR)
    shape = np.zeros_like(freqs)
    inside = freqs >= lo
    if hi is not None:
        inside &= freqs <= hi
    shape[inside] = freqs[inside] ** (slope / 2)
    x = np.fft.irfft(spectrum * shape, n)
    return x / (np.std(x) + 1e-12)


def periodic_band(n, rng, lo, hi, slope=0.0):
    """Band-limited noise with soft edges, periodic over `n`: the loops' filters."""
    spectrum = np.fft.rfft(rng.standard_normal(n))
    freqs = np.fft.rfftfreq(n, 1 / SR)
    centre, width = np.sqrt(lo * hi), np.log2(hi / lo) / 2
    octaves = np.log2(np.maximum(freqs, 1.0) / centre)
    shape = np.exp(-0.5 * (octaves / max(width, 0.05)) ** 4) * np.maximum(freqs, 1.0) ** (slope / 2)
    x = np.fft.irfft(spectrum * shape, n)
    return x / (np.std(x) + 1e-12)


def lfo(n, rng, cycles=(1, 2, 3, 5), smooth=1.0):
    """A slow wobble between 0 and 1 with whole cycles over `n` samples, so it loops."""
    phase = 2 * np.pi * np.arange(n) / n
    wobble = sum(
        rng.uniform(0.5, 1.0) / (k**smooth) * np.sin(k * phase + rng.uniform(0, 2 * np.pi))
        for k in cycles
    )
    wobble -= wobble.min()
    return wobble / (wobble.max() + 1e-12)


def envelope(n, attack, decay, hold=0.0):
    """Rises over `attack` seconds, holds, then falls away exponentially with time constant
    `decay`."""
    t = times(n)
    rise = np.clip(t / max(attack, 1e-4), 0, 1) ** 2
    fall = np.exp(-np.maximum(t - attack - hold, 0) / decay)
    return rise * fall


def place(buffer, sound, at, wrap=False):
    """Adds `sound` into `buffer` from sample `at`, wrapping round the end for loops."""
    if wrap:
        idx = (at + np.arange(len(sound))) % len(buffer)
        np.add.at(buffer, idx, sound)
    else:
        end = min(len(buffer), at + len(sound))
        if end > at:
            buffer[at:end] += sound[: end - at]


def modes(n, rng, freqs, decays, amps, spread=0.03):
    """Struck modes: decaying sines, each detuned a little take to take."""
    t = times(n)
    out = np.zeros(n)
    for f, d, a in zip(freqs, decays, amps):
        f *= 1 + rng.uniform(-spread, spread)
        out += a * np.sin(2 * np.pi * f * t + rng.uniform(0, 2 * np.pi)) * np.exp(-t / d)
    return out


def grains(n, rng, count, lo, hi, length=(0.001, 0.004), spread=None, density=None):
    """Tiny bursts of band-passed noise, the crunch of gravel or snow: `count` of them, at times
    drawn from `density` (a weight per sample) or spread evenly over `spread` seconds."""
    out = np.zeros(n)
    if density is None:
        density = np.ones(n) if spread is None else (times(n) < spread).astype(float)
    p = density / density.sum()
    # each as loud as the density where it falls, so a crunch dies away rather than stopping
    loudness_at = np.sqrt(density / density.max())
    for at in rng.choice(n, size=count, p=p):
        g = samples(rng.uniform(*length))
        burst = rng.standard_normal(g) * np.hanning(g)
        centre = np.exp(rng.uniform(np.log(lo), np.log(hi)))
        burst = bandpass(burst, centre / 1.5, min(centre * 1.5, SR / 2 - 100), order=1)
        place(out, burst * rng.uniform(0.3, 1.0) * loudness_at[at], at)
    return out


def thump(n, rng, freq, decay, drop=0.5):
    """A low body thud: a sine falling in pitch, as a foot or a fall lands."""
    t = times(n)
    f = freq * (1 + rng.uniform(-0.08, 0.08)) * (drop + (1 - drop) * np.exp(-t / 0.03))
    return np.sin(2 * np.pi * np.cumsum(f) / SR) * np.exp(-t / decay)


def bubble(rng, f0=None, tau=None):
    """An air bubble: a sine whose pitch rises as it rings down."""
    f0 = f0 or np.exp(rng.uniform(np.log(300), np.log(1400)))
    tau = tau or rng.uniform(0.008, 0.03)
    n = samples(tau * 6)
    t = times(n)
    f = f0 * (1 + rng.uniform(4, 12) * t)
    return np.sin(2 * np.pi * np.cumsum(f) / SR) * np.exp(-t / tau) * envelope(n, 0.001, 1e9)


def chirp(n_seconds, f_start, f_end, curve=1.0):
    n = samples(n_seconds)
    s = np.linspace(0, 1, n) ** curve
    f = f_start + (f_end - f_start) * s
    return np.sin(2 * np.pi * np.cumsum(f) / SR)


def reverb_ir(seconds, rng, bright=0.3, early=()):
    """A room's impulse response: noise dying away, its highs faster than its lows, with
    `early` reflections as (delay seconds, gain)."""
    n = samples(seconds)
    t = times(n)
    noise = rng.standard_normal(n)
    dark = lowpass(noise, 1200) * np.exp(-t / (seconds / 5))
    light = highpass(noise, 1200) * np.exp(-t / (seconds / 14)) * bright
    ir = dark + light
    for delay, gain in early:
        ir[samples(delay)] += gain * 20
    ir[0] = 0
    return ir / np.sqrt(np.sum(ir**2))


def convolve(x, ir, wrap=False):
    """`x` heard in the room `ir`: wrapped round for loops, else with its tail."""
    if wrap:
        n = len(x)
        h = np.zeros(n)
        h[: min(n, len(ir))] = ir[:n]
        return np.fft.irfft(np.fft.rfft(x) * np.fft.rfft(h), n)
    return signal.fftconvolve(x, ir)


def fade_out(x, seconds):
    n = min(len(x), samples(seconds))
    x = x.copy()
    curve = np.cos(np.linspace(0, np.pi / 2, n)) ** 2
    x[-n:] *= curve if x.ndim == 1 else curve[:, None]
    return x


def trim(x, floor_db=-60):
    """Cuts the silence off the end, with a short fade so it doesn't click."""
    loud = np.nonzero(np.abs(x) > np.max(np.abs(x)) * 10 ** (floor_db / 20))[0]
    end = loud[-1] + 1 if len(loud) else len(x)
    return fade_out(x[: end + samples(0.005)], 0.01)


# --- levels --------------------------------------------------------------------------------------


def weighted(x):
    """Roughly the K-weighting loudness meters use: lows count for less, highs a little more."""
    y = highpass(x, 80) if x.ndim == 1 else np.stack([highpass(c, 80) for c in x.T], 1)
    return y + 0.6 * (highpass(y, 2000, 1) if y.ndim == 1 else np.stack([highpass(c, 2000, 1) for c in y.T], 1))


def loudness(x, window=0.05):
    """The loudest stretch of `window` seconds, in dB of weighted RMS: how loud a short sound
    sounds. Loops and music use a long window, nearly their mean."""
    y = weighted(x)
    power = y**2 if y.ndim == 1 else np.mean(y**2, axis=1)
    w = max(1, min(len(power), samples(window)))
    total = np.concatenate([[0.0], np.cumsum(power)])
    mean = (total[w:] - total[:-w]) / w
    return 10 * np.log10(np.max(mean) + 1e-20)


def level(x, target_db, window=0.05, peak_db=-1.0):
    """Scales `x` to sound `target_db` loud, then pulls in any peaks over `peak_db` with a gentle
    curve rather than clipping them."""
    x = x * 10 ** ((target_db - loudness(x, window)) / 20)
    ceiling = 10 ** (peak_db / 20)
    over = np.abs(x) > ceiling * 0.7
    if over.any():
        knee = ceiling * 0.7
        x = np.where(over, np.sign(x) * (knee + (ceiling - knee) * np.tanh((np.abs(x) - knee) / (ceiling - knee))), x)
    return x


# --- footsteps -----------------------------------------------------------------------------------

STEP_DB = -21


def step(surface, rng):
    n = samples(0.32)
    t = times(n)
    body = thump(n, rng, rng.uniform(70, 95), 0.035) * 0.5
    if surface == "grass":
        brush = bandpass(coloured(n, rng, -0.7), 500, 5500) * envelope(n, 0.008, 0.06)
        crackle = grains(n, rng, 45, 1500, 7000, density=np.exp(-t / 0.05))
        x = brush * 0.5 + crackle * 0.9 + body * 0.6
    elif surface == "dirt":
        mud = bandpass(coloured(n, rng, -1.2), 150, 1600) * envelope(n, 0.006, 0.05)
        crumbs = grains(n, rng, 25, 400, 2500, length=(0.002, 0.006), density=np.exp(-t / 0.04))
        x = mud * 0.8 + crumbs * 0.6 + body
    elif surface == "sand":
        hiss = bandpass(coloured(n, rng, -0.3), 900, 7000) * envelope(n, 0.02, 0.07)
        sift = grains(n, rng, 160, 1500, 8000, length=(0.0005, 0.002), density=envelope(n, 0.015, 0.06))
        x = hiss * 0.45 + sift * 0.7 + body * 0.4
    elif surface == "gravel":
        crunch = grains(n, rng, 170, 700, 6000, length=(0.001, 0.005), density=envelope(n, 0.004, 0.06))
        x = crunch + bandpass(coloured(n, rng, -1), 300, 2500) * envelope(n, 0.004, 0.04) * 0.4 + body * 0.7
    elif surface == "stone":
        click = highpass(rng.standard_normal(n), 1500) * envelope(n, 0.0003, 0.002)
        ring = modes(n, rng, [190, 430, 980, 2150, 3900], [0.03, 0.022, 0.012, 0.008, 0.005], [0.6, 0.5, 0.35, 0.25, 0.15], 0.08)
        grit = grains(n, rng, 12, 2000, 8000, density=np.exp(-t / 0.02))
        x = click * 0.6 + ring * 0.7 + grit * 0.3 + body * 0.6
    elif surface == "wood":
        click = bandpass(rng.standard_normal(n), 1500, 6000) * envelope(n, 0.0003, 0.003)
        knock = modes(n, rng, [125, 260, 545, 1080], [0.07, 0.05, 0.03, 0.015], [0.8, 0.6, 0.35, 0.15], 0.06)
        x = click * 0.5 + knock + body * 0.5
    elif surface == "snow":
        squeak = grains(n, rng, 220, 400, 3000, length=(0.002, 0.007), density=envelope(n, 0.03, 0.06))
        soft = lowpass(coloured(n, rng, -1), 900) * envelope(n, 0.02, 0.06)
        x = squeak * 0.8 + soft * 0.4 + body * 0.4
    elif surface == "leaves":
        rustle = highpass(coloured(n, rng, -0.5), 1800) * envelope(n, 0.01, 0.08)
        rustle *= 0.6 + 0.4 * (rng.random(n // 300 + 1).repeat(300)[:n])
        x = rustle * 0.6 + grains(n, rng, 50, 2500, 9000, density=np.exp(-t / 0.07)) * 0.7 + body * 0.3
    elif surface == "ice":
        click = highpass(rng.standard_normal(n), 2500) * envelope(n, 0.0002, 0.0015)
        glass = modes(n, rng, [1450, 3100, 5200, 7300], [0.03, 0.02, 0.012, 0.008], [0.4, 0.35, 0.25, 0.15], 0.1)
        x = click * 0.6 + glass * 0.6 + body * 0.8
    else:
        raise ValueError(surface)
    return level(trim(x), STEP_DB)


# --- breaking ------------------------------------------------------------------------------------

BREAK_DB = -16


def break_(surface, rng):
    n = samples(0.7)
    t = times(n)
    body = thump(n, rng, rng.uniform(55, 80), 0.06, drop=0.4)
    falling = np.exp(-t / 0.12)
    if surface in ("stone", "ice"):
        crack = bandpass(rng.standard_normal(n), 400, 5000) * envelope(n, 0.0005, 0.012)
        rubble = grains(n, rng, 260, 500, 4500, length=(0.002, 0.008), density=falling)
        x = crack + rubble * 0.9 + body * 0.8
        if surface == "stone":
            x += modes(n, rng, [240, 610, 1350], [0.05, 0.03, 0.015], [0.4, 0.3, 0.2], 0.1)
        else:
            shards = np.zeros(n)
            for _ in range(28):
                ping = modes(samples(0.25), rng, [np.exp(rng.uniform(np.log(2000), np.log(8000)))], [rng.uniform(0.02, 0.09)], [1.0])
                place(shards, ping * rng.uniform(0.2, 0.8), int(rng.exponential(0.07) * SR))
            x = x * 0.6 + shards * 0.7
    elif surface == "wood":
        snap = bandpass(rng.standard_normal(n), 800, 7000) * envelope(n, 0.0005, 0.008)
        splinters = grains(n, rng, 70, 1500, 7000, length=(0.001, 0.004), density=np.exp(-t / 0.08))
        knock = modes(n, rng, [95, 205, 440, 880], [0.12, 0.08, 0.05, 0.02], [0.8, 0.6, 0.35, 0.15], 0.08)
        x = snap + splinters * 0.7 + knock + body * 0.5
    elif surface in ("grass", "dirt", "snow"):
        bright = {"grass": 4500, "dirt": 2500, "snow": 2200}[surface]
        tear = bandpass(coloured(n, rng, -1), 180, bright) * (envelope(n, 0.004, 0.05) + 0.5 * np.roll(envelope(n, 0.004, 0.07), samples(0.06)))
        crumbs = grains(n, rng, 140, 300, bright, length=(0.002, 0.007), density=falling)
        x = tear * 0.8 + crumbs * 0.8 + body
        if surface == "grass":
            x += highpass(coloured(n, rng, -0.5), 2500) * envelope(n, 0.01, 0.1) * 0.3
    elif surface == "sand":
        pour = bandpass(coloured(n, rng, -0.5), 700, 6500) * envelope(n, 0.006, 0.16)
        x = pour * 0.6 + grains(n, rng, 400, 1200, 8000, length=(0.0005, 0.002), density=envelope(n, 0.01, 0.15)) * 0.8 + body * 0.6
    elif surface == "gravel":
        x = grains(n, rng, 420, 600, 6000, length=(0.001, 0.006), density=envelope(n, 0.004, 0.12)) + body * 0.8
    elif surface == "leaves":
        rustle = highpass(coloured(n, rng, -0.6), 1500) * envelope(n, 0.01, 0.14)
        x = rustle * 0.6 + grains(n, rng, 160, 2000, 9000, density=envelope(n, 0.005, 0.12)) * 0.8 + body * 0.2
    else:
        raise ValueError(surface)
    return level(trim(x), BREAK_DB)


# --- the player and the things they touch -------------------------------------------------------


def swim(rng):
    n = samples(0.8)
    slosh = bandpass(coloured(n, rng, -1), 200, 1800) * envelope(n, 0.12, 0.18)
    wash = bandpass(coloured(n, rng, -0.5), 1500, 6000) * envelope(n, 0.15, 0.12) * 0.3
    bubbles = np.zeros(n)
    for _ in range(rng.integers(5, 10)):
        place(bubbles, bubble(rng) * rng.uniform(0.1, 0.4), samples(rng.uniform(0.05, 0.45)))
    return level(trim(slosh + wash + bubbles), -24)


def splash(rng):
    n = samples(1.2)
    t = times(n)
    hit = bandpass(rng.standard_normal(n), 300, 6000) * envelope(n, 0.002, 0.08)
    spray = bandpass(coloured(n, rng, -0.4), 1200, 9000) * envelope(n, 0.01, 0.25) * 0.5
    bubbles = np.zeros(n)
    for _ in range(40):
        place(bubbles, bubble(rng) * rng.uniform(0.1, 0.5), int(rng.exponential(0.18) * SR))
    x = hit + spray + bubbles * np.exp(-t / 0.6) * 0.8 + thump(n, rng, 70, 0.08) * 0.4
    return level(trim(x), -17)


def pickup(rng):
    n = samples(0.12)
    f0 = rng.uniform(480, 640)
    pop = chirp(0.12, f0, f0 * 2.4, 0.6)[:n] * envelope(n, 0.002, 0.03)
    click = highpass(rng.standard_normal(n), 3000) * envelope(n, 0.0002, 0.001) * 0.3
    return level(trim(pop + click), -22)


def eat(rng):
    n = samples(0.28)
    t = times(n)
    bite = np.zeros(n)
    for at in (0.0, rng.uniform(0.05, 0.08)):
        crunch = grains(samples(0.12), rng, 60, 900, 5000, length=(0.001, 0.005), density=np.exp(-times(samples(0.12)) / 0.03))
        place(bite, crunch * rng.uniform(0.6, 1.0), samples(at))
    mouth = lowpass(coloured(n, rng, -1.5), 400) * envelope(n, 0.01, 0.05) * 0.5
    return level(trim(bite + mouth * np.exp(-t / 0.1)), -22)


def hurt(rng):
    n = samples(0.4)
    knock = thump(n, rng, rng.uniform(100, 130), 0.09, drop=0.45)
    smack = bandpass(rng.standard_normal(n), 250, 2500) * envelope(n, 0.001, 0.02)
    breath = bandpass(coloured(n, rng, -1), 350, 900) * envelope(n, 0.01, 0.06) * 0.3
    return level(trim(knock + smack * 0.6 + breath), -16)


def death(rng):
    n = samples(1.4)
    t = times(n)
    knock = thump(n, rng, 85, 0.18, drop=0.4)
    # a sigh of air sinking from 900 Hz to 200: a band swept by mixing two
    low, high = bandpass(coloured(n, rng, -1), 150, 450), bandpass(coloured(n, rng, -1), 500, 1300)
    sweep = np.clip(t / 0.9, 0, 1)
    sigh = (high * (1 - sweep) + low * sweep) * envelope(n, 0.08, 0.35) * 0.5
    return level(trim(knock + sigh), -16)


def creak(n, rng, rate_from, rate_to):
    """A hinge: stick-slip pulses, their rate swept and jittered, ringing a wooden body."""
    out = np.zeros(n)
    at = 0.0
    seconds = n / SR
    while at < seconds:
        rate = rate_from + (rate_to - rate_from) * at / seconds
        out[min(n - 1, samples(at))] += rng.uniform(0.5, 1.0)
        at += 1 / (rate * rng.uniform(0.85, 1.15))
    body = sum(resonator(out, f, q) * a for f, q, a in ((330, 12, 1.0), (780, 10, 0.7), (1450, 8, 0.4), (2600, 6, 0.2)))
    return lowpass(body, 3500) * envelope(n, 0.03, seconds / 2.5)


def chest_open(rng):
    n = samples(0.75)
    hinge = creak(samples(0.45), rng, 55, 140)
    x = np.zeros(n)
    place(x, hinge, 0)
    place(x, step("wood", rng) * 0.4, samples(0.42))
    return level(trim(x), -20)


def chest_close(rng):
    n = samples(0.6)
    x = np.zeros(n)
    place(x, creak(samples(0.18), rng, 120, 70) * 0.6, 0)
    lid = modes(samples(0.4), rng, [110, 240, 520, 1000], [0.08, 0.06, 0.03, 0.015], [1, 0.7, 0.4, 0.2])
    lid += thump(samples(0.4), rng, 80, 0.05) * 0.8
    place(x, lid, samples(0.16))
    return level(trim(x), -18)


def click(rng):
    n = samples(0.05)
    tick = modes(n, rng, [1750, 3600], [0.006, 0.003], [1, 0.4], 0.02)
    snap = highpass(rng.standard_normal(n), 3000) * envelope(n, 0.0002, 0.0012)
    return level(trim(tick + snap * 0.5), -26)


def bleat(rng):
    """A sheep-like call for creatures (M3.1): a nasal buzz with a shaky pitch, its formants
    an open "eh"."""
    seconds = rng.uniform(0.6, 0.9)
    n = samples(seconds)
    t = times(n)
    f0 = rng.uniform(260, 330) * (1 + 0.04 * np.sin(2 * np.pi * rng.uniform(5.5, 7.5) * t)) * (1 - 0.12 * t / seconds)
    phase = np.cumsum(f0) / SR
    buzz = sum(np.sin(2 * np.pi * k * phase) / k for k in range(1, 30) if k * f0.max() < SR / 2.2)
    voice = sum(resonator(buzz, f, q) * a for f, q, a in ((620, 5, 1.0), (1750, 6, 0.7), (2600, 7, 0.4)))
    x = voice * envelope(n, 0.05, seconds / 2.5, hold=seconds * 0.3)
    return level(trim(x), -19)


def voiced(f0, harmonics=30):
    """A buzz following the pitch `f0`, a sample at a time, its harmonics falling off as 1/k."""
    phase = np.cumsum(f0) / SR
    return sum(np.sin(2 * np.pi * k * phase) / k for k in range(1, harmonics) if k * f0.max() < SR / 2.2)


def moo(rng):
    """A cow's call (M3.2): a long low hum through closed lips opening out to an "oo-aa", its pitch
    rising a little and falling away."""
    seconds = rng.uniform(1.0, 1.4)
    n = samples(seconds)
    t = times(n) / seconds
    f0 = rng.uniform(100, 125) * (1 + 0.18 * np.sin(np.pi * t) - 0.1 * t) * (1 + 0.01 * np.sin(2 * np.pi * 5 * times(n)))
    buzz = voiced(f0, 40)
    closed = lowpass(buzz, 350)
    opened = sum(resonator(buzz, f, q) * a for f, q, a in ((450, 5, 1.0), (900, 6, 0.6), (2400, 8, 0.15)))
    mouth = np.clip(t * 2.5 - 0.3, 0, 1)
    x = (closed * (1 - mouth) + opened * mouth) * envelope(n, 0.12, seconds / 3, hold=seconds * 0.4)
    return level(trim(x), -19)


def oink(rng):
    """A pig's grunts (M3.2): two or three short, rough, nasal snorts, the last a little longer."""
    out = []
    count = rng.integers(2, 4)
    for i in range(count):
        d = rng.uniform(0.09, 0.14) * (1.5 if i == count - 1 else 1.0)
        n = samples(d)
        f0 = rng.uniform(140, 190) * (1 - 0.15 * times(n) / d)
        rough = 1 + 0.6 * np.sin(2 * np.pi * rng.uniform(35, 50) * times(n))
        buzz = voiced(f0) * rough + bandpass(rng.standard_normal(n), 400, 2500) * 0.3
        nose = sum(resonator(buzz, f, q) * a for f, q, a in ((350, 4, 1.0), (1100, 5, 0.7), (2600, 6, 0.3)))
        out.append(nose * envelope(n, 0.008, d / 2.5, hold=d * 0.3))
        out.append(np.zeros(samples(rng.uniform(0.05, 0.1))))
    return level(trim(np.concatenate(out)), -20)


def cluck(rng):
    """A hen's clucks (M3.2): a few quick, bright knocks of voice, rising at the last."""
    out = []
    count = rng.integers(3, 6)
    for i in range(count):
        d = rng.uniform(0.05, 0.08)
        n = samples(d)
        f0 = rng.uniform(380, 480) * (1.3 if i == count - 1 else 1.0) * (1 + 0.2 * times(n) / d)
        voice = sum(resonator(voiced(f0), f, q) * a for f, q, a in ((950, 5, 1.0), (2300, 7, 0.6), (3400, 8, 0.25)))
        out.append(voice * envelope(n, 0.003, d / 3))
        out.append(np.zeros(samples(rng.uniform(0.06, 0.14))))
    return level(trim(np.concatenate(out)), -21)


# --- hostile creatures (M3.3) -------------------------------------------------------------------


def formants(x, table):
    """`x` through a vocal tract: resonances at each frequency, with their quality and level."""
    return sum(resonator(x, f, q) * a for f, q, a in table)


def tick(rng, freq, decay=0.004):
    """A dry click of a hard shell: a few struck modes and a snap of noise, a few ms long."""
    n = samples(decay * 8)
    snap = bandpass(rng.standard_normal(n), freq * 0.6, min(freq * 2.5, SR / 2 - 200)) * envelope(n, 0.0002, decay / 2)
    return modes(n, rng, [freq, freq * 1.7, freq * 2.9], [decay, decay * 0.7, decay * 0.4], [1, 0.6, 0.3], 0.08) + snap * 0.6


def clicks(seconds, rng, rate, freq, slowing=1.0):
    """A run of `tick`s at about `rate` a second, each a little different, the gaps stretching by
    `slowing` as it goes, ringing a small hollow body."""
    out = np.zeros(samples(seconds))
    at, gap = 0.0, 1 / rate
    while at < seconds - 0.03:
        place(out, tick(rng, freq * rng.uniform(0.8, 1.25)) * rng.uniform(0.4, 1.0), samples(at))
        at += gap * rng.uniform(0.6, 1.4)
        gap *= slowing
    return out + formants(out, ((freq * 0.45, 6, 0.4),)) * 0.5


def growl(n, rng, f0, rough=40.0):
    """A rough voiced buzz on the pitch `f0`, its loudness shaken quickly, as a throat growls."""
    shake = 1 + 0.7 * np.sin(2 * np.pi * np.cumsum(rough * rng.uniform(0.85, 1.15, n) / SR))
    return voiced(f0, 40) * shake + bandpass(coloured(n, rng, -1), 200, 2000) * 0.25


def gloamer_call(rng):
    """A gloamer's call: a low breathy rasp of a growl that rises and thins into an eerie whine."""
    seconds = rng.uniform(1.1, 1.45)
    n = samples(seconds)
    t = times(n) / seconds
    rise = np.clip((t - 0.45) / 0.4, 0, 1)
    f0 = rng.uniform(85, 105) * (1 + 6.0 * rise**1.5) * (1 + 0.015 * np.sin(2 * np.pi * 6 * times(n)))
    low = formants(growl(n, rng, f0, 35) * (1 - rise), ((380, 4, 1.0), (900, 5, 0.5)))
    whine = formants(voiced(f0, 12) * rise, ((1200, 5, 1.0), (2600, 8, 0.4)))
    breath = bandpass(coloured(n, rng, -1), 500, 3000) * 0.12
    x = (low + whine * 0.8 + breath) * envelope(n, 0.15, seconds / 3.5, hold=seconds * 0.5)
    return level(trim(x), -20)


def gloamer_hurt(rng):
    """A gloamer hurt: a sharp yelp, high and nasal, falling fast."""
    seconds = rng.uniform(0.25, 0.32)
    n = samples(seconds)
    f0 = rng.uniform(620, 720) * (1 - 0.45 * times(n) / seconds)
    x = formants(voiced(f0, 15), ((1400, 5, 1.0), (2900, 7, 0.5))) * envelope(n, 0.004, seconds / 3)
    return level(trim(x), -17)


def gloamer_attack(rng):
    """A gloamer lunging: a short snarling bark, a rough burst of throat opening and shutting."""
    seconds = rng.uniform(0.35, 0.45)
    n = samples(seconds)
    f0 = rng.uniform(150, 190) * (1 + 0.4 * np.sin(np.pi * times(n) / seconds))
    snarl = formants(growl(n, rng, f0, 55), ((500, 3, 1.0), (1300, 4, 0.7), (2800, 6, 0.3)))
    x = snarl * envelope(n, 0.01, seconds / 3, hold=seconds * 0.25)
    return level(trim(x), -17)


def gloamer_death(rng):
    """A gloamer dying: a whimper falling away and dissolving into a last breath."""
    seconds = rng.uniform(1.2, 1.5)
    n = samples(seconds)
    t = times(n) / seconds
    f0 = rng.uniform(520, 600) * (1 - 0.6 * t) * (1 + 0.03 * np.sin(2 * np.pi * 7 * times(n)))
    voice = formants(voiced(f0, 12), ((1100, 5, 1.0), (2400, 7, 0.4))) * np.clip(1.2 - t * 1.6, 0, 1)
    breath = bandpass(coloured(n, rng, -1), 400, 2500) * np.clip((t - 0.25) * 2, 0, 1) * 0.8
    x = (voice + breath) * envelope(n, 0.03, seconds / 3, hold=seconds * 0.35)
    return level(trim(x), -18)


def lurker_call(rng):
    """A lurker's call: dry, resonant chittering, clicks in two or three quick runs."""
    out = []
    for _ in range(rng.integers(2, 4)):
        d = rng.uniform(0.18, 0.32)
        out.append(clicks(d, rng, rng.uniform(35, 55), rng.uniform(2200, 3000)) * envelope(samples(d), 0.01, d / 2, hold=d / 3))
        out.append(np.zeros(samples(rng.uniform(0.08, 0.18))))
    return level(trim(np.concatenate(out)), -21)


def lurker_hurt(rng):
    """A lurker hurt: a shrill squeal over a hiss."""
    seconds = rng.uniform(0.3, 0.4)
    n = samples(seconds)
    f0 = rng.uniform(1700, 1950) * (1 + 0.15 * np.sin(np.pi * times(n) / seconds))
    squeal = formants(voiced(f0, 5), ((3200, 6, 1.0),))
    hiss = bandpass(rng.standard_normal(n), 3500, 8000) * 0.1
    x = (squeal + hiss) * envelope(n, 0.005, seconds / 3, hold=seconds * 0.2)
    return level(trim(x), -18)


def lurker_attack(rng):
    """A lurker dropping on its prey: a long hiss that ends in a burst of quick clicks."""
    seconds = rng.uniform(0.55, 0.7)
    n = samples(seconds)
    hiss = bandpass(rng.standard_normal(n), 2500, 9000) * envelope(n, 0.04, seconds / 3, hold=seconds * 0.3)
    burst = clicks(0.25, rng, 70, rng.uniform(2400, 3200))
    x = hiss * 0.7
    place(x, burst, samples(seconds - 0.3))
    return level(trim(x), -17)


def lurker_death(rng):
    """A lurker dying: its clicks rattling on, slower and slower, to nothing."""
    seconds = 1.3
    x = clicks(seconds, rng, 45, rng.uniform(1900, 2500), slowing=1.12)
    return level(trim(x * envelope(len(x), 0.005, 0.5, hold=0.2)), -18)


def slinger_call(rng):
    """A slinger's call: nasal gibbering, a few short high syllables up and down."""
    out = []
    for _ in range(rng.integers(3, 6)):
        d = rng.uniform(0.06, 0.12)
        n = samples(d)
        f0 = rng.uniform(330, 480) * (1 + rng.uniform(-0.25, 0.25) * times(n) / d)
        vowel = rng.choice([700, 1000, 500])
        out.append(formants(voiced(f0, 20), ((vowel, 5, 1.0), (2200, 6, 0.6), (3000, 7, 0.3))) * envelope(n, 0.006, d / 3, hold=d * 0.3))
        out.append(np.zeros(samples(rng.uniform(0.03, 0.09))))
    return level(trim(np.concatenate(out)), -20)


def slinger_hurt(rng):
    """A slinger hurt: a quick squeak, rising."""
    seconds = rng.uniform(0.15, 0.2)
    n = samples(seconds)
    f0 = rng.uniform(800, 950) * (1 + 0.35 * times(n) / seconds)
    x = formants(voiced(f0, 10), ((1800, 5, 1.0), (3400, 7, 0.4))) * envelope(n, 0.004, seconds / 3, hold=seconds * 0.3)
    return level(trim(x), -17)


def slinger_attack(rng):
    """A slinger throwing: a grunt of effort and the whoosh of the sling swung round and let go."""
    seconds = 0.6
    n = samples(seconds)
    d = 0.16
    grunt = formants(growl(samples(d), rng, rng.uniform(220, 260) * np.ones(samples(d)), 30), ((600, 4, 1.0), (1500, 5, 0.5)))
    grunt *= envelope(samples(d), 0.01, d / 3)
    t = times(n)
    # the sling's whoosh, a band of noise swept up and swelling as it comes round
    swish = np.clip((t - 0.12) / 0.35, 0, 1)
    whoosh = bandpass(rng.standard_normal(n), 400, 1400) * (1 - swish) + bandpass(rng.standard_normal(n), 1400, 4500) * swish
    whoosh *= np.sin(np.pi * np.clip((t - 0.1) / 0.45, 0, 1)) ** 2
    x = whoosh * 0.7
    place(x, grunt, 0)
    return level(trim(x), -18)


def slinger_death(rng):
    """A slinger dying: a thin wail falling away."""
    seconds = rng.uniform(0.9, 1.1)
    n = samples(seconds)
    t = times(n) / seconds
    f0 = rng.uniform(560, 640) * (1 - 0.55 * t) * (1 + 0.04 * np.sin(2 * np.pi * 6 * times(n)))
    x = formants(voiced(f0, 15), ((900, 5, 1.0), (2300, 6, 0.5))) * envelope(n, 0.02, seconds / 3, hold=seconds * 0.4)
    return level(trim(x), -18)


def crumble(seconds, rng, count):
    """Stone grinding and falling: low gritty grains and a few heavier knocks."""
    n = samples(seconds)
    grit = grains(n, rng, count, 150, 1500, length=(0.003, 0.012))
    for _ in range(max(2, count // 25)):
        place(grit, thump(samples(0.3), rng, rng.uniform(60, 110), 0.07) * rng.uniform(0.5, 1.0), samples(rng.uniform(0, seconds * 0.8)))
    return grit


def barrow_brute_call(rng):
    """A barrow brute's call: a deep, slow groan, rumbling from far down a stone chest."""
    seconds = rng.uniform(1.3, 1.6)
    n = samples(seconds)
    t = times(n) / seconds
    f0 = rng.uniform(55, 75) * (1 + 0.12 * np.sin(np.pi * t)) * (1 + 0.02 * np.sin(2 * np.pi * 3 * times(n)))
    x = formants(growl(n, rng, f0, 18), ((250, 3, 1.0), (520, 4, 0.6), (1200, 6, 0.2)))
    x = lowpass(x, 2000) * envelope(n, 0.25, seconds / 3, hold=seconds * 0.45)
    return level(trim(x), -19)


def barrow_brute_hurt(rng):
    """A barrow brute hurt: a short deep grunt over the grind of stone."""
    seconds = 0.45
    n = samples(seconds)
    f0 = rng.uniform(85, 105) * (1 - 0.25 * times(n) / seconds)
    grunt = formants(growl(n, rng, f0, 25), ((300, 3, 1.0), (700, 4, 0.5))) * envelope(n, 0.01, 0.1, hold=0.08)
    x = grunt + crumble(seconds, rng, 50) * 0.5
    return level(trim(x), -17)


def barrow_brute_attack(rng):
    """A barrow brute's blow: a big heaving roar, swelling and breaking off."""
    seconds = rng.uniform(0.8, 1.0)
    n = samples(seconds)
    t = times(n) / seconds
    f0 = rng.uniform(80, 95) * (1 + 0.5 * np.sin(np.pi * t))
    roar = formants(growl(n, rng, f0, 30), ((420, 2.5, 1.0), (900, 3, 0.7), (2000, 5, 0.3)))
    roar += bandpass(coloured(n, rng, -1), 300, 3000) * 0.4
    x = roar * envelope(n, 0.18, seconds / 5, hold=seconds * 0.5)
    return level(trim(x), -17)


def barrow_brute_death(rng):
    """A barrow brute dying: a long groan sinking down as its stone body crumbles apart."""
    seconds = 1.6
    n = samples(seconds)
    t = times(n) / seconds
    f0 = rng.uniform(70, 80) * (1 - 0.45 * t)
    groan = formants(growl(n, rng, f0, 15), ((260, 3, 1.0), (560, 4, 0.5))) * np.clip(1.3 - t * 1.5, 0, 1)
    x = lowpass(groan, 1800) + crumble(seconds, rng, 160) * np.clip(t * 2 - 0.3, 0, 1) * 0.7
    x *= envelope(n, 0.08, 0.4, hold=1.0)
    return level(trim(x), -18)


HOSTILES = ["gloamer", "lurker", "slinger", "barrow_brute"]


# --- ambience loops ------------------------------------------------------------------------------

LOOP_DB = -27


def cave(rng):
    n = samples(24)
    phase = 2 * np.pi * np.arange(n) / n
    hum = periodic_band(n, rng, 30, 160, -1.5) * (0.6 + 0.4 * lfo(n, rng))
    tone = sum(a * np.sin(f * phase + rng.uniform(0, 6.3)) for f, a in ((24 * 55, 0.25), (24 * 82, 0.12)))
    drips = np.zeros(n)
    at = rng.uniform(0, 1)
    while at < 24:
        f0 = np.exp(rng.uniform(np.log(1100), np.log(2600)))
        d = samples(0.1)
        t = times(d)
        plink = np.sin(2 * np.pi * np.cumsum(f0 * (1 + 0.6 * np.minimum(t / 0.02, 1))) / SR) * envelope(d, 0.001, 0.025)
        place(drips, plink * rng.uniform(0.3, 1.0), samples(at), wrap=True)
        at += rng.uniform(1.0, 3.2)
    room = reverb_ir(2.4, rng, bright=0.25, early=((0.09, 0.5), (0.17, 0.35), (0.31, 0.2)))
    drips = drips * 0.4 + convolve(drips, room, wrap=True) * 1.1
    return level(hum * 0.7 + tone * 0.15 + drips, LOOP_DB, window=24)


# --- music ---------------------------------------------------------------------------------------

MUSIC_DB = -24


def note_freq(name):
    names = {"C": -9, "C#": -8, "D": -7, "D#": -6, "E": -5, "F": -4, "F#": -3, "G": -2, "G#": -1, "A": 0, "A#": 1, "B": 2}
    return 440.0 * 2 ** ((names[name[:-1]] + 12 * (int(name[-1]) - 4)) / 12)


def pad_voice(freq, seconds, rng, bright=1500):
    """A soft pad note: three detuned voices of a mellow harmonic tone, swelling in and out."""
    n = samples(seconds)
    t = times(n)
    tone = np.zeros(n)
    for detune in (-0.004, 0.0, 0.004):
        f = freq * (1 + detune)
        tone += sum(np.sin(2 * np.pi * k * f * t + rng.uniform(0, 6.3)) / k**2 for k in range(1, 7) if k * f < 8000)
    tone = lowpass(tone, bright)
    swell = np.minimum(1, t / (seconds * 0.35)) * np.minimum(1, (seconds - t) / (seconds * 0.4))
    return tone * np.sin(np.pi / 2 * np.clip(swell, 0, 1)) ** 2


def pluck(freq, seconds, rng, decay=0.996, bright=0.5):
    """A plucked string, Karplus-Strong: a burst of noise round a delay loop that dulls it."""
    n = samples(seconds)
    period = int(round(SR / freq))
    burst = np.zeros(n)
    burst[:period] = lowpass(rng.uniform(-1, 1, period), 1500 + 6000 * bright)
    a = np.zeros(period + 2)
    a[0], a[period], a[period + 1] = 1.0, -decay / 2, -decay / 2
    return signal.lfilter([1.0], a, burst) * envelope(n, 0.002, 1e9)


def bell(freq, seconds, rng):
    """A soft bell by FM: a sine wobbled by another at a bell's odd ratio, mellowing as it rings."""
    n = samples(seconds)
    t = times(n)
    index = 2.5 * np.exp(-t / 0.4)
    x = np.sin(2 * np.pi * freq * t + index * np.sin(2 * np.pi * freq * 3.5 * t))
    return x * envelope(n, 0.003, seconds / 4)


def piece(rng, chords, scale, bar, bars, voice):
    """Pads on `chords`, a bar each, under sparse notes wandering the `scale`, in a big room."""
    length = bar * bars + 6
    n = samples(length)
    left, right = np.zeros(n), np.zeros(n)
    for i in range(bars):
        for name in chords[i % len(chords)]:
            note = pad_voice(note_freq(name), bar * 1.6, rng) * 0.22
            pan = rng.uniform(0.3, 0.7)
            place(left, note * (1 - pan), samples(i * bar))
            place(right, note * pan, samples(i * bar))
    at, step_ = bar * 0.5, rng.integers(0, len(scale))
    while at < bar * bars - 2:
        step_ = int(np.clip(step_ + rng.choice([-2, -1, -1, 1, 1, 2, 0]), 0, len(scale) - 1))
        f = note_freq(scale[step_])
        note = (pluck(f, 3.0, rng) if voice == "pluck" or rng.random() < 0.6 else bell(f, 3.0, rng)) * rng.uniform(0.35, 0.5)
        pan = rng.uniform(0.2, 0.8)
        place(left, note * (1 - pan), samples(at))
        place(right, note * pan, samples(at))
        # phrases of a few notes, then a rest
        at += rng.choice([0.5, 0.75, 1.0, 1.0, 1.5]) * bar / 4 if rng.random() > 0.2 else bar * rng.uniform(0.75, 1.5)
    rooms = [reverb_ir(4.0, rng, bright=0.2) for _ in range(2)]
    left = left + convolve(left, rooms[0])[:n] * 0.9
    right = right + convolve(right, rooms[1])[:n] * 0.9
    x = np.stack([left, right], 1)
    fade = np.minimum(1, times(n) / 3)[:, None]
    x = fade_out(x * fade, 6)
    return level(x, MUSIC_DB, window=length)


def morning(rng):
    chords = [["D3", "A3", "E4", "F#4"], ["B2", "F#3", "D4", "A4"], ["G2", "D3", "B3", "F#4"], ["A2", "E3", "B3", "C#4"]]
    scale = ["D4", "E4", "F#4", "A4", "B4", "D5", "E5", "F#5", "A5"]
    return piece(rng, chords, scale, 8.0, 8, "pluck")


def drift(rng):
    chords = [["A2", "E3", "B3", "C4"], ["F2", "C3", "A3", "E4"], ["C3", "G3", "D4", "E4"], ["G2", "D3", "A3", "B3"]]
    scale = ["A3", "C4", "D4", "E4", "G4", "A4", "C5", "D5", "E5"]
    return piece(rng, chords, scale, 9.0, 8, "mixed")


def embers(rng):
    chords = [["G2", "D3", "B3", "F#4"], ["E2", "B2", "G3", "F#4"], ["C3", "G3", "D4", "E4"], ["D3", "A3", "D4", "E4"]]
    scale = ["G3", "A3", "B3", "D4", "E4", "G4", "A4", "B4", "D5"]
    return piece(rng, chords, scale, 10.0, 6, "mixed")


# --- the list ------------------------------------------------------------------------------------


def catalogue():
    """Every file to write, by its path under assets/sounds, with what makes it and its seed."""
    out = {}
    for s, surface in enumerate(SURFACES):
        for v in range(VARIATIONS):
            out[f"step/{surface}_{v + 1}"] = (lambda rng, s=surface: step(s, rng), 100 * s + v)
            out[f"break/{surface}_{v + 1}"] = (lambda rng, s=surface: break_(s, rng), 1000 + 100 * s + v)
    for name, make in (("swim", swim), ("splash", splash), ("pickup", pickup), ("eat", eat), ("hurt", hurt), ("bleat", bleat),
                       ("moo", moo), ("oink", oink), ("cluck", cluck)):
        for v in range(VARIATIONS):
            out[f"{name}_{v + 1}"] = (make, 5000 + v)
    for name, make in (("death", death), ("chest_open", chest_open), ("chest_close", chest_close), ("click", click)):
        out[name] = (make, 7)
    for h, kind in enumerate(HOSTILES):
        for c, cry in enumerate(("call", "hurt", "attack", "death")):
            for v in range(VARIATIONS):
                out[f"{kind}_{cry}_{v + 1}"] = (globals()[f"{kind}_{cry}"], 9000 + 100 * h + 10 * c + v)
    # the other loops and the owl are recordings, levelled once and kept as they are, see
    # SKILL.md's Recordings
    out["ambience/cave"] = (cave, 11)
    for name, make in (("morning", morning), ("drift", drift), ("embers", embers)):
        out[f"music/{name}"] = (make, 3)
    return out


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("only", nargs="*", help="only the sounds whose paths start with these")
    parser.add_argument("--wav", type=pathlib.Path, help="also write WAVs here")
    args = parser.parse_args()
    for path, (make, seed) in catalogue().items():
        if args.only and not any(path.startswith(o) for o in args.only):
            continue
        # each path seeds its own, so changing one sound leaves the rest as they were
        began = time.perf_counter()
        rng = np.random.default_rng([seed, sum(map(ord, path))])
        x = make(rng).astype(np.float32)
        file = OUT / f"{path}.ogg"
        file.parent.mkdir(parents=True, exist_ok=True)
        # music and loops take a little more compression, being long
        quality = 0.7 if path.startswith("music/") else 0.6 if path.startswith("ambience/") else 0.4
        # in blocks, as libsndfile's Vorbis encoder crashes given a long stereo file at once
        x = np.ascontiguousarray(x)
        with sf.SoundFile(file, "w", SR, x.shape[1] if x.ndim > 1 else 1, format="OGG", subtype="VORBIS", compression_level=quality) as out:
            for start in range(0, len(x), SR):
                out.write(x[start : start + SR])
        if args.wav:
            (args.wav / path).parent.mkdir(parents=True, exist_ok=True)
            sf.write(args.wav / f"{path}.wav", x, SR)
        print(f"{path}: {len(x) / SR:.2f}s, {time.perf_counter() - began:.1f}s to make")


if __name__ == "__main__":
    main()
