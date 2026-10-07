import { useSyncExternalStore } from "react";
import { EQ_BANDS_HZ, EQ_PRESETS, EQ_Q, autoPreampDb, presetFor, snapGain } from "./eq";
import { bandLevels, follow, logBandEdges, meanLevel, onset } from "./levels";
import { scrobbleAt } from "./track";

/*
 * The page's one audio player: the hero deck, the floating mini player, the EQ and the Discord
 * card all drive this. It also writes --bass, --level and --progress on <html> every frame, which
 * is how the rest of the page moves with the music without re-rendering.
 */

export const TRACK = {
  src: "./low-tide.mp3",
  title: "Low Tide",
  artist: "Tom Rhodes, Hustle Standard",
  cover: "./ghost.webp",
} as const;

/** Half the visualiser ring; the other half mirrors it. */
export const SPECTRUM_BANDS = 40;
const EDGES = logBandEdges(SPECTRUM_BANDS, 40, 14_000);
const BASS_BANDS = 7;

export interface PlayerState {
  playing: boolean;
  started: boolean;
  time: number;
  duration: number;
  scrobbles: number;
}

export interface EqState {
  bands: readonly number[];
  preset: string | null;
}

const root = document.documentElement;
let player: PlayerState = { playing: false, started: false, time: 0, duration: 0, scrobbles: 0 };
let eq: EqState = { bands: EQ_PRESETS[0].bands, preset: EQ_PRESETS[0].name };
const listeners = new Set<() => void>();

let audio: HTMLAudioElement | null = null;
let context: AudioContext | null = null;
let analyser: AnalyserNode | null = null;
let preamp: GainNode | null = null;
let filters: BiquadFilterNode[] = [];
let scrobbled = false;

function notify() {
  for (const listener of listeners) listener();
}

function update(next: Partial<PlayerState>) {
  player = { ...player, ...next };
  notify();
}

function media(): HTMLAudioElement {
  if (audio) return audio;
  const element = new Audio(TRACK.src);
  element.preload = "metadata";
  element.loop = true;
  element.addEventListener("play", () => {
    root.dataset.playing = "true";
    update({ playing: true, started: true });
    startMeter();
  });
  element.addEventListener("pause", () => {
    root.dataset.playing = "false";
    update({ playing: false });
  });
  element.addEventListener("loadedmetadata", () => update({ duration: element.duration }));
  element.addEventListener("timeupdate", () => onTime(element.currentTime));
  audio = element;
  return element;
}

function onTime(time: number) {
  // Looping back to the start is a new play, so it may scrobble again.
  if (time < player.time - 1) scrobbled = false;
  const threshold = scrobbleAt(player.duration);
  const due = !scrobbled && threshold !== null && time >= threshold;
  if (due) scrobbled = true;
  root.style.setProperty("--progress", player.duration > 0 ? (time / player.duration).toFixed(4) : "0");
  update({ time, scrobbles: player.scrobbles + (due ? 1 : 0) });
}

// Built on the first press: audio only starts from a gesture, and an element can feed one graph, once.
function connect(element: HTMLAudioElement) {
  if (context || typeof AudioContext === "undefined") return;
  try {
    const graph = new AudioContext();
    const source = graph.createMediaElementSource(element);
    const gain = graph.createGain();
    const chain = EQ_BANDS_HZ.map((hz, band) => {
      const filter = graph.createBiquadFilter();
      filter.type = "peaking";
      filter.frequency.value = hz;
      filter.Q.value = EQ_Q;
      filter.gain.value = eq.bands[band];
      return filter;
    });
    const meter = graph.createAnalyser();
    meter.fftSize = 2048;
    meter.smoothingTimeConstant = 0.7;
    const nodes: AudioNode[] = [source, gain, ...chain, meter];
    nodes.reduce((from, to) => from.connect(to));
    meter.connect(graph.destination);
    context = graph;
    analyser = meter;
    preamp = gain;
    filters = chain;
    applyEq();
  } catch {
    // No Web Audio: the track still plays, without the EQ or the visualiser.
  }
}

export function togglePlayback() {
  const element = media();
  if (!element.paused) {
    element.pause();
    return;
  }
  connect(element);
  void context?.resume();
  if ("mediaSession" in navigator) {
    navigator.mediaSession.metadata = new MediaMetadata({
      title: TRACK.title,
      artist: TRACK.artist,
      album: "zuno_",
      artwork: [{ src: TRACK.cover, sizes: "256x256", type: "image/webp" }],
    });
  }
  element.play().catch(() => update({ playing: false }));
}

export function seek(seconds: number) {
  media().currentTime = seconds;
}

function applyEq() {
  const now = context?.currentTime ?? 0;
  filters.forEach((filter, band) => filter.gain.setTargetAtTime(eq.bands[band], now, 0.02));
  preamp?.gain.setTargetAtTime(10 ** (autoPreampDb(eq.bands) / 20), now, 0.02);
  notify();
}

export function setEqBand(band: number, db: number) {
  const bands = eq.bands.map((gain, index) => (index === band ? snapGain(db) : gain));
  eq = { bands, preset: presetFor(bands) };
  applyEq();
}

export function applyEqPreset(name: string) {
  const preset = EQ_PRESETS.find((candidate) => candidate.name === name);
  if (!preset) return;
  eq = { bands: preset.bands, preset: preset.name };
  applyEq();
}

// ── Meter ────────────────────────────────────────────────────────────────

type SpectrumListener = (levels: Float32Array) => void;
const spectrumListeners = new Set<SpectrumListener>();
const raw = new Float32Array(SPECTRUM_BANDS);
const smoothed = new Float32Array(SPECTRUM_BANDS);
const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");
let bins: Uint8Array | null = null;
let frame = 0;
let bassFast = 0;
let bassSlow = 0;
let bass = 0;
let level = 0;

function startMeter() {
  if (frame === 0) frame = requestAnimationFrame(tick);
}

function tick() {
  frame = 0;
  const live = analyser !== null && context !== null && audio !== null && !audio.paused && !reducedMotion.matches;
  if (live && analyser && context) {
    bins ??= new Uint8Array(analyser.frequencyBinCount);
    analyser.getByteFrequencyData(bins);
    bandLevels(bins, EDGES, context.sampleRate, raw);
  } else {
    raw.fill(0);
  }

  let loudest = 0;
  for (let band = 0; band < SPECTRUM_BANDS; band += 1) {
    smoothed[band] = follow(smoothed[band], raw[band], 0.55, 0.14);
    loudest = Math.max(loudest, smoothed[band]);
  }
  // --bass follows kicks, not loudness: a steady bass line would otherwise pin it at the top.
  const low = meanLevel(raw, 0, BASS_BANDS);
  bassFast = follow(bassFast, low, 0.7, 0.3);
  bassSlow = follow(bassSlow, low, 0.04, 0.04);
  bass = follow(bass, onset(bassFast, bassSlow), 0.85, 0.12);
  level = follow(level, meanLevel(raw, 0, SPECTRUM_BANDS), 0.4, 0.08);

  root.style.setProperty("--bass", bass.toFixed(3));
  root.style.setProperty("--level", level.toFixed(3));
  for (const listener of spectrumListeners) listener(smoothed);

  // Keeps going after a pause until everything has fallen, so the bars settle instead of freezing.
  if (live || loudest > 0.003 || level > 0.003) frame = requestAnimationFrame(tick);
}

// ── React ────────────────────────────────────────────────────────────────

function subscribe(listener: () => void) {
  media();
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function usePlayer(): PlayerState {
  return useSyncExternalStore(subscribe, () => player);
}

export function useEq(): EqState {
  return useSyncExternalStore(subscribe, () => eq);
}

export function subscribeSpectrum(listener: SpectrumListener): () => void {
  spectrumListeners.add(listener);
  return () => {
    spectrumListeners.delete(listener);
  };
}
