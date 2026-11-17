// Pure 3D gravity-aligned Air-Mouse kinematics for iOS Safari / Android Chrome DeviceMotionEvent.
//
// W3C DeviceMotionEvent.rotationRate convention (deg/s, right-hand rule around device axes):
//   - alpha: angular velocity around +X (left -> right across screen; pitch up/back > 0)
//   - beta:  angular velocity around +Y (bottom -> top along screen; roll/upright-yaw)
//   - gamma: angular velocity around +Z (out of screen toward user; flat-table yaw)
//
// W3C accelerationIncludingGravity reports proper acceleration (pointing UP in Android/Chrome),
// whereas iOS Safari (CoreMotion) reports gravity vector (pointing DOWN). We normalize both
// into a unit world-Up vector (ux, uy, uz) in device coordinates, then project angular velocity
// onto world-Up (yaw) and gravity-horizontal right (pitch) so wrist roll produces zero drift
// and horizontal/vertical cursor axes always match human intuition regardless of tilt posture.

const DEFAULT_TILT_RAD = (35 * Math.PI) / 180;
export const DEFAULT_UP_X = 0;
export const DEFAULT_UP_Y = Math.sin(DEFAULT_TILT_RAD);
export const DEFAULT_UP_Z = Math.cos(DEFAULT_TILT_RAD);

export function createAirState() {
  return {
    ux: DEFAULT_UP_X,
    uy: DEFAULT_UP_Y,
    uz: DEFAULT_UP_Z,
    gravityInitialized: false,
    smoothYaw: 0,
    smoothPitch: 0,
  };
}

export function resetAirMotion(state, resetGravity = false) {
  state.smoothYaw = 0;
  state.smoothPitch = 0;
  if (resetGravity) {
    state.ux = DEFAULT_UP_X;
    state.uy = DEFAULT_UP_Y;
    state.uz = DEFAULT_UP_Z;
    state.gravityInitialized = false;
  }
}

export function detectIOS(nav = typeof navigator !== 'undefined' ? navigator : null) {
  if (!nav) return false;
  const ua = nav.userAgent || '';
  const platform = nav.platform || '';
  const maxTouch = nav.maxTouchPoints || 0;
  return /iPad|iPhone|iPod/.test(ua) || (platform === 'MacIntel' && maxTouch > 1);
}

export function updateGravityUp(state, accelInclG, isIOS = false) {
  if (!accelInclG) return false;
  const ax = accelInclG.x;
  const ay = accelInclG.y;
  const az = accelInclG.z;
  if (!Number.isFinite(ax) || !Number.isFinite(ay) || !Number.isFinite(az)) {
    return false;
  }

  const len = Math.hypot(ax, ay, az);
  // Reject free-fall or extreme linear acceleration spikes (0.3g .. 2.5g)
  if (len < 3.0 || len > 25.0) {
    return false;
  }

  const gravSign = isIOS ? -1 : 1;
  const sx = (ax / len) * gravSign;
  const sy = (ay / len) * gravSign;
  const sz = (az / len) * gravSign;

  if (!state.gravityInitialized) {
    state.ux = sx;
    state.uy = sy;
    state.uz = sz;
    state.gravityInitialized = true;
    return true;
  }

  const alpha = 0.22;
  const nx = state.ux * (1 - alpha) + sx * alpha;
  const ny = state.uy * (1 - alpha) + sy * alpha;
  const nz = state.uz * (1 - alpha) + sz * alpha;
  const nLen = Math.hypot(nx, ny, nz) || 1;
  state.ux = nx / nLen;
  state.uy = ny / nLen;
  state.uz = nz / nLen;
  return true;
}

export function computeAirDelta(rr, accelInclG, dt, state, opts = {}) {
  const {
    sensitivity = 1.2,
    invertX = false,
    invertY = false,
    isIOS = false,
  } = opts;

  updateGravityUp(state, accelInclG, isIOS);

  if (!rr) {
    return { dx: 0, dy: 0 };
  }

  const wx = Number.isFinite(rr.alpha) ? rr.alpha : 0;
  const wy = Number.isFinite(rr.beta) ? rr.beta : 0;
  const wz = Number.isFinite(rr.gamma) ? rr.gamma : 0;

  const ux = state.ux;
  const uy = state.uy;
  const uz = state.uz;

  // Horizontal yaw (around world Up): clockwise look-right is negative about +u
  const rawYawRate = -(wx * ux + wy * uy + wz * uz);

  // Horizontal right axis orthogonal to world Up: r = normalize(x_hat - ux * u_hat)
  let rx = 1 - ux * ux;
  let ry = -ux * uy;
  let rz = -ux * uz;
  const rLen = Math.hypot(rx, ry, rz);
  if (rLen > 0.25) {
    rx /= rLen;
    ry /= rLen;
    rz /= rLen;
  } else {
    // Fallback near extreme ~90 deg sideways roll
    rx = 1;
    ry = 0;
    rz = 0;
  }

  // Vertical pitch: pitching top edge / back aim upward gives +wx, Windows screen-up is -dy
  const rawPitchRate = -(wx * rx + wy * ry + wz * rz);

  // Light low-pass EMA to calm MEMS sensor jitter without adding noticeable lag
  const smoothAlpha = 0.58;
  state.smoothYaw = state.smoothYaw * (1 - smoothAlpha) + rawYawRate * smoothAlpha;
  state.smoothPitch = state.smoothPitch * (1 - smoothAlpha) + rawPitchRate * smoothAlpha;

  // Radial soft-knee deadzone prevents micro-tremor drift without axial stair-stepping
  const deadzone = 1.6;
  const mag = Math.hypot(state.smoothYaw, state.smoothPitch);
  if (mag <= deadzone) {
    return { dx: 0, dy: 0 };
  }

  const effMag = mag - deadzone;
  const dirX = state.smoothYaw / mag;
  const dirY = state.smoothPitch / mag;

  // Precision-to-flick curve: calm & accurate at slow aim, brisk sweep on fast turns
  const accelFactor = 0.72 + Math.min(1.45, Math.pow(effMag / 55, 1.15) * 0.95);
  const speedPxPerSec = effMag * 15.5 * sensitivity * accelFactor;

  let dx = dirX * speedPxPerSec * dt;
  let dy = dirY * speedPxPerSec * dt;

  if (invertX) dx = -dx;
  if (invertY) dy = -dy;

  return { dx, dy };
}
