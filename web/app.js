import {
  computeAirDelta,
  createAirState,
  detectIOS,
  resetAirMotion,
  updateGravityUp,
} from './air-kinematics.js';

const OP = {
  MOVE: 0x01,
  MOUSE_BTN: 0x02,
  SCROLL: 0x03,
  PINCH: 0x04,
  KEY: 0x05,
  TEXT: 0x06,
  PING: 0x07,
  RELEASE_ALL: 0x08,
  PONG: 0x87,
};

const BTN = { LEFT: 0, RIGHT: 1, MIDDLE: 2 };
const ACT = { UP: 0, DOWN: 1, CLICK: 2 };

const STORAGE_KEY = 'airpad_settings_v1';
const TOKEN_KEY = 'airpad_token_v1';

const defaultSettings = {
  trackpadSens: 1.3,
  scrollSpeed: 1.0,
  airSens: 1.2,
  naturalScroll: true,
  tapClick: true,
  invertAirX: false,
  invertAirY: false,
  soundClick: false,
};

function loadSettings() {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return { ...defaultSettings };
    return { ...defaultSettings, ...JSON.parse(raw) };
  } catch {
    return { ...defaultSettings };
  }
}

function saveSettings(s) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(s));
  } catch {
    // ignore storage errors
  }
}

function resolveToken() {
  const url = new URL(window.location.href);
  const fromQuery = url.searchParams.get('token');
  if (fromQuery) {
    sessionStorage.setItem(TOKEN_KEY, fromQuery);
    url.searchParams.delete('token');
    window.history.replaceState({}, '', url.toString());
    return fromQuery;
  }
  return sessionStorage.getItem(TOKEN_KEY) || '';
}

const settings = loadSettings();
const pairingToken = resolveToken();
const textEncoder = new TextEncoder();

// DOM references
const statusPill = document.getElementById('statusPill');
const statusText = document.getElementById('statusText');
const latencyText = document.getElementById('latencyText');

const btnModeTrackpad = document.getElementById('btnModeTrackpad');
const btnModeAir = document.getElementById('btnModeAir');
const trackpadPanel = document.getElementById('trackpadPanel');
const airPanel = document.getElementById('airPanel');

const trackpadSurface = document.getElementById('trackpadSurface');
const touchRipples = document.getElementById('touchRipples');
const gestureHints = document.getElementById('gestureHints');
const dragBadge = document.getElementById('dragBadge');

const motionPermBanner = document.getElementById('motionPermBanner');
const btnEnableMotion = document.getElementById('btnEnableMotion');
const btnAirClutch = document.getElementById('btnAirClutch');
const airCrosshair = document.getElementById('airCrosshair');
const airDot = document.getElementById('airDot');
const airStatusLabel = document.getElementById('airStatusLabel');

const btnKeyboard = document.getElementById('btnKeyboard');
const keyboardBar = document.getElementById('keyboardBar');
const hiddenTextInput = document.getElementById('hiddenTextInput');
const btnFocusInput = document.getElementById('btnFocusInput');

const pausedOverlay = document.getElementById('pausedOverlay');
const authOverlay = document.getElementById('authOverlay');
const authErrorTitle = document.getElementById('authErrorTitle');
const authErrorSub = document.getElementById('authErrorSub');
const btnRetryConnect = document.getElementById('btnRetryConnect');

const btnSettings = document.getElementById('btnSettings');
const settingsModal = document.getElementById('settingsModal');
const btnCloseSettings = document.getElementById('btnCloseSettings');

const rngTrackpadSens = document.getElementById('rngTrackpadSens');
const valTrackpadSens = document.getElementById('valTrackpadSens');
const rngScrollSpeed = document.getElementById('rngScrollSpeed');
const valScrollSpeed = document.getElementById('valScrollSpeed');
const rngAirSens = document.getElementById('rngAirSens');
const valAirSens = document.getElementById('valAirSens');
const chkNaturalScroll = document.getElementById('chkNaturalScroll');
const chkTapClick = document.getElementById('chkTapClick');
const chkInvertAirX = document.getElementById('chkInvertAirX');
const chkInvertAirY = document.getElementById('chkInvertAirY');
const chkSoundClick = document.getElementById('chkSoundClick');
const btnReleaseAll = document.getElementById('btnReleaseAll');

// Web Audio click synthesizer
let audioCtx = null;
function playClickSound(pitch = 680) {
  if (!settings.soundClick) return;
  try {
    if (!audioCtx) {
      const Ctx = window.AudioContext || window.webkitAudioContext;
      if (!Ctx) return;
      audioCtx = new Ctx();
    }
    if (audioCtx.state === 'suspended') {
      audioCtx.resume();
    }
    const now = audioCtx.currentTime;
    const osc = audioCtx.createOscillator();
    const gain = audioCtx.createGain();
    osc.type = 'sine';
    osc.frequency.setValueAtTime(pitch, now);
    gain.gain.setValueAtTime(0.05, now);
    gain.gain.exponentialRampToValueAtTime(0.001, now + 0.025);
    osc.connect(gain);
    gain.connect(audioCtx.destination);
    osc.start(now);
    osc.stop(now + 0.03);
  } catch {
    // ignore audio errors
  }
}

// Wake Lock
let wakeLock = null;
async function requestWakeLock() {
  if (!('wakeLock' in navigator)) return;
  try {
    if (!wakeLock) {
      wakeLock = await navigator.wakeLock.request('screen');
      wakeLock.addEventListener('release', () => {
        wakeLock = null;
      });
    }
  } catch {
    // iOS may reject before user gesture; we retry on interactions
  }
}
document.addEventListener('visibilitychange', () => {
  if (document.visibilityState === 'visible') {
    requestWakeLock();
  }
});

// WebSocket Connection & Binary Protocol
let ws = null;
let reconnectTimer = null;
let pingTimer = null;
let reconnectAttempt = 0;
let serverPaused = false;
let currentMode = 'trackpad'; // 'trackpad' | 'air'

function setStatus(state, label) {
  statusPill.dataset.state = state;
  statusText.textContent = label;
  if (state !== 'connected') {
    latencyText.textContent = '';
  }
}

function sendBinary(buf) {
  if (ws && ws.readyState === WebSocket.OPEN) {
    ws.send(buf);
  }
}

function sendMove(dx, dy) {
  if (serverPaused || (dx === 0 && dy === 0)) return;
  const buf = new ArrayBuffer(9);
  const view = new DataView(buf);
  view.setUint8(0, OP.MOVE);
  view.setFloat32(1, dx, true);
  view.setFloat32(5, dy, true);
  sendBinary(buf);
}

function sendMouseButton(button, action) {
  if (serverPaused) return;
  const buf = new ArrayBuffer(3);
  const view = new DataView(buf);
  view.setUint8(0, OP.MOUSE_BTN);
  view.setUint8(1, button);
  view.setUint8(2, action);
  sendBinary(buf);
}

function sendScroll(dx, dy) {
  if (serverPaused || (dx === 0 && dy === 0)) return;
  const buf = new ArrayBuffer(9);
  const view = new DataView(buf);
  view.setUint8(0, OP.SCROLL);
  view.setFloat32(1, dx, true);
  view.setFloat32(5, dy, true);
  sendBinary(buf);
}

function sendPinch(scaleDelta) {
  if (serverPaused || scaleDelta === 0) return;
  const buf = new ArrayBuffer(5);
  const view = new DataView(buf);
  view.setUint8(0, OP.PINCH);
  view.setFloat32(1, scaleDelta, true);
  sendBinary(buf);
}

function sendKey(vk, action) {
  if (serverPaused) return;
  const buf = new ArrayBuffer(4);
  const view = new DataView(buf);
  view.setUint8(0, OP.KEY);
  view.setUint16(1, vk, true);
  view.setUint8(3, action);
  sendBinary(buf);
}

function sendText(text) {
  if (serverPaused || !text) return;
  const encoded = textEncoder.encode(text);
  const len = Math.min(encoded.length, 65535);
  const buf = new ArrayBuffer(3 + len);
  const view = new DataView(buf);
  view.setUint8(0, OP.TEXT);
  view.setUint16(1, len, true);
  new Uint8Array(buf, 3).set(encoded.subarray(0, len));
  sendBinary(buf);
}

function sendReleaseAll() {
  const buf = new ArrayBuffer(1);
  new DataView(buf).setUint8(0, OP.RELEASE_ALL);
  sendBinary(buf);
}

function sendPing() {
  if (!ws || ws.readyState !== WebSocket.OPEN) return;
  const buf = new ArrayBuffer(9);
  const view = new DataView(buf);
  view.setUint8(0, OP.PING);
  const now = BigInt(Date.now());
  view.setBigUint64(1, now, true);
  sendBinary(buf);
}

function connect() {
  clearTimeout(reconnectTimer);
  clearInterval(pingTimer);

  if (!pairingToken) {
    setStatus('error', 'No Token');
    authErrorTitle.textContent = 'Pairing Token Missing';
    authErrorSub.textContent = 'Scan the QR code from AirPad on your Windows PC to open with a valid token.';
    authOverlay.classList.remove('hidden');
    return;
  }

  authOverlay.classList.add('hidden');
  setStatus('connecting', 'Connecting…');

  const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  const wsUrl = `${proto}//${window.location.host}/ws?token=${encodeURIComponent(pairingToken)}`;

  try {
    ws = new WebSocket(wsUrl);
  } catch {
    scheduleReconnect();
    return;
  }
  ws.binaryType = 'arraybuffer';

  ws.addEventListener('open', () => {
    reconnectAttempt = 0;
    setStatus('connected', 'Connected');
    sendPing();
    pingTimer = setInterval(sendPing, 2000);
  });

  ws.addEventListener('message', (event) => {
    if (event.data instanceof ArrayBuffer) {
      const view = new DataView(event.data);
      if (view.byteLength >= 9 && view.getUint8(0) === OP.PONG) {
        const sentAt = Number(view.getBigUint64(1, true));
        const rtt = Math.max(0, Date.now() - sentAt);
        latencyText.textContent = `${rtt}ms`;
      }
    } else if (typeof event.data === 'string') {
      try {
        const msg = JSON.parse(event.data);
        if (typeof msg.paused === 'boolean') {
          serverPaused = msg.paused;
          pausedOverlay.classList.toggle('hidden', !serverPaused);
          if (serverPaused) {
            setStatus('connecting', 'Paused');
          } else if (ws && ws.readyState === WebSocket.OPEN) {
            setStatus('connected', 'Connected');
          }
        }
      } catch {
        // ignore malformed status JSON
      }
    }
  });

  ws.addEventListener('close', (event) => {
    clearInterval(pingTimer);
    if (event.code === 4000) {
      setStatus('error', 'Replaced');
      authErrorTitle.textContent = 'Disconnected';
      authErrorSub.textContent = event.reason || 'Another controller took over this session.';
      authOverlay.classList.remove('hidden');
      return;
    }
    if (event.code === 4001 || event.code === 1008) {
      setStatus('error', 'Rejected');
      authErrorTitle.textContent = 'Invalid Pairing Token';
      authErrorSub.textContent = 'The token expired or was rotated on the PC. Re-scan the QR code.';
      authOverlay.classList.remove('hidden');
      return;
    }
    setStatus('connecting', 'Reconnecting…');
    scheduleReconnect();
  });

  ws.addEventListener('error', () => {
    // close handler will schedule reconnect
  });
}

function scheduleReconnect() {
  clearTimeout(reconnectTimer);
  const delay = Math.min(3000, 350 * Math.pow(1.5, reconnectAttempt++));
  reconnectTimer = setTimeout(connect, delay);
}

btnRetryConnect.addEventListener('click', () => {
  reconnectAttempt = 0;
  connect();
});

// Mode Switching
function setMode(mode) {
  currentMode = mode;
  const isTrackpad = mode === 'trackpad';
  btnModeTrackpad.classList.toggle('active', isTrackpad);
  btnModeTrackpad.setAttribute('aria-selected', String(isTrackpad));
  btnModeAir.classList.toggle('active', !isTrackpad);
  btnModeAir.setAttribute('aria-selected', String(!isTrackpad));

  trackpadPanel.classList.toggle('active', isTrackpad);
  trackpadPanel.classList.toggle('hidden', !isTrackpad);
  airPanel.classList.toggle('active', !isTrackpad);
  airPanel.classList.toggle('hidden', isTrackpad);

  sendReleaseAll();
  lastMotionTs = 0;
  resetAirMotion(airState, !isTrackpad);
  if (!isTrackpad) {
    setupMotionSensors();
  }
}

btnModeTrackpad.addEventListener('click', () => setMode('trackpad'));
btnModeAir.addEventListener('click', () => setMode('air'));

// Trackpad Multi-Touch State Machine
const activeTouches = new Map();
const rippleElements = new Map();

let gestureMaxFingers = 0;
let gestureStartTime = 0;
let gestureTotalTravel = 0;
let gestureMoved = false;
let lastTapTime = 0;
let pendingTapDrag = false;
let isDragging = false;

let prevTwoMid = null;
let prevTwoDist = null;

function renderTouchDots() {
  const surfaceRect = trackpadSurface.getBoundingClientRect();
  for (const [id, el] of rippleElements.entries()) {
    if (!activeTouches.has(id)) {
      el.remove();
      rippleElements.delete(id);
    }
  }
  for (const [id, pos] of activeTouches.entries()) {
    let el = rippleElements.get(id);
    if (!el) {
      el = document.createElement('div');
      el.className = 'touch-dot';
      touchRipples.appendChild(el);
      rippleElements.set(id, el);
    }
    el.style.left = `${pos.x - surfaceRect.left}px`;
    el.style.top = `${pos.y - surfaceRect.top}px`;
  }
}

function twoFingerMetrics() {
  const pts = Array.from(activeTouches.values());
  if (pts.length < 2) return null;
  const a = pts[0];
  const b = pts[1];
  return {
    mx: (a.x + b.x) * 0.5,
    my: (a.y + b.y) * 0.5,
    dist: Math.hypot(b.x - a.x, b.y - a.y),
  };
}

trackpadSurface.addEventListener('touchstart', (e) => {
  e.preventDefault();
  requestWakeLock();
  gestureHints.classList.add('faded');

  const now = performance.now();
  const wasEmpty = activeTouches.size === 0;

  for (const t of e.changedTouches) {
    activeTouches.set(t.identifier, { x: t.clientX, y: t.clientY });
  }

  if (wasEmpty) {
    gestureMaxFingers = activeTouches.size;
    gestureStartTime = now;
    gestureTotalTravel = 0;
    gestureMoved = false;
    pendingTapDrag = (activeTouches.size === 1) && (now - lastTapTime < 280);
  } else {
    gestureMaxFingers = Math.max(gestureMaxFingers, activeTouches.size);
    pendingTapDrag = false;
  }

  if (activeTouches.size >= 2) {
    const m = twoFingerMetrics();
    prevTwoMid = { x: m.mx, y: m.my };
    prevTwoDist = m.dist;
  }

  renderTouchDots();
}, { passive: false });

trackpadSurface.addEventListener('touchmove', (e) => {
  e.preventDefault();

  let singleDx = 0;
  let singleDy = 0;

  for (const t of e.changedTouches) {
    const prev = activeTouches.get(t.identifier);
    if (prev) {
      const dx = t.clientX - prev.x;
      const dy = t.clientY - prev.y;
      if (activeTouches.size === 1) {
        singleDx += dx;
        singleDy += dy;
      }
      gestureTotalTravel += Math.hypot(dx, dy);
      prev.x = t.clientX;
      prev.y = t.clientY;
    }
  }

  if (gestureTotalTravel > 6) {
    gestureMoved = true;
  }

  if (activeTouches.size === 1 && gestureMaxFingers === 1) {
    if (pendingTapDrag && !isDragging && gestureTotalTravel > 5) {
      isDragging = true;
      pendingTapDrag = false;
      sendMouseButton(BTN.LEFT, ACT.DOWN);
      dragBadge.classList.remove('hidden');
    }

    if (singleDx !== 0 || singleDy !== 0) {
      const speed = Math.hypot(singleDx, singleDy);
      const accel = 1.0 + Math.min(speed / 18, 1.35);
      const gain = settings.trackpadSens * accel;
      sendMove(singleDx * gain, singleDy * gain);
    }
  } else if (activeTouches.size === 2) {
    const m = twoFingerMetrics();
    if (m && prevTwoMid !== null && prevTwoDist !== null) {
      const dmx = m.mx - prevTwoMid.x;
      const dmy = m.my - prevTwoMid.y;
      const dDist = m.dist - prevTwoDist;

      if (Math.abs(dDist) > 2.4 && Math.abs(dDist) > Math.hypot(dmx, dmy) * 0.95) {
        sendPinch(dDist * 0.035);
      } else if (dmx !== 0 || dmy !== 0) {
        const dir = settings.naturalScroll ? 1 : -1;
        const scale = settings.scrollSpeed * 0.065;
        // On Windows WHEEL_DELTA positive = scroll UP (content moves down).
        // Natural scrolling: dragging finger DOWN (dmy > 0) pushes content down -> positive wheel.
        sendScroll(-dmx * scale * dir, dmy * scale * dir);
      }
      prevTwoMid = { x: m.mx, y: m.my };
      prevTwoDist = m.dist;
    }
  }

  renderTouchDots();
}, { passive: false });

function handleTouchEnd(e) {
  e.preventDefault();
  const now = performance.now();

  for (const t of e.changedTouches) {
    activeTouches.delete(t.identifier);
  }

  if (activeTouches.size < 2) {
    prevTwoMid = null;
    prevTwoDist = null;
  }

  if (activeTouches.size === 0) {
    if (isDragging) {
      isDragging = false;
      sendMouseButton(BTN.LEFT, ACT.UP);
      dragBadge.classList.add('hidden');
    } else {
      const duration = now - gestureStartTime;
      const isTap = !gestureMoved && duration < 250 && gestureTotalTravel < 12;
      if (isTap && settings.tapClick) {
        if (gestureMaxFingers === 1) {
          sendMouseButton(BTN.LEFT, ACT.CLICK);
          playClickSound(720);
          lastTapTime = now;
        } else if (gestureMaxFingers === 2) {
          sendMouseButton(BTN.RIGHT, ACT.CLICK);
          playClickSound(540);
        } else if (gestureMaxFingers === 3) {
          sendMouseButton(BTN.MIDDLE, ACT.CLICK);
          playClickSound(620);
        }
      }
    }
    pendingTapDrag = false;
    gestureMaxFingers = 0;
  }

  renderTouchDots();
}

trackpadSurface.addEventListener('touchend', handleTouchEnd, { passive: false });
trackpadSurface.addEventListener('touchcancel', handleTouchEnd, { passive: false });

// Physical-style Bottom Mouse Buttons
for (const btnEl of document.querySelectorAll('.mouse-btn')) {
  const btnId = Number(btnEl.dataset.btn);
  btnEl.addEventListener('pointerdown', (e) => {
    e.preventDefault();
    requestWakeLock();
    btnEl.setPointerCapture(e.pointerId);
    btnEl.classList.add('pressed');
    sendMouseButton(btnId, ACT.DOWN);
    playClickSound(btnId === BTN.RIGHT ? 540 : 720);
  });

  const releaseBtn = (e) => {
    if (btnEl.classList.contains('pressed')) {
      e.preventDefault();
      btnEl.classList.remove('pressed');
      sendMouseButton(btnId, ACT.UP);
    }
  };
  btnEl.addEventListener('pointerup', releaseBtn);
  btnEl.addEventListener('pointercancel', releaseBtn);
}

// Mid-Air Gyroscope Mode
const isIOSDevice = detectIOS();
const airState = createAirState();
let motionPermissionGranted = false;
let motionListenerAttached = false;
let airClutchHeld = false;
let lastMotionTs = 0;
let airDotX = 0;
let airDotY = 0;

function updateAirVisual(paused, dx = 0, dy = 0) {
  airCrosshair.classList.toggle('paused', paused);
  airStatusLabel.textContent = paused ? 'Paused (Re-centering)' : 'Aiming Active';
  if (paused) {
    airDotX *= 0.6;
    airDotY *= 0.6;
  } else {
    airDotX = Math.max(-44, Math.min(44, airDotX * 0.72 + dx * 1.1));
    airDotY = Math.max(-44, Math.min(44, airDotY * 0.72 + dy * 1.1));
  }
  airDot.style.transform = `translate(${airDotX.toFixed(1)}px, ${airDotY.toFixed(1)}px)`;
}

function onDeviceMotion(event) {
  if (currentMode !== 'air' || serverPaused) return;
  const rr = event.rotationRate;
  if (!rr) return;

  const now = performance.now();
  const dt = lastMotionTs > 0
    ? Math.min(Math.max((now - lastMotionTs) / 1000, 0.004), 0.05)
    : 0.016;
  lastMotionTs = now;

  if (airClutchHeld) {
    // Track wrist posture while clutching so release starts aligned to the new grip
    updateGravityUp(airState, event.accelerationIncludingGravity, isIOSDevice);
    resetAirMotion(airState, false);
    updateAirVisual(true);
    return;
  }

  const { dx, dy } = computeAirDelta(
    rr,
    event.accelerationIncludingGravity,
    dt,
    airState,
    {
      sensitivity: settings.airSens,
      invertX: settings.invertAirX,
      invertY: settings.invertAirY,
      isIOS: isIOSDevice,
    },
  );

  if (dx !== 0 || dy !== 0) {
    sendMove(dx, dy);
  }
  updateAirVisual(false, dx, dy);
}

function attachMotionListener() {
  if (motionListenerAttached) return;
  window.addEventListener('devicemotion', onDeviceMotion);
  motionListenerAttached = true;
}

function setupMotionSensors() {
  if (motionPermissionGranted) {
    motionPermBanner.classList.add('hidden');
    attachMotionListener();
    return;
  }

  if (typeof DeviceMotionEvent !== 'undefined' && typeof DeviceMotionEvent.requestPermission === 'function') {
    motionPermBanner.classList.remove('hidden');
  } else if ('DeviceMotionEvent' in window) {
    motionPermissionGranted = true;
    motionPermBanner.classList.add('hidden');
    attachMotionListener();
  } else {
    motionPermBanner.classList.remove('hidden');
    btnEnableMotion.disabled = true;
    btnEnableMotion.textContent = 'Gyroscope Unavailable (Requires HTTPS)';
  }
}

btnEnableMotion.addEventListener('click', async () => {
  requestWakeLock();
  try {
    if (typeof DeviceMotionEvent !== 'undefined' && typeof DeviceMotionEvent.requestPermission === 'function') {
      const res = await DeviceMotionEvent.requestPermission();
      if (res === 'granted') {
        motionPermissionGranted = true;
        motionPermBanner.classList.add('hidden');
        attachMotionListener();
      } else {
        airStatusLabel.textContent = 'Motion permission denied in Safari';
      }
    }
  } catch (err) {
    airStatusLabel.textContent = 'Motion permission requires HTTPS trust';
  }
});

btnAirClutch.addEventListener('pointerdown', (e) => {
  e.preventDefault();
  btnAirClutch.setPointerCapture(e.pointerId);
  airClutchHeld = true;
  resetAirMotion(airState, false);
  btnAirClutch.classList.add('held');
  updateAirVisual(true);
});

const releaseClutch = (e) => {
  if (airClutchHeld) {
    e.preventDefault();
    airClutchHeld = false;
    btnAirClutch.classList.remove('held');
    resetAirMotion(airState, false);
    lastMotionTs = performance.now();
    updateAirVisual(false);
  }
};
btnAirClutch.addEventListener('pointerup', releaseClutch);
btnAirClutch.addEventListener('pointercancel', releaseClutch);

// Keyboard & Modifiers
const stickyModifiers = new Set();

function updateModifierUI() {
  for (const el of document.querySelectorAll('.mod-key')) {
    const vk = Number(el.dataset.mod);
    el.classList.toggle('active', stickyModifiers.has(vk));
  }
}

function sendKeyWithStickyModifiers(vk) {
  for (const modVk of stickyModifiers) {
    sendKey(modVk, ACT.DOWN);
  }
  sendKey(vk, ACT.CLICK);
  for (const modVk of stickyModifiers) {
    sendKey(modVk, ACT.UP);
  }
  if (stickyModifiers.size > 0) {
    stickyModifiers.clear();
    updateModifierUI();
  }
}

btnKeyboard.addEventListener('click', () => {
  const willShow = keyboardBar.classList.contains('hidden');
  keyboardBar.classList.toggle('hidden', !willShow);
  btnKeyboard.classList.toggle('active', willShow);
  if (willShow) {
    hiddenTextInput.focus();
  } else {
    hiddenTextInput.blur();
  }
});

btnFocusInput.addEventListener('click', () => {
  hiddenTextInput.focus();
});

for (const el of document.querySelectorAll('.mod-key')) {
  const modVk = Number(el.dataset.mod);
  el.addEventListener('click', () => {
    if (stickyModifiers.has(modVk)) {
      stickyModifiers.delete(modVk);
    } else {
      stickyModifiers.add(modVk);
    }
    updateModifierUI();
  });
}

for (const el of document.querySelectorAll('.kbd-key[data-vk]')) {
  const vk = Number(el.dataset.vk);
  el.addEventListener('click', () => {
    playClickSound(640);
    sendKeyWithStickyModifiers(vk);
  });
}

let isComposing = false;
hiddenTextInput.addEventListener('compositionstart', () => {
  isComposing = true;
});
hiddenTextInput.addEventListener('compositionend', () => {
  isComposing = false;
  const val = hiddenTextInput.value;
  if (val) {
    sendText(val);
    hiddenTextInput.value = '';
  }
});

hiddenTextInput.addEventListener('input', () => {
  if (isComposing) return;
  const val = hiddenTextInput.value;
  if (val) {
    if (stickyModifiers.size > 0 && val.length === 1) {
      const ch = val.toUpperCase();
      const code = ch.charCodeAt(0);
      if ((code >= 65 && code <= 90) || (code >= 48 && code <= 57)) {
        sendKeyWithStickyModifiers(code);
        hiddenTextInput.value = '';
        return;
      }
    }
    sendText(val);
    hiddenTextInput.value = '';
  }
});

hiddenTextInput.addEventListener('keydown', (e) => {
  if (e.key === 'Backspace' && hiddenTextInput.value === '') {
    e.preventDefault();
    sendKeyWithStickyModifiers(8);
  } else if (e.key === 'Enter') {
    e.preventDefault();
    if (hiddenTextInput.value) {
      sendText(hiddenTextInput.value);
      hiddenTextInput.value = '';
    }
    sendKeyWithStickyModifiers(13);
  } else if (e.key === 'Escape') {
    e.preventDefault();
    sendKeyWithStickyModifiers(27);
  } else if (e.key === 'Tab') {
    e.preventDefault();
    sendKeyWithStickyModifiers(9);
  }
});

// Settings Modal
function syncSettingsUI() {
  rngTrackpadSens.value = String(settings.trackpadSens);
  valTrackpadSens.textContent = `${settings.trackpadSens.toFixed(1)}x`;

  rngScrollSpeed.value = String(settings.scrollSpeed);
  valScrollSpeed.textContent = `${settings.scrollSpeed.toFixed(1)}x`;

  rngAirSens.value = String(settings.airSens);
  valAirSens.textContent = `${settings.airSens.toFixed(1)}x`;

  chkNaturalScroll.checked = settings.naturalScroll;
  chkTapClick.checked = settings.tapClick;
  chkInvertAirX.checked = settings.invertAirX;
  chkInvertAirY.checked = settings.invertAirY;
  chkSoundClick.checked = settings.soundClick;
}

btnSettings.addEventListener('click', () => {
  syncSettingsUI();
  settingsModal.classList.remove('hidden');
});

btnCloseSettings.addEventListener('click', () => {
  settingsModal.classList.add('hidden');
});

settingsModal.addEventListener('click', (e) => {
  if (e.target === settingsModal) {
    settingsModal.classList.add('hidden');
  }
});

rngTrackpadSens.addEventListener('input', () => {
  settings.trackpadSens = parseFloat(rngTrackpadSens.value);
  valTrackpadSens.textContent = `${settings.trackpadSens.toFixed(1)}x`;
  saveSettings(settings);
});

rngScrollSpeed.addEventListener('input', () => {
  settings.scrollSpeed = parseFloat(rngScrollSpeed.value);
  valScrollSpeed.textContent = `${settings.scrollSpeed.toFixed(1)}x`;
  saveSettings(settings);
});

rngAirSens.addEventListener('input', () => {
  settings.airSens = parseFloat(rngAirSens.value);
  valAirSens.textContent = `${settings.airSens.toFixed(1)}x`;
  saveSettings(settings);
});

chkNaturalScroll.addEventListener('change', () => {
  settings.naturalScroll = chkNaturalScroll.checked;
  saveSettings(settings);
});

chkTapClick.addEventListener('change', () => {
  settings.tapClick = chkTapClick.checked;
  saveSettings(settings);
});

chkInvertAirX.addEventListener('change', () => {
  settings.invertAirX = chkInvertAirX.checked;
  saveSettings(settings);
});

chkInvertAirY.addEventListener('change', () => {
  settings.invertAirY = chkInvertAirY.checked;
  saveSettings(settings);
});

chkSoundClick.addEventListener('change', () => {
  settings.soundClick = chkSoundClick.checked;
  saveSettings(settings);
});

btnReleaseAll.addEventListener('click', () => {
  stickyModifiers.clear();
  updateModifierUI();
  sendReleaseAll();
  settingsModal.classList.add('hidden');
});

// Start WebSocket on load
syncSettingsUI();
connect();
