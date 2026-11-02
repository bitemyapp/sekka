// The home page's video, drawn by this browser. The video's own program,
// built for Wasm (site/video/), writes each frame as SVG, and the
// soundtrack's clock says which frame is due. Nothing is fetched until the
// page has loaded, and then only from this site, at low priority; with Save
// Data on, nothing until Play. Nothing plays until Play is pressed.

const player = document.querySelector(".player");
const screen = player.querySelector(".player-screen");
const poster = player.querySelector(".player-poster");
const big = player.querySelector(".player-big");
const bigLabel = player.querySelector(".player-big-label");
const bar = player.querySelector(".player-bar");
const toggle = player.querySelector(".player-toggle");
const time = player.querySelector(".player-time");
const seek = player.querySelector(".player-seek");
const mute = player.querySelector(".player-mute");
const volume = player.querySelector(".player-volume");
const full = player.querySelector(".player-full");
const note = player.querySelector(".player-note");

let video = null; // the program, its soundtrack and its length, once loaded
let loading = null;
let audio = null; // made on the first Play, as browsers ask
let gain = null;
let source = null; // the soundtrack, while it plays
let start = 0; // the audio clock's time at frame 0
let shown = -1; // the frame on screen
let frame = 0; // where the video stands when it isn't playing
let origin = 0; // the frame it last began to play from
let resume = false; // whether to play on when a drag along the bar ends
let level = 1;
let muted = false;

// Fonts and ids are the whole page's: the video's are renamed, so its
// JetBrains Mono can't stand in for the site's and its ids can't meet the
// page's. A family becomes one identifier, "hype-Noto-Sans-Symbols-2",
// which needs no quotes: Chrome keeps the quotes of a quoted name given to
// FontFace as part of the name.
const family = (name) => `hype-${name.replaceAll(" ", "-")}`;

function load() {
  loading ??= open().then(
    (v) => {
      video = v;
      seek.max = String(v.total - 1);
      seek.disabled = false;
      showTime(frame);
      return v;
    },
    (e) => {
      loading = null;
      throw e;
    },
  );
  return loading;
}

async function open() {
  const low = { priority: "low" };
  const src = player.dataset;
  const glue = await import(src.moduleSrc);
  await glue.default({ module_or_path: fetch(src.wasmSrc, low) });
  const bridge = glue.create_wasm_bridge();

  // The fonts are inside the program; the browser draws with the same files,
  // so its text has the widths the frames were laid out with. A family's
  // fallbacks are added before it: the browser tries the last added first,
  // and a fallback only for the characters it is there for.
  bridge.populate_static_fonts_db_with_static_fonts();
  const fonts = [];
  for (let i = 0; ; i++) {
    const font = bridge.get_static_font_data_by_index(i);
    if (!font) break;
    const { name, weight, style, stretch } = font.info;
    fonts.push({
      name,
      data: font.data,
      descriptors: {
        weight: String(weight),
        style: style.toLowerCase(),
        stretch: stretch.replace(/([a-z])([A-Z])/g, "$1-$2").toLowerCase(),
      },
    });
  }
  for (const font of fonts) {
    for (const { from, unicodeRange } of glue.fallbacks.filter((f) => f.family === font.name)) {
      const face = new FontFace(family(font.name), fonts[from].data, { ...font.descriptors, unicodeRange });
      document.fonts.add(await face.load());
    }
    document.fonts.add(await new FontFace(family(font.name), font.data, font.descriptors).load());
  }

  const images = JSON.parse(src.images);
  const shots = glue.images.map(async (name) => {
    if (!images[name]) throw new Error(`no URL for ${name}`);
    const image = new Image();
    image.fetchPriority = "low";
    image.src = images[name];
    await image.decode();
    bridge.add_image_source(name, image.src, image.naturalWidth, image.naturalHeight, undefined);
  });
  const [score] = await Promise.all([soundtrack(), ...shots]);

  glue.durations.set(glue.audio, score.duration);
  const { fps, durationInFrames } = await bridge.prepare(undefined);
  const own = (svg) =>
    svg
      .replace(/font-family="([^"]*)"/g, (_, name) => `font-family="${family(name)}"`)
      .replace(/ id="/g, ' id="hype-')
      .replace(/url\(#/g, "url(#hype-")
      .replace("<svg ", `<svg font-family="${family(glue.defaultFont)}" `);
  return { bridge, score, fps, total: durationInFrames, own };
}

// Opus where the browser can decode it, AAC otherwise; decoded on an offline
// context, since a live one may only be made for Play.
async function soundtrack() {
  const src = player.dataset;
  const opus = document.createElement("audio").canPlayType('audio/webm; codecs="opus"');
  const decoder = new OfflineAudioContext(2, 1, 48000);
  let failure;
  for (const url of opus ? [src.opusSrc, src.aacSrc] : [src.aacSrc]) {
    try {
      const response = await fetch(url, { priority: "low" });
      if (!response.ok) throw new Error(`${url}: ${response.status}`);
      return await decoder.decodeAudioData(await response.arrayBuffer());
    } catch (e) {
      failure = e;
    }
  }
  throw failure;
}

function draw(n) {
  const svg = video.own(video.bridge.render_frame(BigInt(n)));
  screen.querySelector("svg")?.remove();
  screen.insertAdjacentHTML("beforeend", svg);
  poster.hidden = true;
  shown = n;
  showTime(n);
}

function clock(n) {
  const s = Math.floor(n / video.fps);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

let lastSecond = -1;
function showTime(n) {
  seek.value = String(n);
  const s = Math.floor(n / video.fps);
  if (s === lastSecond) return;
  lastSecond = s;
  time.textContent = `${clock(n)} / ${clock(video.total)}`;
  seek.setAttribute("aria-valuetext", `${clock(n)} of ${clock(video.total)}`);
}

// The soundtrack's position as heard, not as scheduled.
function heard() {
  const stamp = audio.getOutputTimestamp?.();
  if (stamp?.performanceTime) {
    return stamp.contextTime + (performance.now() - stamp.performanceTime) / 1000;
  }
  return audio.currentTime - (audio.outputLatency || audio.baseLatency || 0);
}

// The frame due now, by the soundtrack's clock; never before the one it
// began from, in the moment before the soundtrack starts.
const due = () => Math.max(origin, Math.floor((heard() - start) * video.fps));

// Where the video stands, playing or not, whether or not a frame has been
// drawn since it began.
const position = () => (source ? Math.min(video.total - 1, due()) : frame);

function tick() {
  if (!source) return;
  const n = due();
  if (n >= video.total) return finish();
  if (n !== shown) draw(n);
  requestAnimationFrame(tick);
}

function setState(state) {
  player.dataset.state = state;
  const playing = state === "playing";
  toggle.setAttribute("aria-label", playing ? "Pause" : "Play");
  big.hidden = !["poster", "loading", "ended"].includes(state);
  big.setAttribute("aria-busy", String(state === "loading"));
  bigLabel.textContent = { loading: "Loading", ended: "Play again" }[state] ?? "Play";
}

async function play() {
  if (source || player.dataset.state === "loading") return;
  // made and resumed within the press, or browsers keep it silent
  audio ??= new AudioContext();
  audio.resume();
  if (navigator.audioSession) navigator.audioSession.type = "playback";
  if (!video) {
    setState("loading");
    try {
      await load();
    } catch (e) {
      return fail(e);
    }
  }
  begin(player.dataset.state === "ended" ? 0 : frame);
}

function begin(from) {
  if (!gain) {
    gain = audio.createGain();
    gain.connect(audio.destination);
    setLevel();
  }
  source = audio.createBufferSource();
  source.buffer = video.score;
  source.connect(gain);
  source.onended = finish;
  const when = audio.currentTime + 0.05;
  start = when - from / video.fps;
  source.start(when, from / video.fps);
  origin = from;
  shown = -1;
  setState("playing");
  requestAnimationFrame(tick);
}

// Stops the soundtrack, keeping where it was as where to go on from.
function halt() {
  if (!source) return;
  frame = position();
  source.onended = null;
  source.stop();
  source.disconnect();
  source = null;
}

function pause() {
  halt();
  if (frame !== shown) draw(frame);
  setState("paused");
}

function finish() {
  halt();
  frame = 0;
  draw(video.total - 1);
  // the whole length, not the start of the last frame's second
  time.textContent = `${clock(video.total)} / ${clock(video.total)}`;
  lastSecond = -1;
  setState("ended");
}

function togglePlay() {
  if (player.dataset.state === "playing") pause();
  else play();
}

function seekTo(n) {
  if (!video) return;
  n = Math.max(0, Math.min(video.total - 1, Math.round(n)));
  if (source) {
    halt();
    begin(n);
  } else {
    frame = n;
    draw(n);
    if (player.dataset.state !== "playing") setState("paused");
  }
}

const skip = (seconds) => seekTo(position() + seconds * video.fps);

function setLevel() {
  if (gain) gain.gain.value = muted ? 0 : level;
  player.classList.toggle("player-muted", muted || level === 0);
  mute.setAttribute("aria-label", muted || level === 0 ? "Unmute" : "Mute");
  volume.value = String(muted ? 0 : level);
}

function toggleMute() {
  if (muted || level === 0) {
    muted = false;
    if (level === 0) level = 1;
  } else {
    muted = true;
  }
  setLevel();
}

const fullscreen = () => document.fullscreenElement ?? document.webkitFullscreenElement;
function toggleFull() {
  if (fullscreen()) {
    (document.exitFullscreen ?? document.webkitExitFullscreen).call(document);
  } else {
    const request = player.requestFullscreen ?? player.webkitRequestFullscreen;
    Promise.resolve(request.call(player)).catch(() => {});
  }
}
function showFull() {
  full.setAttribute("aria-label", fullscreen() === player ? "Exit full screen" : "Full screen");
}

function fail(e) {
  console.error("The video couldn't be drawn here:", e);
  halt();
  setState("failed");
  big.hidden = true;
  bar.hidden = true;
  note.hidden = false;
}

// The controls stay out of the way while the video plays, until the pointer
// moves over it.
let sleep = 0;
function wake() {
  player.classList.add("player-awake");
  clearTimeout(sleep);
  sleep = setTimeout(() => player.classList.remove("player-awake"), 2500);
}

let pointer = "mouse";
player.addEventListener("pointerdown", (e) => {
  pointer = e.pointerType;
});
player.addEventListener("pointermove", wake);
big.addEventListener("click", play);
toggle.addEventListener("click", togglePlay);
screen.addEventListener("click", () => {
  // a tap on a playing video shows its controls first, as on phones
  const asleep = !player.classList.contains("player-awake");
  wake();
  if (pointer === "touch" && player.dataset.state === "playing" && asleep) return;
  if (player.dataset.state !== "failed") togglePlay();
});
screen.addEventListener("dblclick", toggleFull);

// A drag along the bar pauses the soundtrack and plays on where it ends,
// whether or not the bar moved.
function release() {
  if (!resume) return;
  resume = false;
  begin(frame);
}
seek.addEventListener("pointerdown", () => {
  if (!source) return;
  halt();
  resume = true;
  addEventListener("pointerup", release, { once: true });
  addEventListener("pointercancel", release, { once: true });
});
seek.addEventListener("input", () => {
  const n = Number(seek.value);
  if (source) return seekTo(n);
  frame = n;
  draw(n);
  if (!resume && player.dataset.state !== "playing") setState("paused");
});

mute.addEventListener("click", toggleMute);
volume.addEventListener("input", () => {
  level = Number(volume.value);
  muted = false;
  setLevel();
});

if (document.fullscreenEnabled || document.webkitFullscreenEnabled) {
  full.addEventListener("click", toggleFull);
  document.addEventListener("fullscreenchange", showFull);
  document.addEventListener("webkitfullscreenchange", showFull);
} else {
  full.hidden = true;
}

// The keys of the usual web players, while focus is in the video.
player.addEventListener("keydown", (e) => {
  if (e.altKey || e.ctrlKey || e.metaKey || player.dataset.state === "failed") return;
  const onButton = e.target.closest("button");
  const onVolume = e.target === volume;
  wake();
  switch (e.key) {
    case " ":
      if (onButton) return;
      togglePlay();
      break;
    case "k":
    case "K":
      togglePlay();
      break;
    case "ArrowLeft":
    case "ArrowRight":
      if (onVolume || !video) return;
      skip(e.key === "ArrowLeft" ? -5 : 5);
      break;
    case "j":
    case "J":
    case "l":
    case "L":
      if (!video) return;
      skip(e.key.toLowerCase() === "j" ? -10 : 10);
      break;
    case "Home":
    case "End":
      if (onVolume || !video) return;
      seekTo(e.key === "Home" ? 0 : video.total - 1);
      break;
    case "m":
    case "M":
      toggleMute();
      break;
    case "f":
    case "F":
      if (full.hidden) return;
      toggleFull();
      break;
    default:
      if (!/^[0-9]$/.test(e.key) || !video) return;
      seekTo((video.total * Number(e.key)) / 10);
  }
  e.preventDefault();
});

setState("poster");
big.hidden = false;
bar.hidden = false;

// Fetched once the page has loaded and the browser is idle; the pointer
// coming over the video starts it sooner. With Save Data, Play starts it.
if (!navigator.connection?.saveData) {
  const quietly = () => load().catch(() => {});
  const later = () =>
    window.requestIdleCallback ? requestIdleCallback(quietly, { timeout: 3000 }) : setTimeout(quietly, 500);
  if (document.readyState === "complete") later();
  else addEventListener("load", later, { once: true });
  player.addEventListener("pointerenter", quietly, { once: true });
}
